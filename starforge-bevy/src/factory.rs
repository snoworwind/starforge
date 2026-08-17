//! Factory machines — full power grid + functional machines.
//! Port of js/factory.js: furnace/chest + miner/belt/assembler/refinery/solar/wind/
//! burner/reactor/medbay/collector/lumberbot/beacon/launchpad.

use crate::data::{self, ids};
use crate::daynight;
use crate::inventory::Slot;
use crate::player::Player;
use crate::world::World as GameWorld;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const TICK: f32 = 0.1; // 工厂逻辑 tick（JS 原版同值）

/// Power draw per machine (kW) — POWER_USE in factory.js.
pub fn power_use(kind: MachineKind) -> f32 {
    match kind {
        MachineKind::Miner => 8.0,
        MachineKind::Assembler => 12.0,
        MachineKind::Refinery => 20.0,
        MachineKind::Medbay => 6.0,
        _ => 0.0,
    }
}

/// Power gen per machine (kW) — POWER_GEN in factory.js.
pub fn power_gen(kind: MachineKind) -> f32 {
    match kind {
        MachineKind::Solar => 10.0,
        MachineKind::Reactor => 100.0,
        MachineKind::Burner => 25.0,
        _ => 0.0,
    }
}

#[derive(Component, Clone, Debug)]
pub struct Machine {
    pub pos: [i32; 3],
    pub kind: MachineKind,
    pub dir: u8,
    pub active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MachineKind {
    Furnace,
    Miner,
    Belt,
    Assembler,
    Solar,
    Refinery,
    Chest,
    Reactor,
    Launchpad,
    Wind,
    Burner,
    Beacon,
    Lumberbot,
    Collector,
    Medbay,
    Other,
}

impl MachineKind {
    pub fn from_block_key(key: &str) -> Self {
        match key {
            "furnace" => Self::Furnace,
            "miner" => Self::Miner,
            "belt" => Self::Belt,
            "assembler" => Self::Assembler,
            "solar" => Self::Solar,
            "refinery" => Self::Refinery,
            "chest" => Self::Chest,
            "reactor" => Self::Reactor,
            "launchpad" => Self::Launchpad,
            "wind" => Self::Wind,
            "burner" => Self::Burner,
            "beacon" => Self::Beacon,
            "lumberbot" => Self::Lumberbot,
            "collector" => Self::Collector,
            "medbay" => Self::Medbay,
            _ => Self::Other,
        }
    }

    pub fn block_key(&self) -> &'static str {
        match self {
            Self::Furnace => "furnace",
            Self::Miner => "miner",
            Self::Belt => "belt",
            Self::Assembler => "assembler",
            Self::Solar => "solar",
            Self::Refinery => "refinery",
            Self::Chest => "chest",
            Self::Reactor => "reactor",
            Self::Launchpad => "launchpad",
            Self::Wind => "wind",
            Self::Burner => "burner",
            Self::Beacon => "beacon",
            Self::Lumberbot => "lumberbot",
            Self::Collector => "collector",
            Self::Medbay => "medbay",
            Self::Other => "stone",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Furnace => "熔炉",
            Self::Miner => "自动采矿机",
            Self::Belt => "传送带",
            Self::Assembler => "装配机",
            Self::Solar => "太阳能板",
            Self::Refinery => "精炼厂",
            Self::Chest => "储物箱",
            Self::Reactor => "核子反应堆",
            Self::Launchpad => "发射平台",
            Self::Wind => "风力涡轮机",
            Self::Burner => "火力发电机",
            Self::Beacon => "标记方块",
            Self::Lumberbot => "伐木机器人",
            Self::Collector => "收集点",
            Self::Medbay => "医疗站",
            Self::Other => "机器",
        }
    }
}

// ---------- machine states ----------

#[derive(Clone, Debug, Default)]
pub struct FurnaceState {
    pub input: Option<Slot>,
    pub fuel: Option<Slot>,
    pub output: Option<Slot>,
    pub prog: f32,
    pub burn: f32,
    pub burn_max: f32,
    pub recipe: Option<&'static str>,
    pub on: bool,
}

#[derive(Clone, Debug, Default)]
pub struct MinerState {
    pub output: Option<Slot>,
    pub prog: f32,
    pub deposit: i32,
}

pub const BELT_SPEED: f32 = 1.2;
pub const BELT_GAP: f32 = 0.28;

#[derive(Clone, Debug)]
pub struct BeltItem {
    pub item: String,
    pub t: f32,
}

#[derive(Clone, Debug, Default)]
pub struct BeltState {
    /// (item, t) — t 0..1 progress along the belt.
    pub items: Vec<BeltItem>,
}

#[derive(Clone, Debug, Default)]
pub struct CrafterState {
    pub recipe: Option<&'static str>,
    pub input: HashMap<String, i32>,
    pub output: Option<Slot>,
    pub prog: f32,
}

#[derive(Clone, Debug, Default)]
pub struct ChestState {
    pub slots: Vec<Option<Slot>>, // 24
}

#[derive(Clone, Debug, Default)]
pub struct ReactorState {
    pub fuel: f32, // burn-seconds remaining
}

#[derive(Clone, Debug, Default)]
pub struct BurnerState {
    pub fuel: Option<Slot>,
    pub burn: f32,
    pub burn_max: f32,
}

#[derive(Clone, Debug)]
pub struct BeaconState {
    pub label: String,
    pub gal: bool,
}

impl Default for BeaconState {
    fn default() -> Self {
        Self { label: "标记点".into(), gal: false }
    }
}

#[derive(Clone, Debug, Default)]
pub struct LumberbotState {
    pub cargo: i32,
    pub mine_prog: f32,
    pub deliver_t: f32,
}

#[derive(Clone, Debug, Default)]
pub struct CollectorState {
    pub slots: Vec<Option<Slot>>, // 12
}

#[derive(Clone, Debug, Default)]
pub struct MedbayState {
    pub heal_acc: f32,
}

#[derive(Component, Clone, Debug)]
pub enum MachineState {
    Furnace(FurnaceState),
    Miner(MinerState),
    Belt(BeltState),
    Crafter(CrafterState),
    Chest(ChestState),
    Reactor(ReactorState),
    Burner(BurnerState),
    Beacon(BeaconState),
    Lumberbot(LumberbotState),
    Collector(CollectorState),
    Medbay(MedbayState),
    Plain,
}

/// 电网统计（每 tick 更新，HUD 显示 gen/use）。
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct Power {
    pub generation: f32,
    pub used: f32,
    pub sat: f32,
}

pub const DIRS: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];

impl MachineState {
    pub fn for_kind(kind: MachineKind) -> Self {
        match kind {
            MachineKind::Furnace => Self::Furnace(FurnaceState::default()),
            MachineKind::Miner => Self::Miner(MinerState::default()),
            MachineKind::Belt => Self::Belt(BeltState::default()),
            MachineKind::Assembler | MachineKind::Refinery => Self::Crafter(CrafterState::default()),
            MachineKind::Chest => Self::Chest(ChestState { slots: vec![None; 24] }),
            MachineKind::Reactor => Self::Reactor(ReactorState::default()),
            MachineKind::Burner => Self::Burner(BurnerState::default()),
            MachineKind::Beacon => Self::Beacon(BeaconState::default()),
            MachineKind::Lumberbot => Self::Lumberbot(LumberbotState::default()),
            MachineKind::Collector => Self::Collector(CollectorState { slots: vec![None; 12] }),
            MachineKind::Medbay => Self::Medbay(MedbayState::default()),
            _ => Self::Plain,
        }
    }
}

/// Spawn a machine entity for a placed machine block.
pub fn spawn_machine(commands: &mut Commands, pos: [i32; 3], key: &str, dir: u8) -> Entity {
    let kind = MachineKind::from_block_key(key);
    commands
        .spawn((
            Transform::from_xyz(pos[0] as f32 + 0.5, pos[1] as f32 + 0.5, pos[2] as f32 + 0.5),
            Machine { pos, kind, dir, active: false },
            MachineState::for_kind(kind),
            crate::InGame,
        ))
        .id()
}

/// What a machine block drops when broken.
pub fn machine_drop(block_key: &str) -> &'static str {
    match block_key {
        "furnace" => "furnace_b",
        "miner" => "miner_b",
        "belt" => "belt_b",
        "assembler" => "assembler_b",
        "solar" => "solar_b",
        "refinery" => "refinery_b",
        "chest" => "chest_b",
        "reactor" => "reactor_b",
        "launchpad" => "launchpad_b",
        "wind" => "wind_b",
        "burner" => "burner_b",
        "beacon" => "beacon_b",
        "lumberbot" => "lumberbot_b",
        "collector" => "collector_b",
        "medbay" => "medbay_b",
        _ => "stone",
    }
}

pub const MACHINE_BLOCK_IDS: [u8; 15] = [
    ids::FURNACE,
    ids::MINER,
    ids::BELT,
    ids::ASSEMBLER,
    ids::SOLAR,
    ids::REFINERY,
    ids::CHEST,
    ids::REACTOR,
    ids::LAUNCHPAD,
    ids::WIND,
    ids::BURNER,
    ids::BEACON,
    ids::LUMBERBOT,
    ids::COLLECTOR,
    ids::MEDBAY,
];

// ---------- serialization (per-planet archive) ----------

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MachineSave {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub dir: u8,
    #[serde(default)]
    pub data: MachineDataSave,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct MachineDataSave {
    #[serde(default)]
    pub input: Option<Slot>,
    #[serde(default)]
    pub fuel: Option<Slot>,
    /// 反应堆燃料秒数
    #[serde(default, rename = "fuelS")]
    pub fuel_s: f32,
    #[serde(default)]
    pub output: Option<Slot>,
    #[serde(default)]
    pub prog: f32,
    #[serde(default)]
    pub burn: f32,
    #[serde(default, rename = "burnMax")]
    pub burn_max: f32,
    #[serde(default)]
    pub recipe: Option<String>,
    #[serde(default)]
    pub slots: Vec<Option<Slot>>,
    /// belt items
    #[serde(default)]
    pub items: Vec<(String, f32)>,
    #[serde(default)]
    pub deposit: i32,
    #[serde(default)]
    pub cargo: i32,
    #[serde(default, rename = "healAcc")]
    pub heal_acc: f32,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub gal: bool,
}

impl MachineState {
    pub fn to_save(&self) -> MachineDataSave {
        match self {
            Self::Furnace(f) => MachineDataSave {
                input: f.input.clone(),
                fuel: f.fuel.clone(),
                output: f.output.clone(),
                prog: f.prog,
                burn: f.burn,
                burn_max: f.burn_max,
                recipe: f.recipe.map(|s| s.to_string()),
                ..default()
            },
            Self::Miner(m) => MachineDataSave {
                output: m.output.clone(),
                prog: m.prog,
                deposit: m.deposit,
                ..default()
            },
            Self::Belt(b) => MachineDataSave {
                items: b.items.iter().map(|it| (it.item.clone(), it.t)).collect(),
                ..default()
            },
            Self::Crafter(c) => MachineDataSave {
                output: c.output.clone(),
                prog: c.prog,
                recipe: c.recipe.map(|s| s.to_string()),
                ..default()
            },
            Self::Chest(c) => MachineDataSave { slots: c.slots.clone(), ..default() },
            Self::Reactor(r) => MachineDataSave { fuel_s: r.fuel, ..default() },
            Self::Burner(b) => MachineDataSave {
                fuel: b.fuel.clone(),
                burn: b.burn,
                burn_max: b.burn_max,
                ..default()
            },
            Self::Beacon(b) => MachineDataSave { label: Some(b.label.clone()), gal: b.gal, ..default() },
            Self::Lumberbot(l) => MachineDataSave { cargo: l.cargo, ..default() },
            Self::Collector(c) => MachineDataSave { slots: c.slots.clone(), ..default() },
            Self::Medbay(m) => MachineDataSave { heal_acc: m.heal_acc, ..default() },
            Self::Plain => MachineDataSave::default(),
        }
    }

    pub fn from_save(kind: MachineKind, d: &MachineDataSave) -> Self {
        match kind {
            MachineKind::Furnace => Self::Furnace(FurnaceState {
                input: d.input.clone(),
                fuel: d.fuel.clone(),
                output: d.output.clone(),
                prog: d.prog,
                burn: d.burn,
                burn_max: d.burn_max,
                recipe: None, // 配方由输入物品自动匹配恢复
                on: false,
            }),
            MachineKind::Miner => Self::Miner(MinerState {
                output: d.output.clone(),
                prog: d.prog,
                deposit: d.deposit,
            }),
            MachineKind::Belt => Self::Belt(BeltState {
                items: d.items.iter().map(|(i, t)| BeltItem { item: i.clone(), t: *t }).collect(),
            }),
            MachineKind::Assembler | MachineKind::Refinery => Self::Crafter(CrafterState {
                // 配方在切换/选择时重新设置（存档中不持久化 &'static 引用）
                recipe: None,
                input: HashMap::new(),
                output: d.output.clone(),
                prog: d.prog,
            }),
            MachineKind::Chest => Self::Chest(ChestState {
                slots: if d.slots.is_empty() { vec![None; 24] } else { d.slots.clone() },
            }),
            MachineKind::Reactor => Self::Reactor(ReactorState { fuel: d.fuel_s }),
            MachineKind::Burner => Self::Burner(BurnerState {
                fuel: d.fuel.clone(),
                burn: d.burn,
                burn_max: d.burn_max,
            }),
            MachineKind::Beacon => Self::Beacon(BeaconState {
                label: d.label.clone().unwrap_or_else(|| "标记点".into()),
                gal: d.gal,
            }),
            MachineKind::Lumberbot => Self::Lumberbot(LumberbotState { cargo: d.cargo, ..default() }),
            MachineKind::Collector => Self::Collector(CollectorState {
                slots: if d.slots.is_empty() { vec![None; 12] } else { d.slots.clone() },
            }),
            MachineKind::Medbay => Self::Medbay(MedbayState { heal_acc: d.heal_acc }),
            _ => Self::Plain,
        }
    }
}

// ---------- insert / accept logic (canMachineAccept / machineInsert) ----------

fn belt_can_accept(b: &BeltState, t_start: f32) -> bool {
    !b.items.iter().any(|it| (it.t - t_start).abs() < BELT_GAP)
}

fn belt_insert(b: &mut BeltState, item: &str, t_start: f32) -> bool {
    if !belt_can_accept(b, t_start) {
        return false;
    }
    b.items.push(BeltItem { item: item.to_string(), t: t_start });
    true
}

pub fn can_machine_accept(m: &Machine, item: &str, state: &MachineState) -> bool {
    match state {
        MachineState::Furnace(f) => {
            if data::fuel_value(item) > 0.0
                && (f.fuel.is_none()
                    || (f.fuel.as_ref().map(|s| s.item.as_str()) == Some(item)
                        && f.fuel.as_ref().map(|s| s.n).unwrap_or(0) < 50))
            {
                return true;
            }
            let r = data::RECIPES
                .iter()
                .find(|r| r.station == "furnace" && r.inputs.iter().any(|(i, _)| *i == item));
            if r.is_none() {
                return false;
            }
            if f.input.as_ref().map(|s| s.item.as_str()) != Some(item) && f.input.is_some() {
                return false;
            }
            f.input.as_ref().map(|s| s.n).unwrap_or(0) < 50
        }
        MachineState::Chest(c) => slots_accept(&c.slots, item),
        MachineState::Collector(c) => slots_accept(&c.slots, item),
        MachineState::Crafter(c) => {
            let Some(rid) = c.recipe else { return false };
            let Some(r) = data::RECIPES.iter().find(|r| r.id == rid) else { return false };
            if !r.inputs.iter().any(|(i, _)| *i == item) {
                return false;
            }
            let need = r.inputs.iter().find(|(i, _)| *i == item).map(|(_, n)| *n).unwrap_or(1);
            c.input.get(item).copied().unwrap_or(0) < need * 3
        }
        MachineState::Belt(b) => belt_can_accept(b, 0.0),
        MachineState::Reactor(r) => item == "uranium" && r.fuel < 300.0,
        MachineState::Burner(b) => {
            data::fuel_value(item) > 0.0
                && (b.fuel.is_none()
                    || (b.fuel.as_ref().map(|s| s.item.as_str()) == Some(item)
                        && b.fuel.as_ref().map(|s| s.n).unwrap_or(0) < 50))
        }
        _ => false,
    }
}

fn slots_accept(slots: &[Option<Slot>], item: &str) -> bool {
    slots.iter().any(|s| {
        s.is_none()
            || (s.as_ref().map(|s| s.item.as_str()) == Some(item)
                && s.as_ref().map(|s| s.n).unwrap_or(0)
                    < data::item_by_key(item).map(|i| i.stack).unwrap_or(250))
    })
}

/// Try to insert 1 unit of `item`. Returns true on success.
pub fn machine_insert(m: &Machine, state: &mut MachineState, item: &str) -> bool {
    if !can_machine_accept(m, item, state) {
        return false;
    }
    match state {
        MachineState::Furnace(f) => {
            let is_fuel = data::fuel_value(item) > 0.0;
            let recipe_match = data::RECIPES
                .iter()
                .any(|r| r.station == "furnace" && r.inputs.iter().any(|(i, _)| *i == item));
            if is_fuel
                && (!(f.input.as_ref().map(|s| s.item.as_str()) == Some(item))
                    || (f.fuel.as_ref().map(|s| s.n).unwrap_or(0) < 8))
            {
                match &mut f.fuel {
                    Some(s) if s.item == item => s.n += 1,
                    None => f.fuel = Some(Slot { item: item.to_string(), n: 1 }),
                    _ => return false,
                }
                return true;
            }
            if recipe_match {
                match &mut f.input {
                    Some(s) if s.item == item => s.n += 1,
                    None => f.input = Some(Slot { item: item.to_string(), n: 1 }),
                    _ => return false,
                }
                return true;
            }
            match &mut f.fuel {
                Some(s) if s.item == item => s.n += 1,
                None => f.fuel = Some(Slot { item: item.to_string(), n: 1 }),
                _ => return false,
            }
            true
        }
        MachineState::Chest(c) => insert_into_slots(&mut c.slots, item),
        MachineState::Collector(c) => insert_into_slots(&mut c.slots, item),
        MachineState::Crafter(c) => {
            *c.input.entry(item.to_string()).or_insert(0) += 1;
            true
        }
        MachineState::Belt(b) => belt_insert(b, item, 0.0),
        MachineState::Reactor(r) => {
            r.fuel += 60.0;
            true
        }
        MachineState::Burner(b) => {
            match &mut b.fuel {
                Some(s) if s.item == item => s.n += 1,
                None => b.fuel = Some(Slot { item: item.to_string(), n: 1 }),
                _ => return false,
            }
            true
        }
        _ => false,
    }
}

fn insert_into_slots(slots: &mut [Option<Slot>], item: &str) -> bool {
    let stack = data::item_by_key(item).map(|i| i.stack).unwrap_or(250);
    for s in slots.iter_mut() {
        if let Some(sl) = s {
            if sl.item == item && sl.n < stack {
                sl.n += 1;
                return true;
            }
        }
    }
    for s in slots.iter_mut() {
        if s.is_none() {
            *s = Some(Slot { item: item.to_string(), n: 1 });
            return true;
        }
    }
    false
}

// ---------- output routing (tryOutput) ----------

/// 快照环境：机器信息 + 状态（可在逻辑阶段自由互改）。
struct Snapshot {
    machines: HashMap<Entity, Machine>,
    states: HashMap<Entity, MachineState>,
    pos_index: HashMap<[i32; 3], Entity>,
}

impl Snapshot {
    fn new(q: &Query<(Entity, &mut Machine, &mut MachineState)>) -> Self {
        let mut s = Self {
            machines: HashMap::new(),
            states: HashMap::new(),
            pos_index: HashMap::new(),
        };
        for (e, m, st) in q.iter() {
            s.machines.insert(e, m.clone());
            s.states.insert(e, st.clone());
            s.pos_index.insert(m.pos, e);
        }
        s
    }
}

/// 装配/精炼：该面是否是输入面（皮带终点指向机器所在格——简化：邻格有皮带）。
fn is_input_face(m: &Machine, d: u8, snap: &Snapshot) -> bool {
    let (dx, dz) = DIRS[d as usize];
    let bp = [m.pos[0] + dx, m.pos[1], m.pos[2] + dz];
    let b1 = snap.pos_index.get(&bp);
    let b2 = snap.pos_index.get(&[bp[0], bp[1] - 1, bp[2]]);
    b1.map(|e| {
        snap.machines
            .get(e)
            .map(|mm| mm.kind == MachineKind::Belt)
            .unwrap_or(false)
    })
    .unwrap_or(false)
        || b2
            .map(|e| {
                snap.machines
                    .get(e)
                    .map(|mm| mm.kind == MachineKind::Belt)
                    .unwrap_or(false)
            })
            .unwrap_or(false)
}

/// 机器输出一个物品：输出面皮带/机器优先，其余面仅入机器。
fn try_output(
    m: &Machine,
    item: &str,
    snap: &mut Snapshot,
) -> bool {
    let crafter = matches!(m.kind, MachineKind::Assembler | MachineKind::Refinery);
    let (fdx, fdz) = DIRS[m.dir as usize];
    let mut targets: Vec<Entity> = Vec::new();
    let front = [m.pos[0] + fdx, m.pos[1], m.pos[2] + fdz];
    if let Some(e) = snap.pos_index.get(&front) {
        if !(crafter && is_input_face(m, m.dir, snap)) {
            targets.push(*e);
        }
    }
    for (d, (dx, dz)) in DIRS.iter().enumerate() {
        if d == m.dir as usize {
            continue;
        }
        if crafter && is_input_face(m, d as u8, snap) {
            continue;
        }
        let tp = [m.pos[0] + dx, m.pos[1], m.pos[2] + dz];
        if let Some(e) = snap.pos_index.get(&tp) {
            targets.push(*e);
        }
    }
    for t in targets {
        let Some(tm) = snap.machines.get(&t).cloned() else { continue };
        let Some(ts) = snap.states.get_mut(&t) else { continue };
        if matches!(tm.kind, MachineKind::Belt) {
            if crafter {
                continue; // 装配/精炼不从侧面输出到皮带
            }
            if let MachineState::Belt(bs) = ts {
                if belt_insert(bs, item, 0.1) {
                    return true;
                }
            }
        } else if can_machine_accept(&tm, item, ts) && machine_insert(&tm, ts, item) {
            return true;
        }
    }
    // 无处可送：产出留在机器内
    false
}

// ---------- per-tick logic ----------

fn furnace_tick(m: &mut Machine, f: &mut FurnaceState, snap: &mut Snapshot, drops: &mut Vec<(String, i32)>, commands: &mut Commands, sfx: &crate::audio::Sfx) {
    let input_item = f.input.as_ref().map(|s| s.item.clone());
    let r = input_item.as_deref().and_then(|it| {
        data::RECIPES
            .iter()
            .find(|r| r.station == "furnace" && r.inputs.iter().any(|(i, _)| *i == it))
    });
    let need = r.and_then(|r| {
        input_item
            .as_deref()
            .and_then(|it| r.inputs.iter().find(|(i, _)| *i == it).map(|(_, n)| *n))
    });
    let can_work = r.is_some() && f.input.as_ref().map(|s| s.n).unwrap_or(0) >= need.unwrap_or(1);
    if f.burn <= 0.0 && can_work {
        if let Some(fuel) = f.fuel.as_mut() {
            if fuel.n > 0 {
                f.burn = data::fuel_value(&fuel.item).max(4.0);
                f.burn_max = f.burn;
                fuel.n -= 1;
                if fuel.n <= 0 {
                    f.fuel = None;
                }
            }
        }
    }
    m.active = false;
    f.on = f.burn > 0.0 && can_work;
    if f.on {
        if let (Some(r), Some(nn)) = (r, need) {
            f.burn -= TICK;
            f.prog += TICK / r.time;
            if f.prog >= 1.0 {
                f.prog = 0.0;
                if let Some(inp) = f.input.as_mut() {
                    inp.n -= nn;
                    if inp.n <= 0 {
                        f.input = None;
                    }
                }
                let out_item = r.output.0;
                if let Some(o) = f.output.as_mut() {
                    if o.item != out_item {
                        drops.push((o.item.clone(), o.n));
                        f.output = None;
                    }
                }
                if f.output.is_none() {
                    f.output = Some(Slot { item: out_item.to_string(), n: 0 });
                }
                if let Some(o) = f.output.as_mut() {
                    o.n += r.output.1;
                }
                crate::audio::play(commands, sfx.craft.clone(), 0.4, None);
            }
        }
    } else {
        f.prog = (f.prog - TICK * 0.3).max(0.0);
    }
    if let Some(o) = f.output.clone() {
        if o.n > 0 && try_output(m, &o.item, snap) {
            if let Some(o2) = f.output.as_mut() {
                o2.n -= 1;
                if o2.n <= 0 {
                    f.output = None;
                }
            }
        }
    }
}

fn miner_tick(
    m: &mut Machine,
    s: &mut MinerState,
    sat: f32,
    world: &GameWorld,
    snap: &mut Snapshot,
    world_writes: &mut Vec<([i32; 3], u8)>,
) {
    m.active = false;
    let below = data::block_by_id(world.get(m.pos[0], m.pos[1] - 1, m.pos[2]));
    if let Some(o) = s.output.clone() {
        if o.n > 0 && try_output(m, &o.item, snap) {
            if let Some(o2) = s.output.as_mut() {
                o2.n -= 1;
                if o2.n <= 0 {
                    s.output = None;
                }
            }
        }
    }
    if !below.ore {
        return;
    }
    let eff = sat.max(0.35); // 无电 35% 手摇
    m.active = true;
    s.prog += TICK * 0.5 * eff;
    if s.prog >= 1.0 {
        s.prog = 0.0;
        let drop = below.drops.first().map(|d| d.item).unwrap_or("stone");
        if s.output.is_none() {
            s.output = Some(Slot { item: drop.to_string(), n: 0 });
        }
        if let Some(o) = s.output.as_mut() {
            if o.item != drop {
                return;
            }
            o.n += 1;
        }
        s.deposit += 1;
        if s.deposit >= 300 {
            world_writes.push(([m.pos[0], m.pos[1] - 1, m.pos[2]], ids::STONE));
            s.deposit = 0;
        }
    }
}

fn crafter_tick(
    m: &mut Machine,
    c: &mut CrafterState,
    sat: f32,
    where_: &str,
    snap: &mut Snapshot,
    drops: &mut Vec<(String, i32)>,
) {    m.active = false;
    let Some(rid) = c.recipe else { return };
    let Some(r) = data::RECIPES
        .iter()
        .find(|r| r.id == rid && (r.station == where_ || r.station == "both"))
    else {
        return;
    };
    let has_all = r.inputs.iter().all(|(i, n)| c.input.get(*i).copied().unwrap_or(0) >= *n);
    if c.prog > 0.0 || (has_all && sat > 0.05) {
        if c.prog == 0.0 {
            for (i, n) in r.inputs {
                let v = c.input.entry(i.to_string()).or_insert(0);
                *v -= n;
            }
        }
        m.active = sat > 0.05;
        c.prog += TICK / r.time * sat;
        if c.prog >= 1.0 {
            c.prog = 0.0;
            let out_item = r.output.0;
            if let Some(o) = c.output.as_mut() {
                if o.item != out_item {
                    drops.push((o.item.clone(), o.n));
                    c.output = None;
                }
            }
            if c.output.is_none() {
                c.output = Some(Slot { item: out_item.to_string(), n: 0 });
            }
            if let Some(o) = c.output.as_mut() {
                o.n += r.output.1;
            }
        }
    }
    if let Some(o) = c.output.clone() {
        if o.n > 0 && try_output(m, &o.item, snap) {
            if let Some(o2) = c.output.as_mut() {
                o2.n -= 1;
                if o2.n <= 0 {
                    c.output = None;
                }
            }
        }
    }
}

fn belt_tick(m: &mut Machine, b: &mut BeltState, snap: &mut Snapshot) {
    m.active = !b.items.is_empty();
    b.items.sort_by(|a, c| c.t.partial_cmp(&a.t).unwrap());
    let mut i = 0;
    while i < b.items.len() {
        let max_t = if i == 0 { 1.0 } else { (b.items[i - 1].t - BELT_GAP).max(0.0) };
        let t = b.items[i].t;
        b.items[i].t = (t + BELT_SPEED * TICK).min(max_t.max(t));
        if b.items[i].t >= 0.999 {
            let (dx, dz) = DIRS[m.dir as usize];
            let item = b.items[i].item.clone();
            let nx = m.pos[0] + dx;
            let nz = m.pos[2] + dz;
            let mut moved = false;
            for y in [m.pos[1], m.pos[1] - 1, m.pos[1] + 1] {
                let tp = [nx, y, nz];
                let Some(te) = snap.pos_index.get(&tp).copied() else { continue };
                let Some(tm) = snap.machines.get(&te).cloned() else { continue };
                let Some(ts) = snap.states.get_mut(&te) else { continue };
                if matches!(tm.kind, MachineKind::Belt) {
                    if let MachineState::Belt(bs) = ts {
                        if belt_insert(bs, &item, 0.0) {
                            moved = true;
                            break;
                        }
                    }
                } else {
                    // 禁止向正面输入侧的装配/精炼推入（belt→machine 回流防护）
                    let blocked = matches!(tm.kind, MachineKind::Assembler | MachineKind::Refinery)
                        && tm.dir == m.dir
                        && (m.pos[0] + dx == tm.pos[0] && m.pos[2] + dz == tm.pos[2]);
                    if !blocked && can_machine_accept(&tm, &item, ts) && machine_insert(&tm, ts, &item) {
                        moved = true;
                        break;
                    }
                }
            }
            if moved {
                b.items.remove(i);
                continue;
            }
        }
        i += 1;
    }
}

fn collector_tick(m: &mut Machine, c: &mut CollectorState, snap: &mut Snapshot) {
    m.active = c.slots.iter().any(|s| s.is_some());
    for i in 0..c.slots.len() {
        let Some(s) = c.slots[i].clone() else { continue };
        if s.n > 0 && try_output(m, &s.item, snap) {
            if let Some(sl) = c.slots[i].as_mut() {
                sl.n -= 1;
            }
            if c.slots[i].as_ref().map(|sl| sl.n).unwrap_or(0) <= 0 {
                c.slots[i] = None;
            }
        }
        break; // 每 tick 最多输出 1
    }
}

/// 医疗站需求（站近 + 有钠氧 + 生命未满）
fn medbay_wants(m: &Machine, p: &Player) -> bool {
    if p.dead || p.stats.hp >= 8.0 {
        return false;
    }
    let dx = p.pos.x - (m.pos[0] as f32 + 0.5);
    let dz = p.pos.z - (m.pos[2] as f32 + 0.5);
    if dx * dx + dz * dz > 16.0 {
        return false;
    }
    p.inv.count_item("sodium") >= 1 && p.inv.count_item("oxygen") >= 1
}

// ---------- main factory system ----------

#[derive(Resource, Default)]
pub struct TickAcc(pub f32);

/// 每帧入口：0.1s 累加器驱动 tick，更新电网统计。
#[allow(clippy::too_many_arguments)]
pub fn factory_system(
    time: Res<Time>,
    mut acc: ResMut<TickAcc>,
    mut q: Query<(Entity, &mut Machine, &mut MachineState)>,
    mut power: ResMut<Power>,
    day: Res<daynight::DayTime>,
    world: Option<ResMut<GameWorld>>,
    mut player: Option<Query<&mut Player>>,
    icons: Res<crate::ui::IconMaterials>,
    mut commands: Commands,
    sfx: Res<crate::audio::Sfx>,
) {
    acc.0 += time.delta_secs();
    if acc.0 < TICK {
        return;
    }
    acc.0 = 0.0;

    // 阶段 A：快照
    let mut snap = Snapshot::new(&q);
    let day_f = daynight::day_factor(day.0);
    let mut gen_total = 0.0;
    let mut used = 0.0;
    let wind_t = time.elapsed_secs();
    let Some(mut world) = world else { return };
    {
        let w: &GameWorld = &world;
        for (e, mut m, mut st) in q.iter_mut() {
            match &mut *st {
                MachineState::Plain => {
                    if m.kind == MachineKind::Solar {
                        gen_total += power_gen(MachineKind::Solar) * day_f.max(0.0);
                    }
                    if m.kind == MachineKind::Wind {
                        let alt = (m.pos[1] as f32 - data::SEA_Y).max(0.0) * 0.18;
                        let gust = (wind_t * 0.5 + m.pos[0] as f32 * 0.7 + m.pos[2] as f32 * 1.3).sin() * 3.0
                            + (wind_t * 0.13).sin() * 2.0;
                        gen_total += (6.0 + alt + gust).clamp(2.0, 16.0);
                        m.active = true;
                    }
                }
                MachineState::Burner(b) => {
                    if b.burn <= 0.0 {
                        if let Some(fuel) = b.fuel.as_mut() {
                            if fuel.n > 0 {
                                b.burn = data::fuel_value(&fuel.item).max(4.0) * 1.5;
                                b.burn_max = b.burn;
                                fuel.n -= 1;
                                if fuel.n <= 0 {
                                    b.fuel = None;
                                }
                            }
                        }
                    }
                    if b.burn > 0.0 {
                        b.burn -= TICK;
                        gen_total += power_gen(MachineKind::Burner);
                        m.active = true;
                    } else {
                        m.active = false;
                    }
                }
                MachineState::Reactor(r) => {
                    if r.fuel > 0.0 {
                        gen_total += power_gen(MachineKind::Reactor);
                        r.fuel -= TICK;
                        m.active = true;
                    } else {
                        m.active = false;
                    }
                }
                MachineState::Miner(_) => {
                    if data::block_by_id(w.get(m.pos[0], m.pos[1] - 1, m.pos[2])).ore {
                        used += power_use(MachineKind::Miner);
                    }
                }
                MachineState::Crafter(c) => {
                    if let Some(rid) = c.recipe {
                        if let Some(r) = data::RECIPES.iter().find(|r| r.id == rid) {
                            if c.prog > 0.0
                                || r.inputs
                                    .iter()
                                    .all(|(i, n)| c.input.get(*i).copied().unwrap_or(0) >= *n)
                            {
                                used += power_use(m.kind);
                            }
                        }
                    }
                }
                MachineState::Medbay(_) => {
                    if let Some(pq) = player.as_ref() {
                        if let Ok(p) = pq.single() {
                            if medbay_wants(&m, p) {
                                used += power_use(MachineKind::Medbay);
                            }
                        }
                    }
                }
                _ => {}
            }
            // 同步快照（电力阶段的状态改动）
            snap.states.insert(e, st.clone());
            snap.machines.insert(e, m.clone());
        }
    }
    let sat = if used > 0.0 { (gen_total / used).min(1.0) } else { 1.0 };
    *power = Power { generation: gen_total.round(), used, sat };

    // 阶段 B：快照逻辑
    let mut drops: Vec<(String, i32)> = Vec::new();
    let mut world_writes: Vec<([i32; 3], u8)> = Vec::new();
    let mut order: Vec<Entity> = snap.machines.keys().copied().collect();
    order.sort();
    for e in order {
        let Some(mut m) = snap.machines.get(&e).cloned() else { continue };
        let Some(mut st) = snap.states.remove(&e) else { continue };
        match &mut st {
            MachineState::Furnace(f) => furnace_tick(&mut m, f, &mut snap, &mut drops, &mut commands, &sfx),
            MachineState::Miner(ms) => {
                miner_tick(&mut m, ms, sat, &world, &mut snap, &mut world_writes)
            }
            MachineState::Belt(bs) => belt_tick(&mut m, bs, &mut snap),
            MachineState::Crafter(cs) => {
                let where_ = if m.kind == MachineKind::Refinery { "refinery" } else { "assembler" };
                crafter_tick(&mut m, cs, sat, where_, &mut snap, &mut drops);
            }
            MachineState::Collector(cs) => collector_tick(&mut m, cs, &mut snap),
            MachineState::Medbay(ms) => {
                m.active = false;
                if let Some(pq) = player.as_mut() {
                    if let Ok(mut p) = pq.single_mut() {
                        if medbay_wants(&m, &p) && sat > 0.05 {
                            m.active = true;
                            ms.heal_acc += TICK * sat;
                            while ms.heal_acc >= 1.0 {
                                ms.heal_acc -= 1.0;
                                if p.inv.remove_item("sodium", 1) && p.inv.remove_item("oxygen", 1) {
                                    p.stats.hp = (p.stats.hp + 3.0).min(8.0);
                                    p.toast("医疗站：生命 +3");
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        snap.states.insert(e, st);
        snap.machines.insert(e, m);
    }

    // 阶段 C：回写
    for (e, mut m, mut st) in q.iter_mut() {
        if let Some(nm) = snap.machines.get(&e) {
            *m = nm.clone();
        }
        if let Some(ns) = snap.states.get(&e) {
            *st = ns.clone();
        }
    }
    // 掉落物（产出无处可送时洒在机器上方）
    for (item, n) in drops {
        if let Some(pq) = player.as_ref() {
            if let Ok(p) = pq.single() {
                crate::creatures::spawn_drop(
                    &mut commands,
                    &world,
                    &icons,
                    p.pos + Vec3::Y,
                    Vec3::ZERO,
                    item,
                    n,
                    0.4,
                );
            }
        }
    }
    // 世界改块（矿脉耗尽）
    for (pos, id) in world_writes {
        world.set(pos[0], pos[1], pos[2], id);
    }
}

/// Sync machines with world blocks: despawn machines whose block was removed.
pub fn machine_sync_system(
    world: Res<GameWorld>,
    machines: Query<(Entity, &Machine)>,
    mut commands: Commands,
) {
    for (e, m) in &machines {
        let def = data::block_by_id(world.get(m.pos[0], m.pos[1], m.pos[2]));
        if def.machine.is_none() {
            commands.entity(e).despawn();
        }
    }
}

/// 序列化全部机器（星球档案用）。
pub fn serialize_machines(q: &Query<(Entity, &Machine, &MachineState)>) -> Vec<MachineSave> {
    let mut out = Vec::new();
    for (_, m, st) in q.iter() {
        out.push(MachineSave {
            x: m.pos[0],
            y: m.pos[1],
            z: m.pos[2],
            kind: m.kind.block_key().to_string(),
            dir: m.dir,
            data: st.to_save(),
        });
    }
    out
}

/// 反序列化：为存档中的机器建实体并恢复状态（方块本体已在 chunk 数据中）。
pub fn deserialize_machines(
    commands: &mut Commands,
    saves: &[MachineSave],
) -> Vec<(Entity, MachineSave)> {
    let mut out = Vec::new();
    for s in saves {
        let kind = MachineKind::from_block_key(&s.kind);
        let e = commands
            .spawn((
                Transform::from_xyz(s.x as f32 + 0.5, s.y as f32 + 0.5, s.z as f32 + 0.5),
                Machine { pos: [s.x, s.y, s.z], kind, dir: s.dir, active: false },
                MachineState::from_save(kind, &s.data),
                crate::InGame,
            ))
            .id();
        out.push((e, s.clone()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine(kind: MachineKind, pos: [i32; 3], dir: u8) -> Machine {
        Machine { pos, kind, dir, active: false }
    }

    #[test]
    fn belt_accepts_spaced_items() {
        let mut b = BeltState::default();
        assert!(belt_insert(&mut b, "iron_ore", 0.0));
        // 间距不足（GAP 0.28）→ 拒绝
        assert!(!belt_insert(&mut b, "copper_ore", 0.1));
        // 足够间距 → 接受
        assert!(belt_insert(&mut b, "copper_ore", 0.4));
        assert_eq!(b.items.len(), 2);
    }

    #[test]
    fn furnace_insert_fuel_and_ore() {
        let m = machine(MachineKind::Furnace, [1, 2, 3], 0);
        let mut st = MachineState::for_kind(MachineKind::Furnace);
        // 先放原料
        assert!(machine_insert(&m, &mut st, "iron_ore"));
        assert!(machine_insert(&m, &mut st, "coal"));
        match &st {
            MachineState::Furnace(f) => {
                assert_eq!(f.input.as_ref().map(|s| s.item.as_str()), Some("iron_ore"));
                assert_eq!(f.fuel.as_ref().map(|s| s.item.as_str()), Some("coal"));
            }
            _ => panic!("expected furnace state"),
        }
        // 不接受的物品
        assert!(!machine_insert(&m, &mut st, "circuit"));
    }

    #[test]
    fn reactor_takes_uranium_only() {
        let m = machine(MachineKind::Reactor, [1, 2, 3], 0);
        let mut st = MachineState::for_kind(MachineKind::Reactor);
        assert!(machine_insert(&m, &mut st, "uranium"));
        match &st {
            MachineState::Reactor(r) => assert!((r.fuel - 60.0).abs() < 0.01),
            _ => panic!("expected reactor state"),
        }
        assert!(!machine_insert(&m, &mut st, "carbon"));
    }

    #[test]
    fn crafter_accepts_only_recipe_inputs() {
        let m = machine(MachineKind::Assembler, [1, 2, 3], 0);
        let mut st = MachineState::Crafter(CrafterState {
            recipe: Some("gear"), // iron×2
            ..default()
        });
        assert!(machine_insert(&m, &mut st, "iron"));
        assert!(!machine_insert(&m, &mut st, "copper"));
        match &st {
            MachineState::Crafter(c) => assert_eq!(c.input.get("iron"), Some(&1)),
            _ => panic!("expected crafter state"),
        }
    }

    #[test]
    fn machine_state_save_roundtrip() {
        for kind in [
            MachineKind::Furnace,
            MachineKind::Chest,
            MachineKind::Collector,
            MachineKind::Belt,
            MachineKind::Reactor,
            MachineKind::Burner,
            MachineKind::Beacon,
        ] {
            let st = MachineState::for_kind(kind);
            let save = st.to_save();
            let json = serde_json::to_string(&save).unwrap();
            let back: MachineDataSave = serde_json::from_str(&json).unwrap();
            let st2 = MachineState::from_save(kind, &back);
            // 往返后结构一致（槽位数量等）
            match (st, st2) {
                (MachineState::Chest(a), MachineState::Chest(b)) => assert_eq!(a.slots.len(), b.slots.len()),
                (MachineState::Collector(a), MachineState::Collector(b)) => assert_eq!(a.slots.len(), b.slots.len()),
                (MachineState::Belt(a), MachineState::Belt(b)) => assert_eq!(a.items.len(), b.items.len()),
                (MachineState::Furnace(a), MachineState::Furnace(b)) => {
                    assert_eq!(a.input.is_some(), b.input.is_some());
                    assert_eq!(a.fuel.is_some(), b.fuel.is_some());
                }
                (MachineState::Reactor(a), MachineState::Reactor(b)) => assert!((a.fuel - b.fuel).abs() < 0.01),
                (MachineState::Burner(a), MachineState::Burner(b)) => {
                    assert_eq!(a.fuel.is_some(), b.fuel.is_some());
                }
                (MachineState::Beacon(a), MachineState::Beacon(b)) => assert_eq!(a.label, b.label),
                _ => panic!("roundtrip kind mismatch"),
            }
        }
    }

    #[test]
    fn power_use_gen_table() {
        assert_eq!(power_use(MachineKind::Miner), 8.0);
        assert_eq!(power_use(MachineKind::Assembler), 12.0);
        assert_eq!(power_use(MachineKind::Refinery), 20.0);
        assert_eq!(power_gen(MachineKind::Solar), 10.0);
        assert_eq!(power_gen(MachineKind::Reactor), 100.0);
        assert_eq!(power_gen(MachineKind::Burner), 25.0);
    }
}
