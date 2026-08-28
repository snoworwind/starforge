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
    /// 当前船体生命；旧存档缺失时按船级上限恢复。
    #[serde(default)]
    pub hp: Option<f32>,
}

/// 跃迁途中继续动画所需的最小状态。
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct WarpAnimSave {
    #[serde(default)]
    pub t: f32,
    #[serde(default)]
    pub seed: u32,
    #[serde(default)]
    pub yaw: f32,
    #[serde(default)]
    pub pitch: f32,
    #[serde(default)]
    pub v0: f32,
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
    /// "planet" | "atmo" | "space" | "warping"
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
    /// 飞船位置/姿态/生命；飞行状态恢复时使用位置和姿态。
    #[serde(default)]
    pub ship_state: Option<ShipStateSave>,
    /// 跃迁动画状态（state=warping 时存在）。
    #[serde(default)]
    pub warp_anim: Option<WarpAnimSave>,
    /// 当前星球地图标记（JS mapMarks[pid]）
    #[serde(default)]
    pub marks: Vec<crate::space::Mark>,
    /// 曲率跃迁锁定
    #[serde(default)]
    pub warp_lock: Option<crate::space::WarpLock>,
    /// 放置任务计数（JS placedCount）
    #[serde(default)]
    pub placed: HashMap<String, i32>,
    /// 当前村庄支线；对话本身是瞬时 UI，不进入存档。
    #[serde(default)]
    pub side_quest: Option<crate::quests::SideQuest>,
    /// 当前活动星球的机器状态。
    #[serde(default)]
    pub machines: Vec<crate::factory::MachineSave>,
    /// 当前星系中已访问的非活动星球。
    #[serde(default)]
    pub visited: HashMap<usize, crate::space::PlanetArchive>,
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
    data.name = data.name.chars().filter(|c| !c.is_control()).take(80).collect();
    data.world = data
        .world
        .map(|world| world.chars().filter(|c| !c.is_control()).take(80).collect());
    data.credits = data.credits.clamp(0, 1_000_000_000);
    data.fuel_loaded = data.fuel_loaded.clamp(0, 1);
    data.hot_idx = data.hot_idx.clamp(-1, 8);
    data.inv = crate::inventory::Inventory::from_slots(data.inv).slots;
    data.equipment.sanitize();
    data.quest_idx = data.quest_idx.min(crate::data::QUESTS.len());
    let mut seen_techs = std::collections::HashSet::new();
    data.techs.retain(|tech| {
        crate::data::TECHS.iter().any(|known| known.id == tech) && seen_techs.insert(tech.clone())
    });
    sanitize_ship_save(&mut data.player_ship);
    let cargo_slots = crate::data::ship_class_by_key(&data.player_ship.cls).slots;
    data.player_ship.inv =
        crate::inventory::Inventory::from_slots_with_capacity(data.player_ship.inv, cargo_slots)
            .slots;
    data.ship_garage.truncate(64);
    for ship in &mut data.ship_garage {
        sanitize_ship_save(ship);
        let slots = crate::data::ship_class_by_key(&ship.cls).slots;
        ship.inv = crate::inventory::Inventory::from_slots_with_capacity(
            std::mem::take(&mut ship.inv),
            slots,
        )
        .slots;
    }
    data.researching = data.researching.take().and_then(|(id, progress)| {
        let tech = crate::data::TECHS.iter().find(|tech| tech.id == id)?;
        if crate::data::tech_unlocked(&data.techs, &id)
            || !crate::data::tech_requirements_met(&data.techs, tech)
        {
            return None;
        }
        let progress = if progress.is_finite() {
            progress.clamp(0.0, tech.time)
        } else {
            0.0
        };
        Some((id, progress))
    });
    data.play_time = if data.play_time.is_finite() {
        data.play_time.max(0.0)
    } else {
        0.0
    };
    Some(data)
}

fn sanitize_ship_save(ship: &mut ShipSave) {
    ship.cls = crate::data::ship_class_by_key(&ship.cls).key.to_string();
    if !crate::data::SHIP_MODEL_NAMES
        .iter()
        .any(|(model, _)| *model == ship.model)
    {
        ship.model.clear();
    }
    ship.name = ship
        .name
        .chars()
        .filter(|c| !c.is_control())
        .take(64)
        .collect();
    if ship.name.trim().is_empty() {
        ship.name = d_ship_name();
    }
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
    warp_anim: Option<&WarpAnimSave>,
    marks: &[crate::space::Mark],
    warp_lock: Option<&crate::space::WarpLock>,
    placed: &HashMap<String, i32>,
    side_quest: Option<&crate::quests::SideQuest>,
    machines: &[crate::factory::MachineSave],
    visited: &HashMap<usize, crate::space::PlanetArchive>,
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
        warp_anim: warp_anim.cloned(),
        marks: marks.to_vec(),
        warp_lock: warp_lock.cloned(),
        placed: placed.clone(),
        side_quest: side_quest.cloned(),
        machines: machines.to_vec(),
        visited: visited.clone(),
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
        None,
        &[],
        None,
        &HashMap::new(),
        None,
        &[],
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
    sanitize_mods(&mut data.mods);
    data.day_t = if data.day_t.is_finite() {
        data.day_t.rem_euclid(1.0)
    } else {
        0.0
    };
    data.galaxy_count = data.galaxy_count.clamp(1, 1024);
    data.ship_pos = data.ship_pos.and_then(|mut pos| {
        if !pos.iter().all(|value| value.is_finite()) {
            return None;
        }
        pos[0] = pos[0].clamp(-1_000_000.0, 1_000_000.0);
        pos[1] = pos[1].clamp(
            -256.0,
            crate::planet_scale::PLANET_SCALE.atmosphere_top + 256.0,
        );
        pos[2] = pos[2].clamp(-1_000_000.0, 1_000_000.0);
        Some(pos)
    });
    data.state = match data.state.as_str() {
        "atmo" => "atmo",
        "space" => "space",
        "warping" => "warping",
        _ => "planet",
    }
    .to_string();
    if data.state == "warping" {
        let valid_warp = data.warp_anim.as_mut().is_some_and(|anim| {
            let valid = anim.t.is_finite()
                && anim.yaw.is_finite()
                && anim.pitch.is_finite()
                && anim.v0.is_finite()
                && anim.seed != data.galaxy_seed;
            if valid {
                anim.t = anim
                    .t
                    .clamp(0.0, crate::space::WARP_LAUNCH + crate::space::WARP_RIDE);
                anim.pitch = anim.pitch.clamp(
                    -std::f32::consts::FRAC_PI_2,
                    std::f32::consts::FRAC_PI_2,
                );
                anim.v0 = anim.v0.clamp(0.0, 4_800.0);
            }
            valid
        });
        if !valid_warp {
            // A corrupt/incomplete warp record must not load into a mode whose
            // simulation can never advance. Fall back to ordinary space.
            data.state = "space".to_string();
            data.warp_anim = None;
        }
    } else {
        data.warp_anim = None;
    }
    sanitize_marks(&mut data.marks);
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
    data.placed = data
        .placed
        .into_iter()
        .filter(|(block, amount)| {
            crate::data::BLOCKS.iter().any(|known| known.key == block)
                && (0..=1_000_000).contains(amount)
        })
        .take(4096)
        .collect();
    data.side_quest = data.side_quest.take().filter(|quest| {
        crate::data::item_by_key(&quest.item).is_some()
            && (1..=100).contains(&quest.need)
            && (0..=1_000_000).contains(&quest.reward)
            && quest.x.unsigned_abs() <= 1_000_000
            && quest.z.unsigned_abs() <= 1_000_000
    });
    data.machines.truncate(200_000);
    sanitize_planet_map(&mut data.visited);
    sanitize_creature_records(&mut data.creatures, &mut data.creature_cells);
    data.archives = data
        .archives
        .into_iter()
        .take(256)
        .map(|(seed, mut archive)| {
            sanitize_galaxy_archive(&mut archive);
            (seed, archive)
        })
        .collect();
    if let Some(lock) = &mut data.warp_lock {
        lock.name = lock.name.chars().take(128).collect();
    }
    Some(data)
}

fn sanitize_mods(mods: &mut HashMap<String, Vec<u16>>) {
    let max_pairs = crate::data::CHUNK as usize
        * crate::data::CHUNK as usize
        * crate::data::WORLD_H as usize
        * 2;
    mods.retain(|key, pairs| {
        let mut parts = key.split(',');
        let coords = parts
            .next()
            .and_then(|x| x.parse::<i32>().ok())
            .zip(parts.next().and_then(|z| z.parse::<i32>().ok()));
        coords.is_some_and(|(x, z)| {
            parts.next().is_none()
                && x.unsigned_abs() <= 1_000_000
                && z.unsigned_abs() <= 1_000_000
                && pairs.len() <= max_pairs
        })
    });
}

fn sanitize_marks(marks: &mut Vec<crate::space::Mark>) {
    marks.truncate(4096);
    marks.retain_mut(|mark| {
        mark.label = mark.label.chars().take(128).collect();
        mark.x.unsigned_abs() <= 1_000_000
            && mark.y.unsigned_abs() <= 1_000_000
            && mark.z.unsigned_abs() <= 1_000_000
    });
}

fn sanitize_creature_records(
    creatures: &mut Vec<crate::creatures::HerdSave>,
    cells: &mut Vec<crate::creatures::CellSave>,
) {
    creatures.truncate(100_000);
    creatures.retain_mut(|herd| {
        let valid = herd.cand < u32::BITS as usize
            && herd.cx.unsigned_abs() <= 1_000_000
            && herd.cz.unsigned_abs() <= 1_000_000
            && [herd.x, herd.z, herd.hp, herd.home_x, herd.home_z]
                .iter()
                .all(|value| value.is_finite());
        if valid {
            herd.x = herd.x.clamp(-1_000_000.0, 1_000_000.0);
            herd.z = herd.z.clamp(-1_000_000.0, 1_000_000.0);
            herd.home_x = herd.home_x.clamp(-1_000_000.0, 1_000_000.0);
            herd.home_z = herd.home_z.clamp(-1_000_000.0, 1_000_000.0);
            herd.hp = herd.hp.clamp(-1_000.0, 1_000.0);
        }
        valid
    });
    cells.truncate(100_000);
    cells.retain(|cell| {
        cell.cx.unsigned_abs() <= 1_000_000 && cell.cz.unsigned_abs() <= 1_000_000
    });
}

fn sanitize_planet_archive(archive: &mut crate::space::PlanetArchive) {
    if !archive.ship_pos.iter().all(|value| value.is_finite()) {
        archive.ship_pos = [96.0, 40.0, 96.0];
    }
    for value in &mut archive.ship_pos {
        *value = value.clamp(-1_000_000.0, 1_000_000.0);
    }
    archive.biome = crate::data::biome_by_key(&archive.biome).key.to_string();
    sanitize_mods(&mut archive.mods);
    archive.machines.truncate(200_000);
    sanitize_marks(&mut archive.marks);
    sanitize_creature_records(&mut archive.creatures, &mut archive.creature_cells);
}

fn sanitize_galaxy_archive(archive: &mut crate::space::GalaxyArchive) {
    sanitize_planet_map(&mut archive.planets);
    archive.marks = std::mem::take(&mut archive.marks)
        .into_iter()
        .filter(|(planet, _)| *planet < 64)
        .take(64)
        .map(|(planet, mut marks)| {
            sanitize_marks(&mut marks);
            (planet, marks)
        })
        .collect();
    archive.market = std::mem::take(&mut archive.market)
        .into_iter()
        .filter(|(_, value)| value.is_finite() && *value >= 0.0 && *value <= 1_000_000.0)
        .take(4096)
        .collect();
    archive.stock = std::mem::take(&mut archive.stock)
        .into_iter()
        .filter(|(item, amount)| {
            crate::data::item_by_key(item).is_some() && (0..=100_000).contains(amount)
        })
        .take(4096)
        .collect();
}

fn sanitize_planet_map(planets: &mut HashMap<usize, crate::space::PlanetArchive>) {
    *planets = std::mem::take(planets)
        .into_iter()
        .filter(|(planet, _)| *planet < 64)
        .take(64)
        .map(|(planet, mut saved)| {
            sanitize_planet_archive(&mut saved);
            (planet, saved)
        })
        .collect();
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

fn write_bytes(path: &PathBuf, bytes: &[u8]) -> bool {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
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
        file.write_all(bytes)?;
        file.sync_all()?;
        match std::fs::rename(&tmp, path) {
            Ok(()) => Ok(()),
            Err(_) if path.exists() => {
                // Windows does not atomically replace an existing file with
                // rename. Keep the previous save recoverable until the new
                // file has reached its final name.
                let backup = parent.join(format!(".{name}.bak"));
                if backup.exists() {
                    std::fs::remove_file(&backup)?;
                }
                std::fs::rename(path, &backup)?;
                match std::fs::rename(&tmp, path) {
                    Ok(()) => {
                        let _ = std::fs::remove_file(&backup);
                        Ok(())
                    }
                    Err(error) => {
                        let _ = std::fs::rename(&backup, path);
                        Err(error)
                    }
                }
            }
            Err(e) => Err(e),
        }
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.is_ok()
}

fn write_json<T: Serialize>(path: &PathBuf, data: &T) -> bool {
    serde_json::to_vec_pretty(data)
        .ok()
        .is_some_and(|json| write_bytes(path, &json))
}

/// Previous character-file contents used to roll back a half-completed
/// character/world save pair.
pub enum SaveFileSnapshot {
    Missing,
    Bytes(Vec<u8>),
}

pub fn snapshot_char_file(name: &str) -> Option<SaveFileSnapshot> {
    const MAX_SAVE_BYTES: u64 = 64 * 1024 * 1024;
    let path = char_path(name);
    match std::fs::metadata(&path) {
        Ok(meta) if meta.len() <= MAX_SAVE_BYTES => {
            std::fs::read(path).ok().map(SaveFileSnapshot::Bytes)
        }
        Ok(_) => None,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Some(SaveFileSnapshot::Missing)
        }
        Err(_) => None,
    }
}

pub fn restore_char_file(name: &str, snapshot: &SaveFileSnapshot) -> bool {
    let path = char_path(name);
    match snapshot {
        SaveFileSnapshot::Missing => !path.exists() || std::fs::remove_file(path).is_ok(),
        SaveFileSnapshot::Bytes(bytes) => write_bytes(&path, bytes),
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &PathBuf) -> Option<T> {
    const MAX_SAVE_BYTES: u64 = 64 * 1024 * 1024;
    let read = |candidate: &PathBuf| {
        if std::fs::metadata(candidate).ok()?.len() > MAX_SAVE_BYTES {
            return None;
        }
        let bytes = std::fs::read(candidate).ok()?;
        serde_json::from_slice(&bytes).ok()
    };
    read(path).or_else(|| {
        let parent = path.parent()?;
        let name = path.file_name()?.to_str()?;
        read(&parent.join(format!(".{name}.bak")))
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_ship_state_keeps_optional_health_compatible() {
        let state: ShipStateSave = serde_json::from_str(
            r#"{"pos":[1.0,2.0,3.0],"yaw":0.1,"pitch":0.2,"roll":0.3,"speed":4.0}"#,
        )
        .unwrap();
        assert_eq!(state.hp, None);
    }

    #[test]
    fn ship_save_sanitizer_canonicalizes_untrusted_identity() {
        let mut ship = ShipSave {
            model: "../../unknown".into(),
            cls: "unknown".into(),
            name: format!("{}\0", "x".repeat(100)),
            inv: Vec::new(),
        };
        sanitize_ship_save(&mut ship);
        assert_eq!(ship.cls, crate::data::SHIP_CLASSES[0].key);
        assert!(ship.model.is_empty());
        assert_eq!(ship.name.chars().count(), 64);
        assert!(!ship.name.chars().any(char::is_control));
    }

    #[test]
    fn archive_sanitizer_bounds_nested_untrusted_state() {
        let mut archive = crate::space::GalaxyArchive::default();
        archive.planets.insert(
            0,
            crate::space::PlanetArchive {
                seed: 7,
                biome: "not-a-biome".into(),
                ship_pos: [f32::NAN, 0.0, 0.0],
                machines: Vec::new(),
                mods: HashMap::from([
                    ("0,0".into(), vec![1, 1]),
                    ("0,0,extra".into(), vec![1, 1]),
                ]),
                marks: vec![crate::space::Mark {
                    x: i32::MIN,
                    y: 0,
                    z: 0,
                    label: "x".repeat(256),
                    gal: false,
                }],
                creatures: Vec::new(),
                creature_cells: Vec::new(),
            },
        );
        sanitize_galaxy_archive(&mut archive);
        let planet = &archive.planets[&0];
        assert!(planet.ship_pos.iter().all(|value| value.is_finite()));
        assert_eq!(planet.biome, crate::data::BIOMES[0].key);
        assert_eq!(planet.mods.len(), 1);
        assert!(planet.marks.is_empty());
    }
}
