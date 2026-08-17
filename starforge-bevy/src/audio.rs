//! 音效库 — Kenney CC0 素材（assets/audio/*.ogg，编译期内嵌）。
//! 来源：Kenney UI Audio + Kenney Sci-Fi Sounds（CC0 1.0，免署名），
//! 许可证存于 assets/licenses/。音量主控见 set_master_volume。

use bevy::audio::{AudioSource, PlaybackMode, PlaybackSettings};
use bevy::prelude::*;
use std::sync::Arc;

/// 主音量（0..1），f32 位存储以支持原子读写。
static MASTER_VOL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3F80_0000);

pub fn set_master_volume(v: f32) {
    MASTER_VOL.store(v.clamp(0.0, 1.0).to_bits(), std::sync::atomic::Ordering::Relaxed);
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

macro_rules! ogg {
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
            dig: ogg!(assets, "dig.ogg"),
            place: ogg!(assets, "place.ogg"),
            break_block: ogg!(assets, "break_block.ogg"),
            jump: ogg!(assets, "jump.ogg"),
            hurt: ogg!(assets, "hurt.ogg"),
            pickup: ogg!(assets, "pickup.ogg"),
            click: ogg!(assets, "click.ogg"),
            craft: ogg!(assets, "craft.ogg"),
            jet: ogg!(assets, "jet.ogg"),
            laser_hit: ogg!(assets, "laser_hit.ogg"),
            error: ogg!(assets, "error.ogg"),
            alarm: ogg!(assets, "alarm.ogg"),
            hover: ogg!(assets, "hover.ogg"),
            ui_open: ogg!(assets, "ui_open.ogg"),
            ui_close: ogg!(assets, "ui_close.ogg"),
            explosion: ogg!(assets, "explosion.ogg"),
            engine_loop: ogg!(assets, "engine_loop.ogg"),
            shoot: ogg!(assets, "shoot.ogg"),
            scan: ogg!(assets, "scan.ogg"),
            research: ogg!(assets, "research.ogg"),
            coin: ogg!(assets, "coin.ogg"),
            takeoff: ogg!(assets, "takeoff.ogg"),
            land_ship: ogg!(assets, "land_ship.ogg"),
            dock: ogg!(assets, "dock.ogg"),
            step: ogg!(assets, "step.ogg"),
            land: ogg!(assets, "land.ogg"),
            creature_die: ogg!(assets, "creature_die.ogg"),
            creature_hit: ogg!(assets, "creature_hit.ogg"),
            open_chest: ogg!(assets, "open_chest.ogg"),
            insert: ogg!(assets, "insert.ogg"),
            pulse: ogg!(assets, "pulse.ogg"),
            warp: ogg!(assets, "warp.ogg"),
        }
    }
}

/// Play a one-shot sound（音量受主音量缩放）。
pub fn play(
    commands: &mut Commands,
    handle: Handle<AudioSource>,
    volume: f32,
    pitch: Option<f32>,
) {
    commands.spawn((
        AudioPlayer(handle),
        PlaybackSettings {
            mode: PlaybackMode::Despawn,
            volume: bevy::audio::Volume::Linear(volume * master_volume()),
            speed: pitch.unwrap_or(1.0),
            ..default()
        },
    ));
}

/// 循环音实体（despawn 即停）。
#[derive(Component)]
pub struct LoopSound;

/// 启动一个循环音。
pub fn play_loop(
    commands: &mut Commands,
    handle: Handle<AudioSource>,
    volume: f32,
) -> Entity {
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
