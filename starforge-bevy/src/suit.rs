//! Exoskeleton active abilities: dash, shield burst, emergency oxygen,
//! medkit use, headlamp and night vision.
//!
//! The abilities are gated by the exoskeleton branch of the tech tree and
//! share a compact cooldown HUD drawn above the hotbar. All movement effects
//! feed the existing camera-feel resource so the dash also reads on screen.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use crate::audio;
use crate::particles::{EmitOptions, ParticleStyle, ParticleSystem};
use crate::player::Player;
use crate::schedule::{GameState, in_planet_mode};
use crate::tween::exp_approach;
use crate::ui::Research;

pub const DASH_COST: f32 = 12.0;
pub const DASH_COOLDOWN: f32 = 2.6;
pub const DASH_SPEED: f32 = 15.5;
pub const BURST_COOLDOWN: f32 = 7.0;
pub const BURST_SHIELD_COST: f32 = 2.0;
pub const BURST_RADIUS: f32 = 6.5;
pub const RECYCLE_COOLDOWN: f32 = 9.0;
pub const RECYCLE_SHIELD_COST: f32 = 18.0;
pub const RECYCLE_O2_GAIN: f32 = 24.0;
pub const MEDKIT_COOLDOWN: f32 = 2.0;
pub const MEDKIT_HEAL: f32 = 4.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ability {
    Dash,
    ShieldBurst,
    Recycle,
    Medkit,
}

impl Ability {
    pub fn key_label(self) -> &'static str {
        match self {
            Ability::Dash => "Q",
            Ability::ShieldBurst => "X",
            Ability::Recycle => "Z",
            Ability::Medkit => "H",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Ability::Dash => "冲刺",
            Ability::ShieldBurst => "护盾冲击",
            Ability::Recycle => "应急供氧",
            Ability::Medkit => "医疗包",
        }
    }

    pub fn tech(self) -> Option<&'static str> {
        match self {
            Ability::Dash => Some("exo_mobility"),
            Ability::ShieldBurst => Some("exo_shielding"),
            Ability::Recycle => Some("exo_recycler"),
            Ability::Medkit => None,
        }
    }

    pub const ALL: [Ability; 4] = [
        Ability::Dash,
        Ability::ShieldBurst,
        Ability::Recycle,
        Ability::Medkit,
    ];
}

#[derive(Resource)]
pub struct SuitState {
    pub dash_cd: f32,
    pub burst_cd: f32,
    pub recycle_cd: f32,
    pub medkit_cd: f32,
    /// Decaying dash trail timer (drives afterimages, FOV and particles).
    pub dash_t: f32,
    pub night_vision: bool,
    pub headlamp: bool,
    pub nv_blend: f32,
    pub lamp_on: f32,
    /// Set when a locked ability key is pressed so the HUD can flash a hint.
    pub locked_hint: Option<(Ability, f32)>,
}

impl Default for SuitState {
    fn default() -> Self {
        Self {
            dash_cd: 0.0,
            burst_cd: 0.0,
            recycle_cd: 0.0,
            medkit_cd: 0.0,
            dash_t: 0.0,
            night_vision: false,
            headlamp: false,
            nv_blend: 0.0,
            lamp_on: 0.0,
            locked_hint: None,
        }
    }
}

impl SuitState {
    pub fn unlocked(techs: &[String], ability: Ability) -> bool {
        match ability.tech() {
            Some(tech) => data_techs_unlocked(techs, tech),
            None => true,
        }
    }

    /// (remaining, total) cooldown for the HUD.
    pub fn cooldown(&self, ability: Ability) -> (f32, f32) {
        match ability {
            Ability::Dash => (self.dash_cd, DASH_COOLDOWN),
            Ability::ShieldBurst => (self.burst_cd, BURST_COOLDOWN),
            Ability::Recycle => (self.recycle_cd, RECYCLE_COOLDOWN),
            Ability::Medkit => (self.medkit_cd, MEDKIT_COOLDOWN),
        }
    }

    pub fn ready(&self, ability: Ability) -> bool {
        self.cooldown(ability).0 <= 0.0
    }
}

fn data_techs_unlocked(techs: &[String], id: &str) -> bool {
    crate::data::tech_unlocked(techs, id)
}

/// Spawns the two suit lights (headlamp and night-vision lamp) once.
pub fn setup_suit_lights(mut commands: Commands) {
    commands.spawn((
        PointLight {
            color: Color::srgb(1.0, 0.93, 0.78),
            intensity: 0.0,
            range: 26.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::default(),
        SuitLight { night: false },
        crate::InGame,
    ));
    commands.spawn((
        PointLight {
            color: Color::srgb(0.45, 1.0, 0.55),
            intensity: 0.0,
            range: 20.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::default(),
        SuitLight { night: true },
        crate::InGame,
    ));
}

#[derive(Component)]
pub struct SuitLight {
    pub night: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn suit_input_system(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    research: Res<Research>,
    mut suit: ResMut<SuitState>,
    mut player: Query<&mut Player>,
    mut creatures: Query<(&mut crate::creatures::Creature, &Transform)>,
    mut feel: ResMut<crate::camera_fx::CameraFeel>,
    mut screen: ResMut<crate::screen_fx::ScreenFx>,
    sfx: Res<audio::Sfx>,
    mut fx: ParticleSystem,
) {
    let dt = time.delta_secs().clamp(0.0, 0.1);
    suit.dash_cd = (suit.dash_cd - dt).max(0.0);
    suit.burst_cd = (suit.burst_cd - dt).max(0.0);
    suit.recycle_cd = (suit.recycle_cd - dt).max(0.0);
    suit.medkit_cd = (suit.medkit_cd - dt).max(0.0);
    suit.dash_t = (suit.dash_t - dt).max(0.0);
    if let Some((_, timer)) = suit.locked_hint.as_mut() {
        *timer -= dt;
        if *timer <= 0.0 {
            suit.locked_hint = None;
        }
    }
    let Ok(mut player) = player.single_mut() else {
        return;
    };
    if player.dead {
        return;
    }
    let techs = &research.techs;

    // ---------- Dash ----------
    if keys.just_pressed(KeyCode::KeyQ) {
        if !SuitState::unlocked(techs, Ability::Dash) {
            suit.locked_hint = Some((Ability::Dash, 2.4));
            audio::play(&mut commands, sfx.error.clone(), 0.4, None);
        } else if !suit.ready(Ability::Dash) {
            audio::play(&mut commands, sfx.error.clone(), 0.3, Some(1.3));
        } else if player.stats.jet < DASH_COST && !player.creative() {
            player.toast("冲刺能量不足（喷气背包能量）");
            audio::play(&mut commands, sfx.error.clone(), 0.35, None);
        } else {
            if !player.creative() {
                player.stats.jet = (player.stats.jet - DASH_COST).max(0.0);
            }
            suit.dash_cd = DASH_COOLDOWN;
            suit.dash_t = 0.32;
            let look = player.look_dir();
            let mut direction = Vec3::new(look.x, 0.0, look.z).normalize_or_zero();
            if direction.length_squared() < 1e-4 {
                direction = player.forward();
            }
            let vertical = if player.on_ground { 0.0 } else { 0.16 };
            player.vel.x = direction.x * DASH_SPEED;
            player.vel.z = direction.z * DASH_SPEED;
            player.vel.y = (player.vel.y * 0.4).max(0.0) + vertical;
            feel.ability_fov = 10.0;
            feel.add_trauma(crate::camera_fx::shake::WEAPON_FIRE * 1.4);
            audio::play(&mut commands, sfx.jump.clone(), 0.6, Some(0.72));
            fx.emit(
                ParticleStyle::Thruster,
                player.pos + Vec3::Y * 0.5,
                EmitOptions::default()
                    .count(14)
                    .dir(-direction)
                    .speed(4.0, 10.0)
                    .spread(0.6)
                    .size_scale(1.2),
            );
            fx.emit(
                ParticleStyle::Dust,
                player.pos + Vec3::Y * 0.08,
                EmitOptions::default()
                    .count(10)
                    .dir(-direction)
                    .speed(2.0, 5.0)
                    .spread(1.2)
                    .size_scale(1.1),
            );
        }
    }

    // ---------- Shield burst ----------
    if keys.just_pressed(KeyCode::KeyX) {
        if !SuitState::unlocked(techs, Ability::ShieldBurst) {
            suit.locked_hint = Some((Ability::ShieldBurst, 2.4));
            audio::play(&mut commands, sfx.error.clone(), 0.4, None);
        } else if !suit.ready(Ability::ShieldBurst) {
            audio::play(&mut commands, sfx.error.clone(), 0.3, Some(1.3));
        } else if player.stats.shield < BURST_SHIELD_COST {
            player.toast("护盾能量不足");
            audio::play(&mut commands, sfx.error.clone(), 0.35, None);
        } else {
            player.stats.shield -= BURST_SHIELD_COST;
            suit.burst_cd = BURST_COOLDOWN;
            let origin = player.pos + Vec3::Y * 0.9;
            let mut hits = 0;
            for (mut creature, transform) in &mut creatures {
                if creature.hp <= 0.0 {
                    continue;
                }
                let offset = transform.translation - origin;
                let distance = offset.length();
                if distance > BURST_RADIUS {
                    continue;
                }
                let falloff = 1.0 - distance / BURST_RADIUS;
                creature.hp -= 3.0 + falloff * 3.0;
                creature.hit_t = 0.25;
                creature.aggro_t = 8.0;
                let push = offset.normalize_or_zero() * (8.0 * falloff);
                creature.vel.x += push.x;
                creature.vel.z += push.z;
                creature.vel.y += 2.0 * falloff;
                hits += 1;
            }
            feel.add_trauma(crate::camera_fx::shake::EXPLOSION * 0.8);
            screen.shield_flash = 1.0;
            audio::play(&mut commands, sfx.pulse.clone(), 0.8, Some(0.6));
            // Expanding shockwave ring.
            for ring in 0..3 {
                fx.emit(
                    ParticleStyle::Shield,
                    origin,
                    EmitOptions::default()
                        .count(3 + ring * 2)
                        .speed(6.0 + ring as f32 * 2.0, 9.0 + ring as f32 * 2.0)
                        .spread(2.0)
                        .size_scale(1.4 + ring as f32 * 0.3),
                );
            }
            if hits > 0 {
                player.toast(format!("护盾冲击命中 {hits} 个目标"));
            }
        }
    }

    // ---------- Emergency oxygen ----------
    if keys.just_pressed(KeyCode::KeyZ) {
        if !SuitState::unlocked(techs, Ability::Recycle) {
            suit.locked_hint = Some((Ability::Recycle, 2.4));
            audio::play(&mut commands, sfx.error.clone(), 0.4, None);
        } else if !suit.ready(Ability::Recycle) {
            audio::play(&mut commands, sfx.error.clone(), 0.3, Some(1.3));
        } else if player.stats.shield < RECYCLE_SHIELD_COST {
            player.toast("护盾能量不足以转化氧气");
            audio::play(&mut commands, sfx.error.clone(), 0.35, None);
        } else {
            player.stats.shield -= RECYCLE_SHIELD_COST;
            player.stats.o2 = (player.stats.o2 + RECYCLE_O2_GAIN).min(player.stat_max("o2"));
            suit.recycle_cd = RECYCLE_COOLDOWN;
            audio::play(&mut commands, sfx.research.clone(), 0.5, Some(1.1));
            fx.emit(
                ParticleStyle::Heal,
                player.eye() + Vec3::Y * 0.1,
                EmitOptions::default()
                    .count(8)
                    .speed(0.8, 2.4)
                    .spread(2.0)
                    .size_scale(0.8),
            );
            player.toast(format!("应急供氧 +{RECYCLE_O2_GAIN:.0} O₂"));
        }
    }

    // ---------- Medkit ----------
    if keys.just_pressed(KeyCode::KeyH) {
        if !suit.ready(Ability::Medkit) {
            audio::play(&mut commands, sfx.error.clone(), 0.3, Some(1.3));
        } else if player.stats.hp >= 8.0 {
            player.toast("生命值已满");
        } else if player.inv.count_item("medkit") < 1 {
            player.toast("需要医疗包（背包中未找到）");
            audio::play(&mut commands, sfx.error.clone(), 0.35, None);
        } else {
            player.inv.remove_item("medkit", 1);
            let before = player.stats.hp;
            player.stats.hp = (player.stats.hp + MEDKIT_HEAL).min(8.0);
            suit.medkit_cd = MEDKIT_COOLDOWN;
            audio::play(&mut commands, sfx.craft.clone(), 0.55, Some(1.25));
            fx.heal(player.eye(), (player.stats.hp - before) / MEDKIT_HEAL);
            player.toast("医疗包：外骨骼修复完成");
        }
    }

    // ---------- Night vision / headlamp ----------
    let optics_ready = data_techs_unlocked(techs, "exo_optics");
    if keys.just_pressed(KeyCode::KeyB) {
        if !optics_ready {
            player.toast("战术目镜未解锁（研究科技树）");
            audio::play(&mut commands, sfx.error.clone(), 0.4, None);
        } else {
            suit.night_vision = !suit.night_vision;
            if suit.night_vision {
                suit.headlamp = false;
            }
            audio::play(&mut commands, sfx.click.clone(), 0.5, Some(0.8));
            player.toast(if suit.night_vision {
                "战术夜视：开启"
            } else {
                "战术夜视：关闭"
            });
        }
    }
    if keys.just_pressed(KeyCode::KeyF) {
        if !optics_ready {
            player.toast("头灯未解锁（研究科技树）");
            audio::play(&mut commands, sfx.error.clone(), 0.4, None);
        } else {
            suit.headlamp = !suit.headlamp;
            if suit.headlamp {
                suit.night_vision = false;
            }
            audio::play(&mut commands, sfx.click.clone(), 0.5, Some(1.2));
            player.toast(if suit.headlamp {
                "头灯：开启"
            } else {
                "头灯：关闭"
            });
        }
    }
}

pub fn suit_light_system(
    time: Res<Time>,
    mode: Res<crate::space::FlightMode>,
    mut suit: ResMut<SuitState>,
    mut screen: ResMut<crate::screen_fx::ScreenFx>,
    player: Query<&Player>,
    mut lights: Query<(&SuitLight, &mut Transform, &mut PointLight)>,
) {
    let dt = time.delta_secs().clamp(0.0, 0.1);
    // Toggles are only driven on the ground; this system keeps the blends
    // decaying everywhere so leaving a planet does not leave night vision on.
    suit.nv_blend = exp_approach(
        suit.nv_blend,
        if suit.night_vision { 1.0 } else { 0.0 },
        3.5,
        dt,
    );
    suit.lamp_on = exp_approach(suit.lamp_on, if suit.headlamp { 1.0 } else { 0.0 }, 5.0, dt);
    screen.night_vision = suit.nv_blend;
    let visible = in_planet_mode_local(*mode);
    let Ok(player) = player.single() else { return };
    let eye = player.eye();
    let forward = player.look_dir();
    for (light, mut transform, mut point) in &mut lights {
        if light.night {
            transform.translation = eye + forward * 0.4;
            point.intensity = if visible { 900.0 * suit.nv_blend } else { 0.0 };
        } else {
            transform.translation = eye + forward * 0.8;
            point.intensity = if visible { 2_600.0 * suit.lamp_on } else { 0.0 };
        }
    }
}

fn in_planet_mode_local(mode: crate::space::FlightMode) -> bool {
    use crate::space::FlightMode::*;
    matches!(mode, Planet | Seated)
}

/// Compact ability chips above the hotbar with cooldown shading.
pub fn suit_hud_system(
    mut contexts: EguiContexts,
    ui_state: Res<crate::ui::UiState>,
    suit: Res<SuitState>,
    research: Res<Research>,
    player: Query<&Player>,
) {
    if ui_state.locked() {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else { return };
    if !crate::ui::egui_fonts_ready(ctx) {
        return;
    }
    let Ok(player) = player.single() else { return };
    if player.dead {
        return;
    }
    let techs = &research.techs;
    let abilities: Vec<Ability> = Ability::ALL
        .into_iter()
        .filter(|ability| SuitState::unlocked(techs, *ability))
        .collect();
    if abilities.is_empty() {
        return;
    }
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::new("suit_abilities"),
    ));
    let viewport = ctx.viewport_rect();
    const SIZE: f32 = 34.0;
    const GAP: f32 = 6.0;
    let total = abilities.len() as f32 * SIZE + (abilities.len() as f32 - 1.0) * GAP;
    let start = egui::pos2(viewport.center().x - total * 0.5, viewport.bottom() - 108.0);
    for (index, ability) in abilities.iter().enumerate() {
        let rect = egui::Rect::from_min_size(
            egui::pos2(start.x + index as f32 * (SIZE + GAP), start.y),
            egui::vec2(SIZE, SIZE),
        );
        let (remaining, total_time) = suit.cooldown(*ability);
        let ready = remaining <= 0.0;
        let accent = match ability {
            Ability::Dash => egui::Color32::from_rgb(0x35, 0xe0, 0xe8),
            Ability::ShieldBurst => egui::Color32::from_rgb(0x7d, 0xc8, 0xff),
            Ability::Recycle => egui::Color32::from_rgb(0x7d, 0xff, 0x8a),
            Ability::Medkit => egui::Color32::from_rgb(0xff, 0x6a, 0x5e),
        };
        painter.rect_filled(
            rect,
            egui::CornerRadius::same(6),
            egui::Color32::from_rgba_unmultiplied(12, 18, 26, 190),
        );
        painter.rect_stroke(
            rect,
            egui::CornerRadius::same(6),
            egui::Stroke::new(
                1.2,
                if ready {
                    accent
                } else {
                    egui::Color32::from_rgb(0x35, 0x46, 0x55)
                },
            ),
            egui::StrokeKind::Middle,
        );
        // Cooldown pie: shade the remaining fraction from the top.
        if !ready && total_time > 0.0 {
            let fraction = (remaining / total_time).clamp(0.0, 1.0);
            let shade = egui::Rect::from_min_max(
                egui::pos2(rect.min.x, rect.min.y),
                egui::pos2(rect.max.x, rect.min.y + rect.height() * fraction),
            );
            painter.rect_filled(
                shade,
                egui::CornerRadius::same(6),
                egui::Color32::from_rgba_unmultiplied(0, 0, 0, 130),
            );
        }
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            ability.key_label(),
            egui::FontId::proportional(15.0),
            if ready {
                accent
            } else {
                egui::Color32::from_gray(140)
            },
        );
        // Medkit count badge.
        if *ability == Ability::Medkit {
            let count = player.inv.count_item("medkit");
            painter.text(
                egui::pos2(rect.max.x - 4.0, rect.max.y - 3.0),
                egui::Align2::RIGHT_BOTTOM,
                format!("{count}"),
                egui::FontId::proportional(10.0),
                egui::Color32::from_gray(200),
            );
        }
    }
    // Locked hint flash.
    if let Some((ability, timer)) = &suit.locked_hint
        && *timer > 0.0
    {
        let alpha = (*timer / 2.4).clamp(0.0, 1.0);
        painter.text(
            egui::pos2(viewport.center().x, start.y - 12.0),
            egui::Align2::CENTER_BOTTOM,
            format!("{} 未解锁（研究科技树）", ability.name()),
            egui::FontId::proportional(12.0),
            egui::Color32::from_rgba_unmultiplied(255, 180, 80, (alpha * 230.0) as u8),
        );
    }
}

pub struct SuitPlugin;

impl Plugin for SuitPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SuitState>()
            .add_systems(Startup, setup_suit_lights)
            .add_systems(
                Update,
                suit_input_system
                    .in_set(crate::schedule::GameSet::GroundPlayer)
                    .after(crate::player::movement_system)
                    .run_if(in_state(GameState::Playing))
                    .run_if(in_planet_mode),
            )
            .add_systems(
                Update,
                suit_light_system.run_if(in_state(GameState::Playing)),
            )
            .add_systems(
                Update,
                suit_hud_system
                    .run_if(in_state(GameState::Playing))
                    .run_if(in_planet_mode),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cooldowns_start_ready_and_unlock_rules_hold() {
        let state = SuitState::default();
        for ability in Ability::ALL {
            assert!(state.ready(ability));
        }
        let techs = vec!["exosuit".to_string(), "exo_mobility".to_string()];
        assert!(SuitState::unlocked(&techs, Ability::Dash));
        assert!(!SuitState::unlocked(&techs, Ability::ShieldBurst));
        assert!(SuitState::unlocked(&techs, Ability::Medkit));
        assert!(!SuitState::unlocked(&techs, Ability::Recycle));
    }

    #[test]
    fn cooldown_reports_remaining_fraction() {
        let state = SuitState {
            dash_cd: DASH_COOLDOWN * 0.5,
            ..default()
        };
        let (remaining, total) = state.cooldown(Ability::Dash);
        assert_eq!(total, DASH_COOLDOWN);
        assert!((remaining - DASH_COOLDOWN * 0.5).abs() < 1e-4);
        assert!(!state.ready(Ability::Dash));
    }

    #[test]
    fn ability_keys_are_unique() {
        let labels: Vec<&str> = Ability::ALL.iter().map(|a| a.key_label()).collect();
        let mut unique = labels.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(labels.len(), unique.len());
    }
}
