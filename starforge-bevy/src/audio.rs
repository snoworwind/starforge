//! 音效库 — Sonniss GDC 2026 素材（assets/audio/Sonniss.com-*.wav，编译期内嵌）。
//! 许可证存于 assets/licenses/sonniss_gdc2026_LICENSE.pdf。
//! 音量主控见 set_master_volume。

use bevy::audio::{AudioSource, PlaybackMode, PlaybackSettings};
use bevy::prelude::*;
use std::sync::Arc;
use std::time::Duration;

/// 主音量（0..1），f32 位存储以支持原子读写。
static MASTER_VOL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3F80_0000);

pub fn set_master_volume(v: f32) {
    MASTER_VOL.store(
        v.clamp(0.0, 1.0).to_bits(),
        std::sync::atomic::Ordering::Relaxed,
    );
}

pub fn master_volume() -> f32 {
    f32::from_bits(MASTER_VOL.load(std::sync::atomic::Ordering::Relaxed))
}

#[derive(Resource)]
pub struct Sfx {
    // 原有 12 音（保持调用点兼容）
    pub dig: Handle<AudioSource>,
    pub place: Handle<AudioSource>,
    pub break_block: Handle<AudioSource>,
    pub jump: Handle<AudioSource>,
    pub hurt: Handle<AudioSource>,
    pub pickup: Handle<AudioSource>,
    pub click: Handle<AudioSource>,
    pub craft: Handle<AudioSource>,
    pub jet: Handle<AudioSource>,
    pub laser_hit: Handle<AudioSource>,
    pub error: Handle<AudioSource>,
    pub alarm: Handle<AudioSource>,
    pub rain: Handle<AudioSource>,
    // 新增（JS audio.js 触发点补全）
    pub hover: Handle<AudioSource>,
    pub ui_open: Handle<AudioSource>,
    pub ui_close: Handle<AudioSource>,
    pub explosion: Handle<AudioSource>,
    pub engine_loop: Handle<AudioSource>,
    pub shoot: Handle<AudioSource>,
    pub scan: Handle<AudioSource>,
    pub research: Handle<AudioSource>,
    pub coin: Handle<AudioSource>,
    pub takeoff: Handle<AudioSource>,
    pub land_ship: Handle<AudioSource>,
    pub dock: Handle<AudioSource>,
    pub step: Handle<AudioSource>,
    pub land: Handle<AudioSource>,
    pub creature_die: Handle<AudioSource>,
    pub creature_hit: Handle<AudioSource>,
    pub open_chest: Handle<AudioSource>,
    pub insert: Handle<AudioSource>,
    pub pulse: Handle<AudioSource>,
    pub warp: Handle<AudioSource>,
}

macro_rules! sonniss_wav {
    ($assets:expr, $name:literal) => {{
        $assets.add(AudioSource {
            bytes: Arc::from(include_bytes!(concat!("../assets/audio/", $name)) as &[u8]),
        })
    }};
}

macro_rules! legacy_ogg {
    ($assets:expr, $name:literal) => {{
        $assets.add(AudioSource {
            bytes: Arc::from(include_bytes!(concat!("../assets/audio/", $name)) as &[u8]),
        })
    }};
}

impl Sfx {
    pub fn build(assets: &mut Assets<AudioSource>, volume: f32) -> Self {
        set_master_volume(volume);
        Self {
            dig: sonniss_wav!(assets, "Sonniss.com-dig.wav"),
            place: sonniss_wav!(assets, "Sonniss.com-place.wav"),
            break_block: sonniss_wav!(assets, "Sonniss.com-break-block.wav"),
            jump: sonniss_wav!(assets, "Sonniss.com-jump.wav"),
            hurt: sonniss_wav!(assets, "Sonniss.com-hurt.wav"),
            pickup: sonniss_wav!(assets, "Sonniss.com-pickup.wav"),
            click: sonniss_wav!(assets, "Sonniss.com-click.wav"),
            craft: sonniss_wav!(assets, "Sonniss.com-craft.wav"),
            // Jetpack uses the short hover texture; the ship keeps the
            // separate long engine loop below.
            jet: sonniss_wav!(assets, "Sonniss.com-jet.wav"),
            laser_hit: sonniss_wav!(assets, "Sonniss.com-laser-hit.wav"),
            error: sonniss_wav!(assets, "Sonniss.com-error.wav"),
            alarm: sonniss_wav!(assets, "Sonniss.com-alarm.wav"),
            rain: sonniss_wav!(assets, "Sonniss.com-rain.wav"),
            hover: sonniss_wav!(assets, "Sonniss.com-hover.wav"),
            ui_open: sonniss_wav!(assets, "Sonniss.com-ui-open.wav"),
            ui_close: sonniss_wav!(assets, "Sonniss.com-ui-close.wav"),
            explosion: sonniss_wav!(assets, "Sonniss.com-explosion.wav"),
            engine_loop: sonniss_wav!(assets, "Sonniss.com-engine-loop.wav"),
            shoot: sonniss_wav!(assets, "Sonniss.com-shoot.wav"),
            scan: sonniss_wav!(assets, "Sonniss.com-scan.wav"),
            research: sonniss_wav!(assets, "Sonniss.com-research.wav"),
            coin: sonniss_wav!(assets, "Sonniss.com-coin.wav"),
            takeoff: sonniss_wav!(assets, "Sonniss.com-takeoff.wav"),
            land_ship: sonniss_wav!(assets, "Sonniss.com-land-ship.wav"),
            dock: sonniss_wav!(assets, "Sonniss.com-dock.wav"),
            // Keep the pre-Sonniss footstep asset for the player's ground steps.
            step: legacy_ogg!(assets, "step.ogg"),
            land: sonniss_wav!(assets, "Sonniss.com-land.wav"),
            creature_die: sonniss_wav!(assets, "Sonniss.com-creature-die.wav"),
            creature_hit: sonniss_wav!(assets, "Sonniss.com-creature-hit.wav"),
            open_chest: sonniss_wav!(assets, "Sonniss.com-open-chest.wav"),
            insert: sonniss_wav!(assets, "Sonniss.com-insert.wav"),
            pulse: sonniss_wav!(assets, "Sonniss.com-pulse.wav"),
            warp: sonniss_wav!(assets, "Sonniss.com-warp.wav"),
        }
    }
}

/// Play a one-shot sound（音量受主音量缩放）。
pub fn play(commands: &mut Commands, handle: Handle<AudioSource>, volume: f32, pitch: Option<f32>) {
    commands.spawn((
        AudioPlayer(handle),
        OneShotSound::default(),
        PlaybackSettings {
            mode: PlaybackMode::Despawn,
            volume: bevy::audio::Volume::Linear(volume * master_volume()),
            speed: pitch.unwrap_or(1.0),
            ..default()
        },
    ));
}

/// Play a one-shot sound from a world-space position.
///
/// This is intentionally separate from [`play`]: UI and player-local sounds
/// should stay centered, while combat, wildlife, and station sounds should
/// pan according to their position relative to the camera listener.
pub fn play_spatial(
    commands: &mut Commands,
    handle: Handle<AudioSource>,
    position: Vec3,
    volume: f32,
    pitch: Option<f32>,
) {
    commands.spawn((
        AudioPlayer(handle),
        OneShotSound::default(),
        Transform::from_translation(position),
        PlaybackSettings {
            mode: PlaybackMode::Despawn,
            spatial: true,
            volume: bevy::audio::Volume::Linear(volume * master_volume()),
            speed: pitch.unwrap_or(1.0),
            ..default()
        },
    ));
}

/// Lifetime guard for short-lived effects created by [`play`] or
/// [`play_spatial`]. The audio backend normally despawns these when the clip
/// ends; the TTL also cleans up a malformed/blocked source so it cannot keep
/// consuming mixer state forever.
#[derive(Component)]
pub struct OneShotSound {
    ttl: f32,
}

impl Default for OneShotSound {
    fn default() -> Self {
        Self { ttl: 8.0 }
    }
}

/// Keep a bad/long source or a burst of repeated effects from exhausting the
/// audio mixer. Looping engine/jet sounds are deliberately not counted here.
pub fn limit_one_shots(
    time: Res<Time>,
    mut commands: Commands,
    mut sounds: Query<(Entity, &mut OneShotSound)>,
) {
    const MAX_ONE_SHOTS: usize = 24;
    let mut active = Vec::new();
    for (entity, mut sound) in &mut sounds {
        sound.ttl -= time.delta_secs();
        if sound.ttl > 0.0 {
            active.push(entity);
        } else {
            commands.entity(entity).despawn();
        }
    }
    let extra = active.len().saturating_sub(MAX_ONE_SHOTS);
    for entity in active.into_iter().take(extra) {
        commands.entity(entity).despawn();
    }
}

/// 循环音实体（despawn 即停）。
#[derive(Component)]
pub struct LoopSound;

/// 启动一个循环音。
pub fn play_loop(commands: &mut Commands, handle: Handle<AudioSource>, volume: f32) -> Entity {
    commands
        .spawn((
            AudioPlayer(handle),
            PlaybackSettings {
                mode: PlaybackMode::Loop,
                volume: bevy::audio::Volume::Linear(volume * master_volume()),
                ..default()
            },
            LoopSound,
            crate::InGame,
        ))
        .id()
}

/// Jetpack playback state: start at the beginning, then loop from 7 seconds.
#[derive(Component)]
pub struct JetSound {
    elapsed: f32,
    looped: bool,
    handle: Handle<AudioSource>,
    volume: f32,
}

pub fn play_jet(commands: &mut Commands, handle: Handle<AudioSource>, volume: f32) -> Entity {
    let scaled_volume = volume * master_volume();
    commands
        .spawn((
            AudioPlayer(handle.clone()),
            PlaybackSettings {
                mode: PlaybackMode::Once,
                volume: bevy::audio::Volume::Linear(scaled_volume),
                ..default()
            },
            JetSound {
                elapsed: 0.0,
                looped: false,
                handle,
                volume: scaled_volume,
            },
            crate::InGame,
        ))
        .id()
}

/// Switch a held jetpack from its intro into a loop beginning at 7 seconds.
pub fn advance_jet_sounds(
    time: Res<Time>,
    mut commands: Commands,
    mut jets: Query<(Entity, &mut JetSound)>,
) {
    for (entity, mut jet) in &mut jets {
        if jet.looped {
            continue;
        }
        jet.elapsed += time.delta_secs();
        if jet.elapsed < 7.0 {
            continue;
        }
        jet.looped = true;
        commands.entity(entity).remove::<AudioPlayer>();
        commands.entity(entity).insert((
            AudioPlayer(jet.handle.clone()),
            PlaybackSettings {
                mode: PlaybackMode::Loop,
                start_position: Some(Duration::from_secs(7)),
                volume: bevy::audio::Volume::Linear(jet.volume),
                ..default()
            },
        ));
    }
}

/// Start a looping sound at a world-space position.
pub fn play_loop_spatial(
    commands: &mut Commands,
    handle: Handle<AudioSource>,
    position: Vec3,
    volume: f32,
) -> Entity {
    commands
        .spawn((
            AudioPlayer(handle),
            Transform::from_translation(position),
            PlaybackSettings {
                mode: PlaybackMode::Loop,
                spatial: true,
                volume: bevy::audio::Volume::Linear(volume * master_volume()),
                ..default()
            },
            LoopSound,
            crate::InGame,
        ))
        .id()
}
