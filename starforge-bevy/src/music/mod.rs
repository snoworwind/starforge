//! Procedural music & ambience: a live synthesizer streamed through Bevy's
//! audio mixer.
//!
//! [`MusicTrack`] is an infinite custom audio asset: Bevy asks it for a
//! [`MusicDecoder`], whose engine renders stereo samples on the audio thread
//! while reading a lock-free control surface ([`engine::MusicShared`]). The
//! Bevy-side [`music_director_system`] translates game state (scene, biome,
//! hostiles, health, time of day) into that control surface every frame.

pub mod ambience;
pub mod dsp;
pub mod engine;
pub mod theory;

use bevy::audio::{AddAudioSource, AudioPlayer, Decodable, PlaybackMode, PlaybackSettings};
use bevy::prelude::*;
use std::sync::Arc;

use crate::player::Player;
use crate::save::Settings;
use crate::space::{FlightMode, ShipState};
use engine::{MusicDecoder, MusicScene, MusicShared, Stinger};
use theory::Key;

/// A gameplay event that should be accented by the soundtrack.
#[derive(Message, Clone, Copy, Debug)]
pub struct MusicStinger(pub Stinger);

/// The infinite asset handed to Bevy's audio player.
#[derive(Asset, TypePath)]
pub struct MusicTrack {
    pub shared: Arc<MusicShared>,
}

impl Decodable for MusicTrack {
    type Decoder = MusicDecoder;

    fn decoder(&self) -> MusicDecoder {
        MusicDecoder::new(self.shared.clone(), 44_100.0)
    }
}

/// Infinite environmental ambience asset.
#[derive(Asset, TypePath)]
pub struct AmbienceTrack {
    pub shared: Arc<ambience::AmbienceShared>,
}

impl Decodable for AmbienceTrack {
    type Decoder = ambience::AmbienceDecoder;

    fn decoder(&self) -> ambience::AmbienceDecoder {
        ambience::AmbienceDecoder::new(self.shared.clone(), 44_100.0)
    }
}

#[derive(Resource)]
pub struct AmbienceManager {
    pub shared: Arc<ambience::AmbienceShared>,
    pub kind: ambience::AmbienceKind,
    pub underground: bool,
}

/// Persistent music state that survives menu/world transitions.
#[derive(Resource)]
pub struct MusicManager {
    pub shared: Arc<MusicShared>,
    pub handle: Handle<MusicTrack>,
    pub entity: Option<Entity>,
    pub scene: MusicScene,
    pub intensity: f32,
    pub danger: f32,
    pub combat_cooldown: f32,
    pub enabled: bool,
    pub last_scan: f32,
    pub last_level: f32,
    pub last_mode: FlightMode,
}

impl MusicManager {
    pub fn stinger(&self, stinger: Stinger) {
        self.shared.trigger_stinger(stinger);
    }
}

fn setup_music(
    mut commands: Commands,
    mut tracks: ResMut<Assets<MusicTrack>>,
    mut ambience_tracks: ResMut<Assets<AmbienceTrack>>,
    settings: Res<Settings>,
) {
    let shared = MusicShared::new();
    shared.set_volume(settings.music_volume);
    shared.set_master(settings.volume);
    shared.set_paused(!settings.music);
    let handle = tracks.add(MusicTrack {
        shared: shared.clone(),
    });
    // The decoder never ends, so `Once` is effectively "play forever" without
    // requiring a clonable source. Volume is applied inside the synth.
    let entity = commands
        .spawn((
            AudioPlayer(handle.clone()),
            PlaybackSettings {
                mode: PlaybackMode::Once,
                volume: bevy::audio::Volume::Linear(1.0),
                ..default()
            },
            crate::InGame,
        ))
        .id();
    commands.insert_resource(MusicManager {
        shared,
        handle,
        entity: Some(entity),
        scene: MusicScene::Menu,
        intensity: 0.0,
        danger: 0.0,
        combat_cooldown: 0.0,
        enabled: settings.music,
        last_scan: 0.0,
        last_level: 0.0,
        last_mode: FlightMode::Planet,
    });

    let ambience_shared = ambience::AmbienceShared::new();
    ambience_shared.set_volume(settings.ambience_volume);
    ambience_shared.set_master(settings.volume);
    ambience_shared.set_paused(!settings.music);
    let ambience_handle = ambience_tracks.add(AmbienceTrack {
        shared: ambience_shared.clone(),
    });
    commands.spawn((
        AudioPlayer(ambience_handle),
        PlaybackSettings {
            mode: PlaybackMode::Once,
            volume: bevy::audio::Volume::Linear(1.0),
            ..default()
        },
        crate::InGame,
    ));
    commands.insert_resource(AmbienceManager {
        shared: ambience_shared,
        kind: ambience::AmbienceKind::None,
        underground: false,
    });
}

/// Where in a planet's vertical profile the player is: above ground (0),
/// underground (1).
pub fn cave_factor(player_y: f32, surface_y: f32) -> f32 {
    crate::tween::smoothstep(surface_y - 3.0, surface_y - 10.0, player_y)
}

/// Combat intensity from nearby hostile wildlife.
pub fn wildlife_threat(player: Vec3, hostiles: impl Iterator<Item = (Vec3, f32, f32)>) -> f32 {
    let mut threat: f32 = 0.0;
    for (position, aggro, hp) in hostiles {
        if hp <= 0.0 {
            continue;
        }
        let distance = position.distance(player);
        if distance > 30.0 {
            continue;
        }
        let proximity = 1.0 - distance / 30.0;
        threat = threat.max(proximity * (0.45 + aggro * 0.55));
    }
    threat
}

/// Health/hazard panic level: drives the heartbeat pulse and dissonance.
pub fn danger_level(hp: f32, max_hp: f32, o2: f32, haz: f32, dead: bool) -> f32 {
    if dead {
        return 0.0;
    }
    let hp_panic = (1.0 - hp / max_hp.max(1.0)).max(0.0);
    let o2_panic = if o2 < 30.0 { 1.0 - o2 / 30.0 } else { 0.0 };
    let haz_panic = if haz < 25.0 { 1.0 - haz / 25.0 } else { 0.0 };
    (hp_panic * 1.1)
        .max(o2_panic)
        .max(haz_panic)
        .clamp(0.0, 1.0)
}

#[allow(clippy::too_many_arguments)]
pub fn music_director_system(
    time: Res<Time>,
    state: Res<State<crate::GameState>>,
    mode: Res<FlightMode>,
    settings: Res<Settings>,
    world: Option<Res<crate::world::World>>,
    day: Option<Res<crate::daynight::DayTime>>,
    player: Query<&Player>,
    ship: Option<Res<ShipState>>,
    creatures: Query<(&crate::creatures::Creature, &Transform)>,
    visitors: Query<(&crate::space::VisitorShip, &Transform)>,
    screen: Option<Res<crate::screen_fx::ScreenFx>>,
    mut stingers: MessageReader<MusicStinger>,
    mut manager: ResMut<MusicManager>,
) {
    let dt = time.delta_secs().clamp(0.0, 0.1);
    manager.shared.set_volume(settings.music_volume);
    manager.shared.set_master(settings.volume);
    manager.enabled = settings.music;
    manager.shared.set_paused(!settings.music);

    // ---------- Scene ----------
    let mut scene = match *state.get() {
        crate::GameState::Menu | crate::GameState::Loading => MusicScene::Menu,
        crate::GameState::Playing => match *mode {
            FlightMode::Warping => MusicScene::Warp,
            FlightMode::Station => MusicScene::Station,
            FlightMode::Space => MusicScene::Space,
            _ => MusicScene::Planet,
        },
    };
    // Underground exploration gets its own palette (silent in space).
    let Ok(player) = player.single() else {
        return;
    };
    let mut underground = false;
    if let Some(world) = world.as_deref()
        && !mode.space_scene()
    {
        let surface = world
            .g
            .height_at(player.pos.x.floor(), player.pos.z.floor()) as f32;
        underground = cave_factor(player.pos.y, surface) > 0.5 && !player.in_liquid;
    }
    if underground && scene == MusicScene::Planet {
        scene = MusicScene::Cave;
    }
    manager.shared.set_cave(underground);
    if scene != manager.scene {
        manager.scene = scene;
        manager.shared.set_scene(scene);
    }

    // ---------- Biome & seed ----------
    if let Some(world) = world.as_deref() {
        let key = world.biome().key;
        if let Some(index) = crate::data::BIOMES
            .iter()
            .position(|biome| biome.key == key)
        {
            manager.shared.set_biome(index);
        }
        manager.shared.set_seed(world.seed);
    }
    if let Some(day) = day {
        manager.shared.set_day(crate::daynight::day_factor(day.0));
    }

    // ---------- Intensity ----------
    let mut target_intensity: f32 = 0.0;
    if matches!(*mode, FlightMode::Planet | FlightMode::Seated) {
        let hostiles = creatures.iter().map(|(creature, transform)| {
            (
                transform.translation,
                if creature.aggro_t > 0.0 { 1.0 } else { 0.0 },
                creature.hp,
            )
        });
        target_intensity = wildlife_threat(player.pos, hostiles);
    } else if *mode == FlightMode::Space {
        let hostile_visitors = visitors
            .iter()
            .filter(|(visitor, _)| visitor.hostile)
            .map(|(_, transform)| {
                let distance = transform
                    .translation
                    .distance(ship.as_ref().map(|ship| ship.pos).unwrap_or(player.pos));
                (1.0 - (distance / 320.0)).clamp(0.0, 1.0)
            })
            .fold(0.0f32, f32::max);
        target_intensity = hostile_visitors * 0.9;
    }
    // Mining laser and active jetpack add a touch of urgency.
    if player.hot_idx == -1
        && player.mining.is_some()
        && matches!(*mode, FlightMode::Planet | FlightMode::Seated)
    {
        target_intensity = target_intensity.max(0.18);
    }
    if *mode == FlightMode::Warping {
        target_intensity = 1.0;
    }
    // Fast attack, slow release; combat lingers for a couple of seconds.
    let rise = if target_intensity > manager.intensity {
        2.2
    } else {
        0.35
    };
    manager.intensity = crate::tween::exp_approach(manager.intensity, target_intensity, rise, dt);
    manager.shared.set_intensity(manager.intensity);

    // ---------- Danger ----------
    let danger = danger_level(
        player.stats.hp,
        player.stat_max("hp"),
        player.stats.o2,
        player.stats.haz,
        player.dead,
    );
    manager.danger = crate::tween::exp_approach(manager.danger, danger, 1.6, dt);
    manager.shared.set_danger(manager.danger);

    // ---------- Warp ----------
    manager.shared.set_warping(*mode == FlightMode::Warping);

    // ---------- Stingers ----------
    for message in stingers.read() {
        manager.shared.trigger_stinger(message.0);
    }
    // Automatic accents: entering combat for the first time after a lull.
    if manager.intensity > 0.6 && manager.combat_cooldown <= 0.0 {
        manager.combat_cooldown = 12.0;
        manager.shared.trigger_stinger(Stinger::Alert);
    } else {
        manager.combat_cooldown = (manager.combat_cooldown - dt).max(0.0);
    }
    // Edge-detect visual events instead of threading stingers through every
    // gameplay system: scans, research completions and warp arrivals.
    if let Some(screen) = screen.as_deref() {
        if screen.scan > 0.85 && manager.last_scan <= 0.85 {
            manager.shared.trigger_stinger(Stinger::Scan);
        }
        if screen.level_flash > 0.85 && manager.last_level <= 0.85 {
            manager.shared.trigger_stinger(Stinger::Discovery);
        }
        manager.last_scan = screen.scan;
        manager.last_level = screen.level_flash;
    }
    if manager.last_mode == FlightMode::Warping && *mode != FlightMode::Warping {
        manager.shared.trigger_stinger(Stinger::WarpArrival);
    }
    manager.last_mode = *mode;
}

/// Snapshots helper used by the UI (planet music key preview).
pub fn planet_key_preview(seed: u32, biome_key: &str) -> Key {
    theory::key_for_context(biome_key, seed)
}

/// Chooses the environmental bed from the current world state.
#[allow(clippy::too_many_arguments)]
pub fn ambience_director_system(
    state: Res<State<crate::GameState>>,
    mode: Res<FlightMode>,
    settings: Res<Settings>,
    world: Option<Res<crate::world::World>>,
    day: Option<Res<crate::daynight::DayTime>>,
    player: Query<&Player>,
    mut manager: ResMut<AmbienceManager>,
) {
    manager.shared.set_volume(settings.ambience_volume);
    manager.shared.set_master(settings.volume);
    manager.shared.set_paused(!settings.music);
    if let Some(day) = day {
        manager
            .shared
            .set_night(1.0 - crate::daynight::day_factor(day.0));
    }
    let mut kind = ambience::AmbienceKind::None;
    let mut intensity = 0.45f32;
    if *state.get() == crate::GameState::Playing
        && !mode.space_scene()
        && let (Some(world), Ok(player)) = (world.as_deref(), player.single())
    {
        let surface = world
            .g
            .height_at(player.pos.x.floor(), player.pos.z.floor()) as f32;
        let underground = cave_factor(player.pos.y, surface) > 0.5;
        manager.underground = underground;
        kind = if underground {
            ambience::AmbienceKind::Cave
        } else {
            ambience::AmbienceKind::for_biome(world.biome().key)
        };
        if player.stats.haz < 50.0 {
            intensity += 0.18;
        }
        if player.in_liquid {
            intensity += 0.12;
        }
        if player.pos.y > crate::data::SEA_Y + 400.0 {
            intensity += 0.1;
        }
    } else {
        manager.underground = false;
    }
    manager.shared.set_intensity(intensity.min(1.0));
    if kind != manager.kind {
        manager.kind = kind;
        manager.shared.set_kind(kind);
    }
}

pub struct MusicPlugin;

impl Plugin for MusicPlugin {
    fn build(&self, app: &mut App) {
        app.add_audio_source::<MusicTrack>()
            .add_audio_source::<AmbienceTrack>()
            .add_message::<MusicStinger>()
            .add_systems(Startup, setup_music)
            .add_systems(
                Update,
                (
                    music_director_system.run_if(resource_exists::<MusicManager>),
                    ambience_director_system.run_if(resource_exists::<AmbienceManager>),
                )
                    .chain(),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cave_factor_is_zero_above_and_one_deep_below() {
        assert_eq!(cave_factor(70.0, 64.0), 0.0);
        assert_eq!(cave_factor(50.0, 64.0), 1.0);
        let middle = cave_factor(58.0, 64.0);
        assert!(middle > 0.0 && middle < 1.0);
    }

    #[test]
    fn wildlife_threat_scales_with_proximity_and_aggro() {
        let player = Vec3::ZERO;
        let calm_far = wildlife_threat(player, [(Vec3::new(25.0, 0.0, 0.0), 0.0, 4.0)].into_iter());
        let angry_close =
            wildlife_threat(player, [(Vec3::new(3.0, 0.0, 0.0), 1.0, 4.0)].into_iter());
        assert!(angry_close > calm_far);
        let dead = wildlife_threat(player, [(Vec3::new(3.0, 0.0, 0.0), 1.0, 0.0)].into_iter());
        assert_eq!(dead, 0.0);
    }

    #[test]
    fn danger_level_covers_health_o2_and_hazard() {
        assert_eq!(danger_level(8.0, 8.0, 100.0, 100.0, false), 0.0);
        assert!(danger_level(1.0, 8.0, 100.0, 100.0, false) > 0.8);
        assert!(danger_level(8.0, 8.0, 10.0, 100.0, false) > 0.5);
        assert!(danger_level(8.0, 8.0, 100.0, 5.0, false) > 0.5);
        assert_eq!(danger_level(0.0, 8.0, 0.0, 0.0, true), 0.0);
    }

    #[test]
    fn planet_key_is_deterministic() {
        let a = planet_key_preview(42, "frozen");
        let b = planet_key_preview(42, "frozen");
        assert_eq!(a, b);
        let different = planet_key_preview(43, "frozen");
        let _ = different;
    }
}
