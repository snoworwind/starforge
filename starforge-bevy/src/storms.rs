//! Severe weather events layered on top of the biome climate: lightning
//! storms, polar auroras, sandstorms and meteor showers.
//!
//! Events are transient (never saved) and driven by a small director that
//! picks one at a time from the biome, time of day and a cooldown. All visual
//! effects reuse existing systems: the screen flash comes from `ScreenFx`,
//! particles from the shared particle framework and fog from `DistanceFog`.

use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;

use crate::daynight::DayTime;
use crate::particles::{EmitOptions, ParticleStyle, ParticleSystem};
use crate::player::Player;
use crate::schedule::{GameState, ground_scene_mode};
use crate::space::FlightMode;
use crate::tween::exp_approach;
use crate::visual::ResolvedQuality;
use crate::world::World;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StormKind {
    None,
    Lightning,
    Aurora,
    Sandstorm,
    MeteorShower,
}

impl StormKind {
    pub fn label(self) -> &'static str {
        match self {
            StormKind::None => "平静",
            StormKind::Lightning => "雷暴",
            StormKind::Aurora => "极光",
            StormKind::Sandstorm => "沙暴",
            StormKind::MeteorShower => "流星雨",
        }
    }
}

#[derive(Resource)]
pub struct StormState {
    pub kind: StormKind,
    /// Seconds until the current event ends.
    pub remaining: f32,
    /// Cooldown until a new event may start.
    pub cooldown: f32,
    pub lightning_cd: f32,
    pub meteor_cd: f32,
    pub sand_cd: f32,
    /// Pending thunder sounds as (delay, volume).
    pub thunder_queue: Vec<(f32, f32)>,
    pub aurora_entities: Vec<Entity>,
    pub aurora_phase: f32,
    pub intensity: f32,
    pub announced: bool,
}

impl Default for StormState {
    fn default() -> Self {
        Self {
            kind: StormKind::None,
            remaining: 0.0,
            cooldown: 45.0,
            lightning_cd: 2.0,
            meteor_cd: 1.5,
            sand_cd: 0.0,
            thunder_queue: Vec::new(),
            aurora_entities: Vec::new(),
            aurora_phase: 0.0,
            intensity: 0.0,
            announced: false,
        }
    }
}

#[derive(Component)]
pub struct LightningFlash {
    pub life: f32,
}

#[derive(Component)]
pub struct AuroraRibbon {
    pub index: usize,
    pub material: Handle<StandardMaterial>,
}

#[derive(Component)]
pub struct Meteor {
    pub velocity: Vec3,
    pub life: f32,
    pub trail_t: f32,
}

/// Picks a new event for the current conditions. `biome` is the current voxel
/// biome key, `night` is 0..1 darkness.
pub fn choose_event(biome: &str, night: f32, rng: &mut crate::rng::Rng) -> (StormKind, f32, f32) {
    // Returns (kind, duration, cooldown).
    match biome {
        "ashen" | "ferrous" | "volcanic" | "obsidian" => {
            if rng.next() < 0.72 {
                (StormKind::Lightning, 45.0 + rng.next() * 50.0, 70.0)
            } else {
                (StormKind::MeteorShower, 30.0 + rng.next() * 30.0, 120.0)
            }
        }
        "frozen" | "crystal" => {
            if night > 0.55 {
                (StormKind::Aurora, 80.0 + rng.next() * 80.0, 90.0)
            } else {
                (StormKind::None, 0.0, 60.0)
            }
        }
        "desert" | "salt" => {
            let roll = rng.next();
            if roll < 0.5 {
                (StormKind::Sandstorm, 40.0 + rng.next() * 40.0, 80.0)
            } else if roll < 0.62 {
                (StormKind::Lightning, 30.0 + rng.next() * 30.0, 90.0)
            } else {
                (StormKind::None, 0.0, 50.0)
            }
        }
        "alien" | "redmoss" | "hive" => {
            if rng.next() < 0.35 {
                (StormKind::MeteorShower, 25.0 + rng.next() * 30.0, 140.0)
            } else {
                (StormKind::None, 0.0, 70.0)
            }
        }
        "lush" | "fungal" | "murk" | "ocean" | "amber" => {
            if rng.next() < 0.4 {
                (StormKind::Lightning, 35.0 + rng.next() * 30.0, 100.0)
            } else {
                (StormKind::None, 0.0, 60.0)
            }
        }
        _ => (StormKind::None, 0.0, 60.0),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn storm_director_system(
    time: Res<Time>,
    quality: Res<ResolvedQuality>,
    mode: Res<FlightMode>,
    world: Option<Res<World>>,
    day: Option<Res<DayTime>>,
    player: Query<&Player>,
    mut state: ResMut<StormState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut screen: Option<ResMut<crate::screen_fx::ScreenFx>>,
    mut big_ev: MessageWriter<crate::quests::BigMessageEvent>,
) {
    let dt = time.delta_secs().clamp(0.0, 0.1);
    if !quality.weather.effective || mode.space_scene() {
        if state.kind != StormKind::None {
            end_event(&mut state, &mut commands);
        }
        return;
    }
    let Ok(player) = player.single() else { return };
    state.remaining = (state.remaining - dt).max(0.0);
    if state.remaining <= 0.0 && state.kind != StormKind::None {
        end_event(&mut state, &mut commands);
    }
    state.cooldown = (state.cooldown - dt).max(0.0);
    if state.kind != StormKind::None || state.cooldown > 0.0 {
        return;
    }
    let Some(world) = world.as_deref() else {
        return;
    };
    let night = day
        .as_deref()
        .map(|day| 1.0 - crate::daynight::day_factor(day.0))
        .unwrap_or(0.0);
    let mut rng =
        crate::rng::Rng::new((time.elapsed_secs() * 100.0) as u32 ^ world.seed.wrapping_mul(31));
    let (kind, duration, cooldown) = choose_event(world.biome().key, night, &mut rng);
    state.kind = kind;
    state.remaining = duration;
    state.cooldown = cooldown;
    state.announced = false;
    state.intensity = 0.0;
    match kind {
        StormKind::Lightning => {
            state.lightning_cd = 1.5 + rng.next() * 2.5;
        }
        StormKind::MeteorShower => {
            state.meteor_cd = 0.8 + rng.next() * 1.5;
        }
        StormKind::Aurora => {
            spawn_aurora(
                &mut commands,
                &mut meshes,
                &mut materials,
                &mut state,
                player.pos,
            );
        }
        _ => {}
    }
    if kind != StormKind::None {
        big_ev.write(crate::quests::BigMessageEvent {
            title: format!("天气事件：{}", kind.label()),
            sub: event_subtitle(kind).to_string(),
            dur: 3.0,
        });
        state.announced = true;
        if let Some(screen) = screen.as_deref_mut() {
            screen.discover();
        }
    }
}

fn event_subtitle(kind: StormKind) -> &'static str {
    match kind {
        StormKind::Lightning => "远离空旷高地，雷击会造成伤害",
        StormKind::Aurora => "极光在夜空中流动",
        StormKind::Sandstorm => "能见度骤降，注意呼吸过滤",
        StormKind::MeteorShower => "陨石带来稀有金属",
        StormKind::None => "",
    }
}

fn end_event(state: &mut StormState, commands: &mut Commands) {
    state.kind = StormKind::None;
    state.remaining = 0.0;
    for entity in state.aurora_entities.drain(..) {
        commands.entity(entity).despawn();
    }
    state.intensity = 0.0;
}

/// Lightning: screen flash, thunder and occasional strikes near the player.
#[allow(clippy::too_many_arguments)]
pub fn lightning_system(
    time: Res<Time>,
    mut state: ResMut<StormState>,
    mut commands: Commands,
    player: Query<&Player>,
    world: Option<Res<World>>,
    sfx: Res<crate::audio::Sfx>,
    mut feel: ResMut<crate::camera_fx::CameraFeel>,
    mut screen: Option<ResMut<crate::screen_fx::ScreenFx>>,
    mut fx: ParticleSystem,
    mut flashes: Query<(Entity, &mut LightningFlash, &mut PointLight)>,
) {
    let dt = time.delta_secs().clamp(0.0, 0.1);
    for (entity, mut flash, mut light) in &mut flashes {
        flash.life -= dt;
        light.intensity *= (1.0 - dt * 6.0).max(0.0);
        if flash.life <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
    // Thunder playback (staggered after the flash).
    let mut play_thunder = false;
    let mut thunder_volume: f32 = 0.0;
    let mut remaining = Vec::new();
    for (delay, volume) in state.thunder_queue.drain(..) {
        let next = delay - dt;
        if next <= 0.0 {
            play_thunder = true;
            thunder_volume = thunder_volume.max(volume);
        } else {
            remaining.push((next, volume));
        }
    }
    state.thunder_queue = remaining;
    if play_thunder {
        crate::audio::play(
            &mut commands,
            sfx.explosion.clone(),
            thunder_volume,
            Some(0.45),
        );
    }
    if state.kind != StormKind::Lightning {
        return;
    }
    let Ok(player) = player.single() else { return };
    let Some(world) = world.as_deref() else {
        return;
    };
    state.intensity = exp_approach(state.intensity, 1.0, 0.8, dt);
    state.lightning_cd -= dt;
    if state.lightning_cd > 0.0 {
        return;
    }
    let mut rng =
        crate::rng::Rng::new((time.elapsed_secs() * 997.0) as u32 ^ (player.pos.x * 8.0) as u32);
    state.lightning_cd = 1.6 + rng.next() * 4.5;
    let angle = rng.next() * std::f32::consts::TAU;
    let distance = 6.0 + rng.next() * 26.0;
    let strike = Vec3::new(
        player.pos.x + angle.cos() * distance,
        player.pos.y + 26.0,
        player.pos.z + angle.sin() * distance,
    );
    commands.spawn((
        PointLight {
            color: Color::srgb(0.85, 0.9, 1.0),
            intensity: 3_500_000.0,
            range: 120.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_translation(strike),
        LightningFlash { life: 0.18 },
        crate::InGame,
    ));
    // Ground impact sparks.
    let ground = world.g.height_at(strike.x.floor(), strike.z.floor()) as f32 + 1.0;
    fx.explosion(Vec3::new(strike.x, ground + 0.4, strike.z), 0.9);
    state.thunder_queue.push((0.4 + distance * 0.03, 0.5));
    if let Some(screen) = screen.as_deref_mut() {
        screen.lightning = 1.0;
    }
    let near = player.pos.distance(Vec3::new(strike.x, ground, strike.z));
    if near < 3.5 {
        feel.add_trauma(crate::camera_fx::shake::THUNDER);
    }
}

/// Aurora ribbons: large emissive sheets that shimmer with a slow wave.
#[allow(clippy::too_many_arguments)]
pub fn aurora_system(
    time: Res<Time>,
    day: Option<Res<DayTime>>,
    player: Query<&Player>,
    mut state: ResMut<StormState>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut ribbons: Query<(&AuroraRibbon, &mut Transform)>,
) {
    if state.kind != StormKind::Aurora {
        return;
    }
    let Ok(player) = player.single() else { return };
    let night = day
        .as_deref()
        .map(|day| 1.0 - crate::daynight::day_factor(day.0))
        .unwrap_or(0.0);
    state.intensity = exp_approach(
        state.intensity,
        night.clamp(0.0, 1.0),
        0.5,
        time.delta_secs(),
    );
    state.aurora_phase += time.delta_secs() * 0.12;
    let phase = state.aurora_phase;
    let brightness = state.intensity * 0.75;
    for (ribbon, mut transform) in &mut ribbons {
        // Slow vertical breathing and lateral drift.
        let base = ribbon.index as f32;
        let bob = (phase * 1.3 + base).sin() * 8.0;
        let drift = (phase * 0.7 + base * 1.7).cos() * 14.0;
        let yaw = base * 1.35 + phase * 0.05;
        transform.translation = Vec3::new(
            player.pos.x + yaw.cos() * 420.0 + drift,
            player.pos.y + 210.0 + bob + base * 24.0,
            player.pos.z + yaw.sin() * 420.0,
        );
        // Face the player horizontally so the ribbons are always broadside.
        transform.look_at(player.pos, Vec3::Y);
        if let Some(mut material) = materials.get_mut(&ribbon.material) {
            let hue = (phase * 0.06 + base * 0.21).fract();
            let (r, g, b) = aurora_color(hue);
            material.base_color = Color::srgb(r * brightness, g * brightness, b * brightness);
            material.emissive = LinearRgba::new(
                r * brightness,
                g * brightness * 1.2,
                b * brightness * 1.4,
                1.0,
            );
        }
    }
}

/// Green-cyan-violet aurora palette.
pub fn aurora_color(hue: f32) -> (f32, f32, f32) {
    let r = (hue * std::f32::consts::TAU).sin() * 0.5 + 0.5;
    let g = ((hue + 0.25) * std::f32::consts::TAU).sin() * 0.5 + 0.5;
    let b = ((hue + 0.55) * std::f32::consts::TAU).sin() * 0.5 + 0.5;
    (0.15 + r * 0.25, 0.55 + g * 0.45, 0.5 + b * 0.5)
}

fn spawn_aurora(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    state: &mut StormState,
    center: Vec3,
) {
    let mesh = meshes.add(Plane3d::default().mesh().size(460.0, 150.0));
    for index in 0..4 {
        let material = materials.add(StandardMaterial {
            base_color: Color::srgb(0.1, 0.5, 0.45),
            emissive: LinearRgba::new(0.1, 0.4, 0.5, 1.0),
            unlit: true,
            alpha_mode: AlphaMode::Add,
            cull_mode: None,
            double_sided: true,
            fog_enabled: false,
            ..default()
        });
        let entity = commands
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(center + Vec3::Y * 220.0),
                AuroraRibbon { index, material },
                crate::InGame,
            ))
            .id();
        state.aurora_entities.push(entity);
    }
}

/// Sandstorm: wind-blown sand around the player plus compressed fog.
#[allow(clippy::too_many_arguments)]
pub fn sandstorm_system(
    time: Res<Time>,
    mut state: ResMut<StormState>,
    player: Query<&Player>,
    world: Option<Res<World>>,
    mut fog_q: Query<&mut DistanceFog, With<Camera3d>>,
    mut fx: ParticleSystem,
) {
    let dt = time.delta_secs().clamp(0.0, 0.1);
    let active = state.kind == StormKind::Sandstorm;
    state.intensity = exp_approach(state.intensity, if active { 1.0 } else { 0.0 }, 0.6, dt);
    if !active || state.intensity < 0.01 {
        return;
    }
    let Ok(player) = player.single() else { return };
    let Some(world) = world.as_deref() else {
        return;
    };
    // Fog pressure.
    for mut fog in &mut fog_q {
        if let FogFalloff::Linear { start, end } = &mut fog.falloff {
            let pressure = state.intensity;
            *start = *start * (1.0 - pressure) + 18.0 * pressure;
            *end = *end * (1.0 - pressure) + 260.0 * pressure;
        }
        fog.color = Color::srgb(0.72, 0.6, 0.4);
    }
    state.sand_cd -= dt;
    if state.sand_cd > 0.0 {
        return;
    }
    state.sand_cd = 0.05;
    let mut rng = crate::rng::Rng::new((time.elapsed_secs() * 120.0) as u32 ^ world.seed);
    let count = 3 + (state.intensity * 5.0) as u32;
    for _ in 0..count {
        let angle = rng.next() * std::f32::consts::TAU;
        let radius = 4.0 + rng.next() * 20.0;
        let pos = Vec3::new(
            player.pos.x + angle.cos() * radius,
            player.pos.y + rng.next() * 4.0 - 1.0,
            player.pos.z + angle.sin() * radius,
        );
        fx.emit(
            ParticleStyle::Sand,
            pos,
            EmitOptions::default()
                .count(1)
                .dir(Vec3::new(1.0, 0.0, 0.35))
                .speed(6.0, 12.0)
                .spread(0.4)
                .size_scale(0.8 + state.intensity * 0.6)
                .life_scale(1.4),
        );
    }
}

/// Meteor shower: falling rocks that leave resources on impact.
#[allow(clippy::too_many_arguments)]
pub fn meteor_system(
    time: Res<Time>,
    mut state: ResMut<StormState>,
    mut commands: Commands,
    player: Query<&Player>,
    world: Option<Res<World>>,
    icons: Res<crate::ui::IconMaterials>,
    mut meteors: Query<(Entity, &mut Meteor, &mut Transform)>,
    mut feel: ResMut<crate::camera_fx::CameraFeel>,
    mut fx: ParticleSystem,
) {
    let dt = time.delta_secs().clamp(0.0, 0.1);
    let Some(world) = world.as_deref() else {
        return;
    };
    let active = state.kind == StormKind::MeteorShower;
    state.intensity = exp_approach(state.intensity, if active { 1.0 } else { 0.0 }, 0.7, dt);
    let Ok(player) = player.single() else { return };

    if active {
        state.meteor_cd -= dt;
        if state.meteor_cd <= 0.0 {
            let mut rng = crate::rng::Rng::new(
                (time.elapsed_secs() * 313.0) as u32 ^ world.seed.wrapping_mul(7),
            );
            state.meteor_cd = 1.4 + rng.next() * 3.4;
            let angle = rng.next() * std::f32::consts::TAU;
            let radius = 12.0 + rng.next() * 26.0;
            let spawn = Vec3::new(
                player.pos.x + angle.cos() * radius,
                player.pos.y + 80.0,
                player.pos.z + angle.sin() * radius,
            );
            let velocity = Vec3::new(
                -angle.cos() * (6.0 + rng.next() * 8.0),
                -(28.0 + rng.next() * 14.0),
                -angle.sin() * (6.0 + rng.next() * 8.0),
            );
            let mesh = fx.add_mesh(Sphere::new(0.55).mesh().ico(2).expect("meteor sphere"));
            let material = fx.add_material(StandardMaterial {
                base_color: Color::srgb(1.0, 0.6, 0.25),
                emissive: LinearRgba::new(1.0, 0.45, 0.12, 1.0) * 3.0,
                unlit: true,
                ..default()
            });
            commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::from_translation(spawn),
                Meteor {
                    velocity,
                    life: 6.0,
                    trail_t: 0.0,
                },
                crate::InGame,
            ));
        }
    }

    for (entity, mut meteor, mut transform) in &mut meteors {
        meteor.life -= dt;
        transform.translation += meteor.velocity * dt;
        meteor.trail_t -= dt;
        if meteor.trail_t <= 0.0 {
            meteor.trail_t = 0.03;
            fx.emit(
                ParticleStyle::Flame,
                transform.translation,
                EmitOptions::default()
                    .count(2)
                    .dir(-meteor.velocity.normalize_or_zero())
                    .speed(2.0, 5.0)
                    .spread(0.5)
                    .size_scale(1.1),
            );
            fx.emit(
                ParticleStyle::Smoke,
                transform.translation,
                EmitOptions::default()
                    .count(1)
                    .speed(0.5, 1.5)
                    .spread(1.0)
                    .size_scale(0.9)
                    .alpha(0.3),
            );
        }
        let ground = world.g.height_at(
            transform.translation.x.floor(),
            transform.translation.z.floor(),
        ) as f32
            + 1.0;
        if transform.translation.y <= ground || meteor.life <= 0.0 {
            let impact = Vec3::new(
                transform.translation.x,
                ground + 0.5,
                transform.translation.z,
            );
            fx.explosion(impact, 1.1);
            let distance = impact.distance(player.pos);
            if distance < 26.0 {
                feel.add_trauma(crate::camera_fx::shake::METEOR * (1.0 - distance / 26.0));
            }
            // Meteorites leave a small metal yield.
            let mut rng = crate::rng::Rng::new(
                (impact.x * 31.0) as u32 ^ (impact.z * 57.0) as u32 ^ world.seed,
            );
            let loot: [(&str, i32); 3] = [
                ("iron_ore", 1 + (rng.next() * 2.0) as i32),
                ("nickel", 1),
                ("basalt_shard", 2),
            ];
            let (item, amount) = loot[rng.range(loot.len())];
            crate::creatures::spawn_drop(
                &mut commands,
                world,
                &icons,
                impact + Vec3::Y * 0.4,
                Vec3::new(0.0, 2.4, 0.0),
                item.to_string(),
                amount,
                0.6,
            );
            commands.entity(entity).despawn();
        }
    }
}

/// Fog pressure is applied after daynight each frame; when no sandstorm is
/// active the fog values are simply left untouched by this module.
pub struct StormPlugin;

impl Plugin for StormPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StormState>().add_systems(
            Update,
            (
                storm_director_system,
                lightning_system,
                aurora_system,
                sandstorm_system,
                meteor_system,
            )
                .chain()
                .in_set(crate::schedule::GameSet::CommonWeather)
                .run_if(in_state(GameState::Playing))
                .run_if(ground_scene_mode),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_selection_matches_biome_character() {
        let mut rng = crate::rng::Rng::new(1);
        for _ in 0..200 {
            let (kind, duration, cooldown) = choose_event("ferrous", 0.8, &mut rng);
            assert!(matches!(
                kind,
                StormKind::Lightning | StormKind::MeteorShower
            ));
            if kind != StormKind::None {
                assert!(duration > 0.0 && cooldown > 0.0);
            }
        }
        let mut rng = crate::rng::Rng::new(2);
        let mut auroras = 0;
        for _ in 0..100 {
            let (kind, _, _) = choose_event("frozen", 0.9, &mut rng);
            if kind == StormKind::Aurora {
                auroras += 1;
            }
        }
        assert!(auroras > 50, "auroras should be common at night");
        let mut rng = crate::rng::Rng::new(3);
        let (kind, _, _) = choose_event("frozen", 0.1, &mut rng);
        assert_eq!(kind, StormKind::None, "no aurora during the day");
    }

    #[test]
    fn aurora_palette_is_visible() {
        for step in 0..20 {
            let (r, g, b) = aurora_color(step as f32 / 20.0);
            assert!((0.0..=1.2).contains(&r));
            assert!(g > 0.4 && b > 0.3);
        }
    }

    #[test]
    fn default_state_is_calm() {
        let state = StormState::default();
        assert_eq!(state.kind, StormKind::None);
        assert_eq!(state.intensity, 0.0);
    }
}
