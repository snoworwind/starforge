//! Save/load — Terraria-style separated character & world saves, JSON files under `saves/`.
//! Mirrors the original's v4 JSON schema (adapted to native files instead of IndexedDB).

use crate::inventory::Slot;
use crate::player::Player;
use crate::world::World;
use serde::{Deserialize, Serialize};
use bevy::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;

pub const SAVE_VERSION: u32 = 4;

/// 外观（捏人）— 与原始 char record appearance 字段一致。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Appearance {
    #[serde(default = "d_skin")]
    pub skin: String,
    #[serde(default = "d_hair_style")]
    pub hair_style: String,
    #[serde(default = "d_hair")]
    pub hair: String,
    #[serde(default = "d_suit")]
    pub suit: String,
    #[serde(default = "d_trim")]
    pub trim: String,
    #[serde(default = "d_pants")]
    pub pants: String,
    #[serde(default = "d_boots")]
    pub boots: String,
    #[serde(default = "d_helmet")]
    pub helmet: bool,
    #[serde(default = "d_visor")]
    pub visor: String,
}

fn d_skin() -> String { "#e8c49a".into() }
fn d_hair_style() -> String { "short".into() }
fn d_hair() -> String { "#4a3018".into() }
fn d_suit() -> String { "#4a5a6e".into() }
fn d_trim() -> String { "#35e0e8".into() }
fn d_pants() -> String { "#33404c".into() }
fn d_boots() -> String { "#1e262e".into() }
fn d_helmet() -> bool { true }
fn d_visor() -> String { "#ffb347".into() }

impl Appearance {
    pub fn random(seed: u32) -> Self {
        let mut r = crate::rng::Rng::new(seed);
        let skins = ["#e8c49a", "#d8b48a", "#c89878", "#8d5a3c", "#6b4630", "#f0d8b8", "#b98e6a", "#e8d0b0"];
        let hairs = ["#4a3018", "#2e2620", "#5a4632", "#7a5a8a", "#a86a3a", "#d8c8a8", "#c23a3a", "#1e2e4a"];
        let styles = ["short", "long", "pony", "mohawk", "bun"];
        let suits = ["#4a5a6e", "#3fa8c9", "#5a3e3e", "#6e6a2a", "#3e5a6e", "#4a4258", "#5a6a3a", "#7a3a2a"];
        let trims = ["#35e0e8", "#ffb347", "#ff6a5e", "#b58aff", "#7dff8a", "#ffd94d", "#f0f0f0", "#35b0ff"];
        let pants = ["#33404c", "#4a3c2e", "#2e3a44", "#3a3248", "#3e3a2e", "#443430"];
        let boots = ["#1e262e", "#2e2620", "#26221a", "#241e2e", "#2a221e", "#33261a"];
        let visors = ["#ffb347", "#35e0e8", "#ff6a5e", "#b58aff", "#7dff8a", "#f0f0f0"];
        Self {
            skin: skins[r.range(skins.len())].into(),
            hair_style: styles[r.range(styles.len())].into(),
            hair: hairs[r.range(hairs.len())].into(),
            suit: suits[r.range(suits.len())].into(),
            trim: trims[r.range(trims.len())].into(),
            pants: pants[r.range(pants.len())].into(),
            boots: boots[r.range(boots.len())].into(),
            helmet: r.next() < 0.7,
            visor: visors[r.range(visors.len())].into(),
        }
    }

    pub fn style_label(style: &str) -> &'static str {
        match style {
            "none" => "无",
            "short" => "短发",
            "long" => "长发",
            "pony" => "马尾",
            "mohawk" => "莫霍克",
            "bun" => "发髻",
            _ => "短发",
        }
    }
}

/// 飞船存档（playerShip / shipGarage 条目）。
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ShipSave {
    #[serde(default = "d_model")]
    pub model: String,
    #[serde(default = "d_cls")]
    pub cls: String,
    #[serde(default = "d_ship_name")]
    pub name: String,
    #[serde(default)]
    pub inv: Vec<Option<Slot>>,
}

fn d_model() -> String { "ship".into() }
fn d_cls() -> String { "C".into() }
fn d_ship_name() -> String { "拓荒者号".into() }

/// 太空船位置/姿态（world record shipState）。
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ShipStateSave {
    #[serde(default)]
    pub pos: [f32; 3],
    #[serde(default)]
    pub yaw: f32,
    #[serde(default)]
    pub pitch: f32,
    #[serde(default)]
    pub roll: f32,
    #[serde(default)]
    pub speed: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CharData {
    pub v: u32,
    pub kind: String,
    pub name: String,
    pub pos: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub stats: [f32; 6], // hp, shield, o2, haz, jet, laser
    pub inv: Vec<Option<Slot>>,
    pub hot_idx: i32,
    pub credits: i32,
    pub difficulty: u8, // 0 easy, 1 normal, 2 hard, 3 creative
    pub techs: Vec<String>,
    pub world: Option<String>,
    #[serde(default)]
    pub appearance: Appearance,
    #[serde(default)]
    pub fuel_loaded: i32,
    #[serde(default)]
    pub player_ship: ShipSave,
    #[serde(default)]
    pub ship_garage: Vec<ShipSave>,
    #[serde(default)]
    pub quest_idx: usize,
    #[serde(default)]
    pub play_time: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WorldData {
    pub v: u32,
    pub kind: String,
    pub name: String,
    pub seed: u32,
    pub biome: String,
    pub day_t: f32,
    pub mods: HashMap<String, Vec<u16>>,
    /// "planet" | "space"
    #[serde(default = "d_state")]
    pub state: String,
    #[serde(default)]
    pub current_planet: usize,
    #[serde(default = "d_galaxy_seed")]
    pub galaxy_seed: u32,
    #[serde(default = "d_galaxy_count")]
    pub galaxy_count: u32,
    #[serde(default)]
    pub market: HashMap<String, f32>,
    #[serde(default)]
    pub flags: HashMap<String, bool>,
    /// 地面飞船停泊点
    #[serde(default)]
    pub ship_pos: Option<[f32; 3]>,
    /// 太空飞船状态（state=space 时存在）
    #[serde(default)]
    pub ship_state: Option<ShipStateSave>,
}

fn d_state() -> String { "planet".into() }
fn d_galaxy_seed() -> u32 { crate::data::HOME_GALAXY_SEED }
fn d_galaxy_count() -> u32 { 1 }

pub fn saves_dir() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("saves")
}

pub fn char_path(name: &str) -> PathBuf {
    saves_dir().join("chars").join(format!("{name}.char.json"))
}

pub fn world_path(name: &str) -> PathBuf {
    saves_dir().join("worlds").join(format!("{name}.world.json"))
}

pub fn save_char(
    p: &Player,
    name: &str,
    world_name: Option<&str>,
    techs: &[String],
    appearance: &Appearance,
    fuel_loaded: i32,
    player_ship: &ShipSave,
    ship_garage: &[ShipSave],
    quest_idx: usize,
) -> bool {
    let data = CharData {
        v: SAVE_VERSION,
        kind: "char".into(),
        name: name.into(),
        pos: [p.pos.x, p.pos.y, p.pos.z],
        yaw: p.yaw,
        pitch: p.pitch,
        stats: [
            p.stats.hp,
            p.stats.shield,
            p.stats.o2,
            p.stats.haz,
            p.stats.jet,
            p.stats.laser,
        ],
        inv: p.inv.slots.clone(),
        hot_idx: p.hot_idx,
        credits: p.credits,
        difficulty: match p.difficulty {
            crate::data::Difficulty::Easy => 0,
            crate::data::Difficulty::Normal => 1,
            crate::data::Difficulty::Hard => 2,
            crate::data::Difficulty::Creative => 3,
        },
        techs: techs.to_vec(),
        world: world_name.map(|s| s.to_string()),
        appearance: appearance.clone(),
        fuel_loaded,
        player_ship: player_ship.clone(),
        ship_garage: ship_garage.to_vec(),
        quest_idx,
        play_time: p.play_time,
    };
    write_json(&char_path(name), &data)
}

pub fn load_char(name: &str) -> Option<CharData> {
    read_json(&char_path(name))
}

#[allow(clippy::too_many_arguments)]
pub fn save_world_full(
    world: &World,
    name: &str,
    day_t: f32,
    state: &str,
    current_planet: usize,
    galaxy_seed: u32,
    galaxy_count: u32,
    market: &HashMap<String, f32>,
    flags: &HashMap<String, bool>,
    ship_pos: Option<[f32; 3]>,
    ship_state: Option<&ShipStateSave>,
) -> bool {
    let data = WorldData {
        v: SAVE_VERSION,
        kind: "world".into(),
        name: name.into(),
        seed: world.seed,
        biome: world.biome().key.into(),
        day_t,
        mods: world.serialize_mods(),
        state: state.into(),
        current_planet,
        galaxy_seed,
        galaxy_count,
        market: market.clone(),
        flags: flags.clone(),
        ship_pos,
        ship_state: ship_state.cloned(),
    };
    write_json(&world_path(name), &data)
}

pub fn save_world(world: &World, name: &str, day_t: f32) -> bool {
    save_world_full(
        world,
        name,
        day_t,
        "planet",
        0,
        crate::data::HOME_GALAXY_SEED,
        1,
        &HashMap::new(),
        &HashMap::new(),
        None,
        None,
    )
}

pub fn load_world(name: &str) -> Option<WorldData> {
    read_json(&world_path(name))
}

pub fn list_worlds() -> Vec<String> {
    list_files(&saves_dir().join("worlds"), "world.json")
}

pub fn list_chars() -> Vec<String> {
    list_files(&saves_dir().join("chars"), "char.json")
}

fn list_files(dir: &PathBuf, suffix: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(stem) = name.strip_suffix(&format!(".{suffix}")) {
                out.push(stem.to_string());
            }
        }
    }
    out.sort();
    out
}

fn write_json<T: Serialize>(path: &PathBuf, data: &T) -> bool {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_string_pretty(data) {
        Ok(json) => std::fs::write(path, json).is_ok(),
        Err(_) => false,
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &PathBuf) -> Option<T> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// List worlds that exist on disk as (name, seed, biome) for the menu.
pub fn world_summaries() -> Vec<(String, u32, String)> {
    let mut out = Vec::new();
    for name in list_worlds() {
        if let Some(w) = load_world(&name) {
            out.push((name, w.seed, w.biome));
        }
    }
    out
}

/// Settings persisted to saves/settings.json.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[derive(Resource)]
pub struct Settings {
    pub view_dist: i32,
    pub mouse_sens: f32,
    pub volume: f32,
    pub show_fps: bool,
    #[serde(default)]
    pub pixelated: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            view_dist: 10,
            mouse_sens: 1.0,
            volume: 0.8,
            show_fps: false,
            pixelated: false,
        }
    }
}

pub fn load_settings() -> Settings {
    read_json(&saves_dir().join("settings.json")).unwrap_or_default()
}

pub fn save_settings(s: &Settings) -> bool {
    write_json(&saves_dir().join("settings.json"), s)
}
