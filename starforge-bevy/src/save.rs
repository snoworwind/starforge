//! Save/load — Terraria-style separated character & world saves, JSON files under `saves/`.
//! Mirrors the original's v4 JSON schema (adapted to native files instead of IndexedDB).

use crate::inventory::Slot;
use crate::player::Player;
use crate::world::World;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

pub const SAVE_VERSION: u32 = 5;

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

fn d_skin() -> String {
    "#e8c49a".into()
}
fn d_hair_style() -> String {
    "short".into()
}
fn d_hair() -> String {
    "#4a3018".into()
}
fn d_suit() -> String {
    "#4a5a6e".into()
}
fn d_trim() -> String {
    "#35e0e8".into()
}
fn d_pants() -> String {
    "#33404c".into()
}
fn d_boots() -> String {
    "#1e262e".into()
}
fn d_helmet() -> bool {
    true
}
fn d_visor() -> String {
    "#ffb347".into()
}

impl Appearance {
    pub fn random(seed: u32) -> Self {
        let mut r = crate::rng::Rng::new(seed);
        let skins = [
            "#e8c49a", "#d8b48a", "#c89878", "#8d5a3c", "#6b4630", "#f0d8b8", "#b98e6a", "#e8d0b0",
        ];
        let hairs = [
            "#4a3018", "#2e2620", "#5a4632", "#7a5a8a", "#a86a3a", "#d8c8a8", "#c23a3a", "#1e2e4a",
        ];
        let styles = ["short", "long", "pony", "mohawk", "bun"];
        let suits = [
            "#4a5a6e", "#3fa8c9", "#5a3e3e", "#6e6a2a", "#3e5a6e", "#4a4258", "#5a6a3a", "#7a3a2a",
        ];
        let trims = [
            "#35e0e8", "#ffb347", "#ff6a5e", "#b58aff", "#7dff8a", "#ffd94d", "#f0f0f0", "#35b0ff",
        ];
        let pants = [
            "#33404c", "#4a3c2e", "#2e3a44", "#3a3248", "#3e3a2e", "#443430",
        ];
        let boots = [
            "#1e262e", "#2e2620", "#26221a", "#241e2e", "#2a221e", "#33261a",
        ];
        let visors = [
            "#ffb347", "#35e0e8", "#ff6a5e", "#b58aff", "#7dff8a", "#f0f0f0",
        ];
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

fn d_model() -> String {
    "ship".into()
}
fn d_cls() -> String {
    "C".into()
}
fn d_ship_name() -> String {
    "拓荒者号".into()
}

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
    #[serde(default)]
    pub equipment: crate::player::Equipment,
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
    /// 进行中的研究（JS researching {id, t}）
    #[serde(default)]
    pub researching: Option<(String, f32)>,
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
    pub stock: HashMap<String, i32>,
    #[serde(default)]
    pub flags: HashMap<String, bool>,
    /// 地面飞船停泊点
    #[serde(default)]
    pub ship_pos: Option<[f32; 3]>,
    /// 太空飞船状态（state=space 时存在）
    #[serde(default)]
    pub ship_state: Option<ShipStateSave>,
    /// 当前星球地图标记（JS mapMarks[pid]）
    #[serde(default)]
    pub marks: Vec<crate::space::Mark>,
    /// 曲率跃迁锁定
    #[serde(default)]
    pub warp_lock: Option<crate::space::WarpLock>,
    /// 放置任务计数（JS placedCount）
    #[serde(default)]
    pub placed: HashMap<String, i32>,
    /// 跨星系档案（JS galaxyArchives）
    #[serde(default)]
    pub archives: HashMap<u32, crate::space::GalaxyArchive>,
    /// 当前星球兽群（MC 风格：位置/血量/领地随存档保存，被杀不复活）
    #[serde(default)]
    pub creatures: Vec<crate::creatures::HerdSave>,
    /// 兽群细胞占用/被杀位图
    #[serde(default)]
    pub creature_cells: Vec<crate::creatures::CellSave>,
}

fn d_state() -> String {
    "planet".into()
}
fn d_galaxy_seed() -> u32 {
    crate::data::HOME_GALAXY_SEED
}
fn d_galaxy_count() -> u32 {
    1
}

pub fn saves_dir() -> PathBuf {
    // Preserve an existing working-directory save tree so development runs
    // do not strand the user's worlds. For packaged builds, or when launched
    // from a shortcut with no save tree in the working directory, fall back
    // beside the executable just like the asset root.
    let cwd_saves = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("saves");
    if cwd_saves.is_dir() {
        return cwd_saves;
    }
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join("saves")
}

fn safe_name(name: &str) -> String {
    let mut out: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | ' ' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    while out.ends_with('.') || out.ends_with(' ') {
        out.pop();
    }
    let device_name = out
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved_device = matches!(
        device_name.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    );
    if out.is_empty() || out == "." || out == ".." || reserved_device {
        "unnamed".into()
    } else {
        out.chars().take(80).collect()
    }
}

pub fn char_path(name: &str) -> PathBuf {
    saves_dir()
        .join("chars")
        .join(format!("{}.char.json", safe_name(name)))
}

pub fn world_path(name: &str) -> PathBuf {
    saves_dir()
        .join("worlds")
        .join(format!("{}.world.json", safe_name(name)))
}

#[allow(clippy::too_many_arguments)]
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
    researching: Option<&(String, f32)>,
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
        equipment: p.equipment.clone(),
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
        researching: researching.cloned(),
    };
    write_json(&char_path(name), &data)
}

pub fn load_char(name: &str) -> Option<CharData> {
    let mut data: CharData = read_json(&char_path(name))?;
    data.hot_idx = data.hot_idx.clamp(-1, 8);
    data.inv = crate::inventory::Inventory::from_slots(data.inv).slots;
    data.equipment.sanitize();
    data.quest_idx = data.quest_idx.min(crate::data::QUESTS.len());
    let mut seen_techs = std::collections::HashSet::new();
    data.techs.retain(|tech| {
        crate::data::TECHS.iter().any(|known| known.id == tech) && seen_techs.insert(tech.clone())
    });
    let cargo_slots = crate::data::ship_class_by_key(&data.player_ship.cls).slots;
    data.player_ship.inv =
        crate::inventory::Inventory::from_slots_with_capacity(data.player_ship.inv, cargo_slots)
            .slots;
    data.ship_garage.truncate(64);
    for ship in &mut data.ship_garage {
        let slots = crate::data::ship_class_by_key(&ship.cls).slots;
        ship.inv = crate::inventory::Inventory::from_slots_with_capacity(
            std::mem::take(&mut ship.inv),
            slots,
        )
        .slots;
    }
    if let Some((_, progress)) = &mut data.researching {
        if !progress.is_finite() {
            *progress = 0.0;
        }
        *progress = progress.clamp(0.0, 1_000_000.0);
    }
    data.play_time = if data.play_time.is_finite() {
        data.play_time.max(0.0)
    } else {
        0.0
    };
    Some(data)
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
    stock: &HashMap<String, i32>,
    flags: &HashMap<String, bool>,
    ship_pos: Option<[f32; 3]>,
    ship_state: Option<&ShipStateSave>,
    marks: &[crate::space::Mark],
    warp_lock: Option<&crate::space::WarpLock>,
    placed: &HashMap<String, i32>,
    archives: &HashMap<u32, crate::space::GalaxyArchive>,
    creatures: &[crate::creatures::HerdSave],
    creature_cells: &[crate::creatures::CellSave],
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
        stock: stock.clone(),
        flags: flags.clone(),
        ship_pos,
        ship_state: ship_state.cloned(),
        marks: marks.to_vec(),
        warp_lock: warp_lock.cloned(),
        placed: placed.clone(),
        archives: archives.clone(),
        creatures: creatures.to_vec(),
        creature_cells: creature_cells.to_vec(),
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
        &HashMap::new(),
        None,
        None,
        &[],
        None,
        &HashMap::new(),
        &HashMap::new(),
        &[],
        &[],
    )
}

pub fn load_world(name: &str) -> Option<WorldData> {
    let mut data: WorldData = read_json(&world_path(name))?;
    // A valid chunk RLE contains at most two entries per voxel. Rejecting
    // malformed entries here prevents a corrupt save from being retained and
    // re-parsed forever during streaming.
    if data.mods.len() > 200_000 {
        return None;
    }
    data.mods.retain(|key, pairs| {
        let mut parts = key.split(',');
        let valid_key = parts
            .next()
            .and_then(|x| x.parse::<i32>().ok())
            .zip(parts.next().and_then(|z| z.parse::<i32>().ok()))
            .is_some_and(|(x, z)| x.abs() <= 1_000_000 && z.abs() <= 1_000_000);
        valid_key
            && pairs.len()
                <= crate::data::CHUNK as usize
                    * crate::data::CHUNK as usize
                    * crate::data::WORLD_H as usize
                    * 2
    });
    data.day_t = if data.day_t.is_finite() {
        data.day_t.rem_euclid(1.0)
    } else {
        0.0
    };
    data.galaxy_count = data.galaxy_count.clamp(1, 1024);
    data.marks.truncate(4096);
    data.flags = data.flags.into_iter().take(4096).collect();
    data.market = data
        .market
        .into_iter()
        .filter(|(_, value)| value.is_finite() && *value >= 0.0 && *value <= 1_000_000.0)
        .take(4096)
        .collect();
    data.stock = data
        .stock
        .into_iter()
        .filter(|(item, amount)| {
            crate::data::item_by_key(item).is_some() && (0..=100_000).contains(amount)
        })
        .take(4096)
        .collect();
    data.placed = data.placed.into_iter().take(4096).collect();
    data.archives = data.archives.into_iter().take(256).collect();
    Some(data)
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
        Ok(json) => {
            let Some(parent) = path.parent() else {
                return false;
            };
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("save.json");
            let tmp = parent.join(format!(".{name}.{}.tmp", std::process::id()));
            let result = (|| {
                use std::io::Write;
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .truncate(true)
                    .write(true)
                    .open(&tmp)?;
                file.write_all(json.as_bytes())?;
                file.sync_all()?;
                match std::fs::rename(&tmp, path) {
                    Ok(()) => Ok(()),
                    Err(_) if path.exists() => {
                        // Windows does not replace an existing file with
                        // rename. The target is removed only after the fully
                        // written temporary file is synced.
                        std::fs::remove_file(path)?;
                        std::fs::rename(&tmp, path)
                    }
                    Err(e) => Err(e),
                }
            })();
            if result.is_err() {
                let _ = std::fs::remove_file(&tmp);
            }
            result.is_ok()
        }
        Err(_) => false,
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &PathBuf) -> Option<T> {
    const MAX_SAVE_BYTES: u64 = 64 * 1024 * 1024;
    if std::fs::metadata(path).ok()?.len() > MAX_SAVE_BYTES {
        return None;
    }
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
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LodMode {
    Legacy,
    #[default]
    Hierarchical,
}

#[derive(Serialize, Deserialize, Clone, Debug, Resource)]
pub struct Settings {
    pub view_dist: i32,
    #[serde(default)]
    pub lod_mode: LodMode,
    pub mouse_sens: f32,
    pub volume: f32,
    pub show_fps: bool,
    #[serde(default)]
    pub pixelated: bool,
    #[serde(default = "default_enabled")]
    pub clouds: bool,
    #[serde(default = "default_enabled")]
    pub weather: bool,
    #[serde(default = "default_cloud_coverage")]
    pub cloud_coverage: f32,
    #[serde(default = "default_cloud_density")]
    pub cloud_density: f32,
    #[serde(default = "default_cloud_raymarch_steps")]
    pub cloud_raymarch_steps: u32,
    #[serde(default = "default_cloud_render_width")]
    pub cloud_render_width: u32,
    #[serde(default = "default_cloud_render_height")]
    pub cloud_render_height: u32,
    /// F3 lighting/Bloom controls. Nested so older settings files remain
    /// compatible and deserialize this whole group from `Default`.
    #[serde(default)]
    pub lighting: crate::daynight::LightingTuning,
}

fn default_enabled() -> bool {
    true
}

fn default_cloud_coverage() -> f32 {
    0.61
}

fn default_cloud_density() -> f32 {
    0.09
}

fn default_cloud_raymarch_steps() -> u32 {
    24
}

fn default_cloud_render_width() -> u32 {
    1536
}

fn default_cloud_render_height() -> u32 {
    864
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            view_dist: 10,
            lod_mode: LodMode::Hierarchical,
            mouse_sens: 1.0,
            volume: 0.8,
            show_fps: false,
            pixelated: false,
            clouds: true,
            weather: true,
            cloud_coverage: default_cloud_coverage(),
            cloud_density: default_cloud_density(),
            cloud_raymarch_steps: default_cloud_raymarch_steps(),
            cloud_render_width: default_cloud_render_width(),
            cloud_render_height: default_cloud_render_height(),
            lighting: crate::daynight::LightingTuning::default(),
        }
    }
}

fn sanitize_cloud_settings(settings: &mut Settings) {
    settings.cloud_coverage = if settings.cloud_coverage.is_finite() {
        settings.cloud_coverage.clamp(0.0, 1.0)
    } else {
        default_cloud_coverage()
    };
    settings.cloud_density = if settings.cloud_density.is_finite() {
        settings.cloud_density.clamp(0.0, 1.0)
    } else {
        default_cloud_density()
    };
    settings.cloud_raymarch_steps = settings.cloud_raymarch_steps.clamp(4, 64);
    if !matches!(
        (settings.cloud_render_width, settings.cloud_render_height),
        (1280, 720) | (1536, 864) | (1920, 1080) | (2560, 1600)
    ) {
        settings.cloud_render_width = default_cloud_render_width();
        settings.cloud_render_height = default_cloud_render_height();
    }
}

pub fn load_settings() -> Settings {
    let mut settings: Settings = read_json(&saves_dir().join("settings.json")).unwrap_or_default();
    settings.view_dist = settings.view_dist.clamp(3, 32);
    settings.mouse_sens = if settings.mouse_sens.is_finite() {
        settings.mouse_sens.clamp(0.05, 5.0)
    } else {
        1.0
    };
    settings.volume = if settings.volume.is_finite() {
        settings.volume.clamp(0.0, 1.0)
    } else {
        0.8
    };
    settings.lighting.sanitize();
    sanitize_cloud_settings(&mut settings);
    settings
}

pub fn save_settings(s: &Settings) -> bool {
    let mut safe = s.clone();
    safe.view_dist = safe.view_dist.clamp(3, 32);
    safe.mouse_sens = if safe.mouse_sens.is_finite() {
        safe.mouse_sens.clamp(0.05, 5.0)
    } else {
        1.0
    };
    safe.volume = if safe.volume.is_finite() {
        safe.volume.clamp(0.0, 1.0)
    } else {
        0.8
    };
    safe.lighting.sanitize();
    sanitize_cloud_settings(&mut safe);
    write_json(&saves_dir().join("settings.json"), &safe)
}

/// Inserts the app-wide Settings resource (loaded once from disk by main
/// and passed in through the plugin group config).
pub struct SaveSettingsPlugin(pub Settings);

impl Plugin for SaveSettingsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.0.clone());
    }
}
