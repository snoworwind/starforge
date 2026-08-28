//! Factory machines — local power grids, item/fluid logistics and colony systems.
//! Includes production, storage, generation, automation, settlement and defense
//! machines with per-planet persistence.

use crate::creatures::Creature;
use crate::data::{self, ids};
use crate::daynight;
use crate::inventory::Slot;
use crate::player::Player;
use crate::schedule::{GameSet, GameState, ground_mode};
use crate::world::World as GameWorld;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

pub const TICK: f32 = 0.1; // 工厂逻辑 tick（JS 原版同值）

/// Power draw per machine (kW) — POWER_USE in factory.js.
pub fn power_use(kind: MachineKind) -> f32 {
    match kind {
        MachineKind::Miner => 8.0,
        MachineKind::Assembler => 12.0,
        MachineKind::Refinery => 20.0,
        MachineKind::Medbay => 6.0,
        MachineKind::Pump => 2.0,
        MachineKind::ColonyCore => 15.0,
        MachineKind::Turret => 10.0,
        _ => 0.0,
    }
}

/// Power gen per machine (kW) — POWER_GEN in factory.js.
pub fn power_gen(kind: MachineKind) -> f32 {
    match kind {
        MachineKind::Solar => 10.0,
        MachineKind::Reactor => 100.0,
        MachineKind::Burner => 25.0,
        MachineKind::Geothermal => 45.0,
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
    Splitter,
    Filter,
    Cable,
    Battery,
    Pipe,
    Tank,
    Pump,
    Geothermal,
    ColonyCore,
    Turret,
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
            "splitter" => Self::Splitter,
            "filter" => Self::Filter,
            "cable" => Self::Cable,
            "battery" => Self::Battery,
            "pipe" => Self::Pipe,
            "tank" => Self::Tank,
            "pump" => Self::Pump,
            "geothermal" => Self::Geothermal,
            "colony_core" => Self::ColonyCore,
            "turret" => Self::Turret,
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
            Self::Splitter => "splitter",
            Self::Filter => "filter",
            Self::Cable => "cable",
            Self::Battery => "battery",
            Self::Pipe => "pipe",
            Self::Tank => "tank",
            Self::Pump => "pump",
            Self::Geothermal => "geothermal",
            Self::ColonyCore => "colony_core",
            Self::Turret => "turret",
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
            Self::Splitter => "智能分流器",
            Self::Filter => "筛选分流器",
            Self::Cable => "电力电缆",
            Self::Battery => "工业蓄电池",
            Self::Pipe => "流体管道",
            Self::Tank => "储液罐",
            Self::Pump => "流体泵",
            Self::Geothermal => "地热发电机",
            Self::ColonyCore => "殖民核心",
            Self::Turret => "自动防御炮塔",
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
pub struct RouterState {
    pub items: Vec<BeltItem>,
    /// 仅筛选分流器使用；匹配物走正面，其他物走右侧。
    pub filter: Option<String>,
    pub route: u8,
}

pub const BATTERY_CAPACITY: f32 = 500.0;

#[derive(Clone, Debug, Default)]
pub struct BatteryState {
    /// 储能，单位 kWs。
    pub charge: f32,
}

#[derive(Clone, Debug, Default)]
pub struct ColonyState {
    pub input: HashMap<String, i32>,
    pub output: Option<Slot>,
    /// 0..1, one full settlement production cycle.
    pub prog: f32,
    pub habitat: i32,
    pub residents: i32,
    pub scan_t: f32,
    pub cycles: i32,
}

#[derive(Clone, Debug, Default)]
pub struct TurretState {
    pub cooldown: f32,
    pub engaged: bool,
    pub kills: i32,
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
        Self {
            label: "标记点".into(),
            gal: false,
        }
    }
}

/// 伐木机器人相位（JS factory.js 状态机：scan/move/chop/deliver/wait）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BotPhase {
    #[default]
    Scan,
    Move,
    Chop,
    Deliver,
    Wait,
}

#[derive(Clone, Debug, Default)]
pub struct LumberbotState {
    pub cargo: i32,
    /// 兼容旧存档字段
    pub mine_prog: f32,
    pub deliver_t: f32,
    pub phase: BotPhase,
    /// 机器人当前位置（世界坐标）
    pub pos: [f32; 3],
    /// 当前目标（原木段 / 收集点）
    pub target: Option<[i32; 3]>,
    /// 扫描列游标
    pub scan_off: usize,
    pub chop_t: f32,
    pub wait_t: f32,
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
    Router(RouterState),
    Crafter(CrafterState),
    Chest(ChestState),
    Tank(ChestState),
    Battery(BatteryState),
    Colony(ColonyState),
    Turret(TurretState),
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

#[inline]
fn dir_index(dir: u8) -> usize {
    dir as usize % DIRS.len()
}

impl MachineState {
    pub fn for_kind(kind: MachineKind) -> Self {
        match kind {
            MachineKind::Furnace => Self::Furnace(FurnaceState::default()),
            MachineKind::Miner => Self::Miner(MinerState::default()),
            MachineKind::Belt => Self::Belt(BeltState::default()),
            MachineKind::Splitter | MachineKind::Filter => Self::Router(RouterState::default()),
            MachineKind::Pipe | MachineKind::Pump => Self::Belt(BeltState::default()),
            MachineKind::Assembler | MachineKind::Refinery => {
                Self::Crafter(CrafterState::default())
            }
            MachineKind::Chest => Self::Chest(ChestState {
                slots: vec![None; 24],
            }),
            MachineKind::Tank => Self::Tank(ChestState {
                slots: vec![None; 12],
            }),
            MachineKind::Battery => Self::Battery(BatteryState::default()),
            MachineKind::ColonyCore => Self::Colony(ColonyState::default()),
            MachineKind::Turret => Self::Turret(TurretState::default()),
            MachineKind::Reactor => Self::Reactor(ReactorState::default()),
            MachineKind::Burner => Self::Burner(BurnerState::default()),
            MachineKind::Beacon => Self::Beacon(BeaconState::default()),
            MachineKind::Lumberbot => Self::Lumberbot(LumberbotState::default()),
            MachineKind::Collector => Self::Collector(CollectorState {
                slots: vec![None; 12],
            }),
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
            Transform::from_xyz(
                pos[0] as f32 + 0.5,
                pos[1] as f32 + 0.5,
                pos[2] as f32 + 0.5,
            ),
            Machine {
                pos,
                kind,
                dir,
                active: false,
            },
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
        "splitter" => "splitter_b",
        "filter" => "filter_b",
        "cable" => "cable_b",
        "battery" => "battery_b",
        "pipe" => "pipe_b",
        "tank" => "tank_b",
        "pump" => "pump_b",
        "geothermal" => "geothermal_b",
        "colony_core" => "colony_core_b",
        "turret" => "turret_b",
        _ => "stone",
    }
}

/// 拆机内容返还（JS Factory.remove 退款：in/fuel/out/槽位/皮带/碳 cargo/铀/进行中配方原料）。
pub fn machine_refund(state: &MachineState) -> Vec<(String, i32)> {
    let mut out: Vec<(String, i32)> = Vec::new();
    let push = |out: &mut Vec<(String, i32)>, item: &str, n: i32| {
        if n > 0 {
            out.push((item.to_string(), n));
        }
    };
    match state {
        MachineState::Furnace(f) => {
            if let Some(s) = &f.input {
                push(&mut out, &s.item, s.n);
            }
            if let Some(s) = &f.fuel {
                push(&mut out, &s.item, s.n);
            }
            if let Some(s) = &f.output {
                push(&mut out, &s.item, s.n);
            }
        }
        MachineState::Miner(m) => {
            if let Some(s) = &m.output {
                push(&mut out, &s.item, s.n);
            }
        }
        MachineState::Belt(b) => {
            for it in &b.items {
                push(&mut out, &it.item, 1);
            }
        }
        MachineState::Router(r) => {
            for it in &r.items {
                push(&mut out, &it.item, 1);
            }
        }
        MachineState::Crafter(c) => {
            for (k, v) in &c.input {
                push(&mut out, k, *v);
            }
            if let Some(s) = &c.output {
                push(&mut out, &s.item, s.n);
            }
            // 进行中配方：退还一组原料（JS: prog>0 && recipe）
            if c.prog > 0.0
                && let Some(r) = c
                    .recipe
                    .and_then(|rid| data::RECIPES.iter().find(|r| r.id == rid))
            {
                for (i, n) in r.inputs {
                    push(&mut out, i, *n);
                }
            }
        }
        MachineState::Chest(c) => {
            for s in c.slots.iter().flatten() {
                push(&mut out, &s.item, s.n);
            }
        }
        MachineState::Tank(c) => {
            for s in c.slots.iter().flatten() {
                push(&mut out, &s.item, s.n);
            }
        }
        MachineState::Colony(c) => {
            for (item, amount) in &c.input {
                push(&mut out, item, *amount);
            }
            if let Some(slot) = &c.output {
                push(&mut out, &slot.item, slot.n);
            }
        }
        MachineState::Collector(c) => {
            for s in c.slots.iter().flatten() {
                push(&mut out, &s.item, s.n);
            }
        }
        MachineState::Reactor(r) => {
            // Partial fuel has already produced power and must not round back
            // into a whole uranium item when the reactor is dismantled.
            push(&mut out, "uranium", (r.fuel / 60.0).floor() as i32);
        }
        MachineState::Burner(b) => {
            if let Some(s) = &b.fuel {
                push(&mut out, &s.item, s.n);
            }
        }
        MachineState::Lumberbot(l) => {
            push(&mut out, "carbon", l.cargo);
        }
        _ => {}
    }
    out
}

pub const MACHINE_BLOCK_IDS: [u8; 25] = [
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
    ids::SPLITTER,
    ids::FILTER,
    ids::CABLE,
    ids::BATTERY,
    ids::PIPE,
    ids::TANK,
    ids::PUMP,
    ids::GEOTHERMAL,
    ids::COLONY_CORE,
    ids::TURRET,
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
    /// crafter 原料表（装配机/精炼厂）
    #[serde(default)]
    pub input_map: HashMap<String, i32>,
    /// belt items
    #[serde(default)]
    pub items: Vec<(String, f32)>,
    #[serde(default)]
    pub route: u8,
    #[serde(default)]
    pub charge: f32,
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
    /// 伐木机器人位置
    #[serde(default)]
    pub bot_pos: Option<[f32; 3]>,
    /// 殖民核心扫描到的有效舱室方块数。
    #[serde(default)]
    pub habitat: i32,
    /// 殖民核心当前可容纳的居民数。
    #[serde(default)]
    pub residents: i32,
    /// 殖民核心已完成的生产周期数。
    #[serde(default)]
    pub cycles: i32,
    /// 炮塔射击冷却。
    #[serde(default)]
    pub cooldown: f32,
    /// 炮塔累计击杀数。
    #[serde(default)]
    pub kills: i32,
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
            Self::Router(r) => MachineDataSave {
                items: r.items.iter().map(|it| (it.item.clone(), it.t)).collect(),
                label: r.filter.clone(),
                route: r.route,
                ..default()
            },
            Self::Crafter(c) => MachineDataSave {
                output: c.output.clone(),
                prog: c.prog,
                recipe: c.recipe.map(|s| s.to_string()),
                input_map: c.input.clone(),
                ..default()
            },
            Self::Chest(c) => MachineDataSave {
                slots: c.slots.clone(),
                ..default()
            },
            Self::Tank(c) => MachineDataSave {
                slots: c.slots.clone(),
                ..default()
            },
            Self::Battery(b) => MachineDataSave {
                charge: b.charge,
                ..default()
            },
            Self::Colony(c) => MachineDataSave {
                input_map: c.input.clone(),
                output: c.output.clone(),
                prog: c.prog,
                habitat: c.habitat,
                residents: c.residents,
                cycles: c.cycles,
                ..default()
            },
            Self::Turret(t) => MachineDataSave {
                cooldown: t.cooldown,
                kills: t.kills,
                ..default()
            },
            Self::Reactor(r) => MachineDataSave {
                fuel_s: r.fuel,
                ..default()
            },
            Self::Burner(b) => MachineDataSave {
                fuel: b.fuel.clone(),
                burn: b.burn,
                burn_max: b.burn_max,
                ..default()
            },
            Self::Beacon(b) => MachineDataSave {
                label: Some(b.label.clone()),
                gal: b.gal,
                ..default()
            },
            Self::Lumberbot(l) => MachineDataSave {
                cargo: l.cargo,
                bot_pos: Some(l.pos),
                ..default()
            },
            Self::Collector(c) => MachineDataSave {
                slots: c.slots.clone(),
                ..default()
            },
            Self::Medbay(m) => MachineDataSave {
                heal_acc: m.heal_acc,
                ..default()
            },
            Self::Plain => MachineDataSave::default(),
        }
    }

    pub fn from_save(kind: MachineKind, d: &MachineDataSave) -> Self {
        let clean_slot = |slot: &Option<Slot>| {
            slot.as_ref().and_then(|slot| {
                let max = data::item_by_key(&slot.item)?.stack;
                (slot.n > 0).then(|| Slot {
                    item: slot.item.clone(),
                    n: slot.n.clamp(1, max),
                })
            })
        };
        let clean_slots = |slots: &[Option<Slot>], max_len: usize| {
            let mut cleaned = slots
                .iter()
                .take(max_len)
                .map(clean_slot)
                .collect::<Vec<_>>();
            cleaned.resize(max_len, None);
            cleaned
        };
        let finite = |value: f32, fallback: f32, max: f32| {
            if value.is_finite() {
                value.clamp(0.0, max)
            } else {
                fallback
            }
        };
        match kind {
            MachineKind::Furnace => Self::Furnace(FurnaceState {
                input: clean_slot(&d.input),
                fuel: clean_slot(&d.fuel),
                output: clean_slot(&d.output),
                prog: finite(d.prog, 0.0, 1.0),
                burn: finite(d.burn, 0.0, 86_400.0),
                burn_max: finite(d.burn_max, 0.0, 86_400.0),
                recipe: None, // 配方由输入物品自动匹配恢复
                on: false,
            }),
            MachineKind::Miner => Self::Miner(MinerState {
                output: clean_slot(&d.output),
                prog: finite(d.prog, 0.0, 1.0),
                deposit: d.deposit.clamp(0, 300),
            }),
            MachineKind::Belt | MachineKind::Pipe | MachineKind::Pump => Self::Belt(BeltState {
                items: d
                    .items
                    .iter()
                    .take(128)
                    .filter(|(item, t)| data::item_by_key(item).is_some() && t.is_finite())
                    .map(|(i, t)| BeltItem {
                        item: i.clone(),
                        t: t.clamp(0.0, 1.0),
                    })
                    .collect(),
            }),
            MachineKind::Splitter | MachineKind::Filter => Self::Router(RouterState {
                items: d
                    .items
                    .iter()
                    .take(128)
                    .filter(|(item, t)| data::item_by_key(item).is_some() && t.is_finite())
                    .map(|(i, t)| BeltItem {
                        item: i.clone(),
                        t: t.clamp(0.0, 1.0),
                    })
                    .collect(),
                filter: d
                    .label
                    .as_ref()
                    .filter(|item| data::item_by_key(item).is_some())
                    .cloned(),
                route: d.route % 3,
            }),
            MachineKind::Assembler | MachineKind::Refinery => Self::Crafter(CrafterState {
                // 配方 id 以字符串存档，读档时解析回 &'static str
                recipe: d
                    .recipe
                    .as_deref()
                    .and_then(|rid| data::RECIPES.iter().find(|r| r.id == rid).map(|r| r.id)),
                input: d
                    .input_map
                    .iter()
                    .filter(|(item, n)| data::item_by_key(item).is_some() && **n > 0)
                    .map(|(item, n)| (item.clone(), (*n).min(1_000_000)))
                    .collect(),
                output: clean_slot(&d.output),
                prog: finite(d.prog, 0.0, 1.0),
            }),
            MachineKind::Chest => Self::Chest(ChestState {
                slots: clean_slots(&d.slots, 24),
            }),
            MachineKind::Tank => Self::Tank(ChestState {
                slots: clean_slots(&d.slots, 12),
            }),
            MachineKind::Battery => Self::Battery(BatteryState {
                charge: finite(d.charge, 0.0, BATTERY_CAPACITY),
            }),
            MachineKind::ColonyCore => Self::Colony(ColonyState {
                input: d
                    .input_map
                    .iter()
                    .filter(|(item, amount)| colony_supply(item) && **amount > 0)
                    .map(|(item, amount)| (item.clone(), (*amount).min(1_000)))
                    .collect(),
                output: clean_slot(&d.output).filter(|slot| slot.item == "data"),
                prog: finite(d.prog, 0.0, 1.0),
                habitat: d.habitat.clamp(0, 10_000),
                residents: d.residents.clamp(0, 8),
                scan_t: 0.0,
                cycles: d.cycles.clamp(0, 1_000_000),
            }),
            MachineKind::Turret => Self::Turret(TurretState {
                cooldown: finite(d.cooldown, 0.0, 60.0),
                engaged: false,
                kills: d.kills.clamp(0, 1_000_000),
            }),
            MachineKind::Reactor => Self::Reactor(ReactorState {
                fuel: finite(d.fuel_s, 0.0, 86_400.0),
            }),
            MachineKind::Burner => Self::Burner(BurnerState {
                fuel: clean_slot(&d.fuel),
                burn: finite(d.burn, 0.0, 86_400.0),
                burn_max: finite(d.burn_max, 0.0, 86_400.0),
            }),
            MachineKind::Beacon => Self::Beacon(BeaconState {
                label: d
                    .label
                    .as_deref()
                    .unwrap_or("标记点")
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(128)
                    .collect(),
                gal: d.gal,
            }),
            MachineKind::Lumberbot => Self::Lumberbot(LumberbotState {
                cargo: d.cargo.clamp(0, 40),
                pos: d
                    .bot_pos
                    .filter(|pos| pos.iter().all(|v| v.is_finite()))
                    .map(|pos| {
                        [
                            pos[0].clamp(-1_000_000.0, 1_000_000.0),
                            pos[1].clamp(-256.0, 512.0),
                            pos[2].clamp(-1_000_000.0, 1_000_000.0),
                        ]
                    })
                    .unwrap_or_default(),
                ..default()
            }),
            MachineKind::Collector => Self::Collector(CollectorState {
                slots: clean_slots(&d.slots, 12),
            }),
            MachineKind::Medbay => Self::Medbay(MedbayState {
                heal_acc: finite(d.heal_acc, 0.0, 86_400.0),
            }),
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
    b.items.push(BeltItem {
        item: item.to_string(),
        t: t_start,
    });
    true
}

pub fn is_fluid_item(item: &str) -> bool {
    matches!(item, "acid" | "coolant" | "oxygen_cell" | "hazard_cell")
}

pub fn colony_supply(item: &str) -> bool {
    matches!(item, "oxygen_cell" | "medkit" | "biofiber")
}

fn colony_supplied(input: &HashMap<String, i32>) -> bool {
    input.get("oxygen_cell").copied().unwrap_or(0) >= 1
        && input.get("medkit").copied().unwrap_or(0) >= 1
        && input.get("biofiber").copied().unwrap_or(0) >= 2
}

fn colony_resident_capacity(habitat: i32) -> i32 {
    if habitat >= 12 {
        (habitat / 4).clamp(1, 8)
    } else {
        0
    }
}

fn colony_output_room(output: &Option<Slot>) -> bool {
    output
        .as_ref()
        .is_none_or(|slot| slot.item == "data" && slot.n <= 498)
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
        MachineState::Tank(c) => is_fluid_item(item) && slots_accept(&c.slots, item),
        MachineState::Collector(c) => slots_accept(&c.slots, item),
        MachineState::Crafter(c) => {
            let Some(rid) = c.recipe else { return false };
            let Some(r) = data::RECIPES.iter().find(|r| r.id == rid) else {
                return false;
            };
            if !r.inputs.iter().any(|(i, _)| *i == item) {
                return false;
            }
            let need = r
                .inputs
                .iter()
                .find(|(i, _)| *i == item)
                .map(|(_, n)| *n)
                .unwrap_or(1);
            c.input.get(item).copied().unwrap_or(0) < need * 3
        }
        MachineState::Belt(b) => {
            (!matches!(m.kind, MachineKind::Pipe | MachineKind::Pump) || is_fluid_item(item))
                && belt_can_accept(b, 0.0)
        }
        MachineState::Router(r) => !r.items.iter().any(|it| it.t < BELT_GAP),
        MachineState::Colony(c) => {
            colony_supply(item) && c.input.get(item).copied().unwrap_or(0) < 24
        }
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
                    None => {
                        f.fuel = Some(Slot {
                            item: item.to_string(),
                            n: 1,
                        })
                    }
                    _ => return false,
                }
                return true;
            }
            if recipe_match {
                match &mut f.input {
                    Some(s) if s.item == item => s.n += 1,
                    None => {
                        f.input = Some(Slot {
                            item: item.to_string(),
                            n: 1,
                        })
                    }
                    _ => return false,
                }
                return true;
            }
            match &mut f.fuel {
                Some(s) if s.item == item => s.n += 1,
                None => {
                    f.fuel = Some(Slot {
                        item: item.to_string(),
                        n: 1,
                    })
                }
                _ => return false,
            }
            true
        }
        MachineState::Chest(c) => insert_into_slots(&mut c.slots, item),
        MachineState::Tank(c) => insert_into_slots(&mut c.slots, item),
        MachineState::Collector(c) => insert_into_slots(&mut c.slots, item),
        MachineState::Crafter(c) => {
            *c.input.entry(item.to_string()).or_insert(0) += 1;
            true
        }
        MachineState::Belt(b) => belt_insert(b, item, 0.0),
        MachineState::Router(r) => {
            r.items.push(BeltItem {
                item: item.to_string(),
                t: 0.0,
            });
            true
        }
        MachineState::Colony(c) => {
            *c.input.entry(item.to_string()).or_default() += 1;
            true
        }
        MachineState::Reactor(r) => {
            r.fuel += 60.0;
            true
        }
        MachineState::Burner(b) => {
            match &mut b.fuel {
                Some(s) if s.item == item => s.n += 1,
                None => {
                    b.fuel = Some(Slot {
                        item: item.to_string(),
                        n: 1,
                    })
                }
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
        if let Some(sl) = s
            && sl.item == item
            && sl.n < stack
        {
            sl.n += 1;
            return true;
        }
    }
    for s in slots.iter_mut() {
        if s.is_none() {
            *s = Some(Slot {
                item: item.to_string(),
                n: 1,
            });
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
    let (dx, dz) = DIRS[dir_index(d)];
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
fn try_output(m: &Machine, item: &str, snap: &mut Snapshot) -> bool {
    let crafter = matches!(m.kind, MachineKind::Assembler | MachineKind::Refinery);
    let (fdx, fdz) = DIRS[dir_index(m.dir)];
    let mut targets: Vec<Entity> = Vec::new();
    let front = [m.pos[0] + fdx, m.pos[1], m.pos[2] + fdz];
    if let Some(e) = snap.pos_index.get(&front)
        && !(crafter && is_input_face(m, m.dir, snap))
    {
        targets.push(*e);
    }
    for (d, (dx, dz)) in DIRS.iter().enumerate() {
        if d == dir_index(m.dir) {
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
        let Some(tm) = snap.machines.get(&t).cloned() else {
            continue;
        };
        let Some(ts) = snap.states.get_mut(&t) else {
            continue;
        };
        if matches!(tm.kind, MachineKind::Belt) {
            if crafter {
                continue; // 装配/精炼不从侧面输出到皮带
            }
            if let MachineState::Belt(bs) = ts
                && belt_insert(bs, item, 0.1)
            {
                return true;
            }
        } else if can_machine_accept(&tm, item, ts) && machine_insert(&tm, ts, item) {
            return true;
        }
    }
    // 无处可送：产出留在机器内
    false
}

// ---------- per-tick logic ----------

fn furnace_tick(
    m: &mut Machine,
    f: &mut FurnaceState,
    snap: &mut Snapshot,
    drops: &mut Vec<([i32; 3], String, i32)>,
    commands: &mut Commands,
    sfx: &crate::audio::Sfx,
    researched: &[String],
) {
    let input_item = f.input.as_ref().map(|s| s.item.clone());
    let r = input_item.as_deref().and_then(|it| {
        data::RECIPES.iter().find(|r| {
            r.station == "furnace"
                && data::recipe_unlocked(researched, r)
                && r.inputs.iter().any(|(i, _)| *i == it)
        })
    });
    let need = r.and_then(|r| {
        input_item
            .as_deref()
            .and_then(|it| r.inputs.iter().find(|(i, _)| *i == it).map(|(_, n)| *n))
    });
    let can_work = r.is_some() && f.input.as_ref().map(|s| s.n).unwrap_or(0) >= need.unwrap_or(1);
    if f.burn <= 0.0
        && can_work
        && let Some(fuel) = f.fuel.as_mut()
        && fuel.n > 0
    {
        f.burn = data::fuel_value(&fuel.item).max(4.0);
        f.burn_max = f.burn;
        fuel.n -= 1;
        if fuel.n <= 0 {
            f.fuel = None;
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
                if let Some(o) = f.output.as_mut()
                    && o.item != out_item
                {
                    drops.push((m.pos, o.item.clone(), o.n));
                    f.output = None;
                }
                if f.output.is_none() {
                    f.output = Some(Slot {
                        item: out_item.to_string(),
                        n: 0,
                    });
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
    if let Some(o) = f.output.clone()
        && o.n > 0
        && try_output(m, &o.item, snap)
        && let Some(o2) = f.output.as_mut()
    {
        o2.n -= 1;
        if o2.n <= 0 {
            f.output = None;
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
    if let Some(o) = s.output.clone()
        && o.n > 0
        && try_output(m, &o.item, snap)
        && let Some(o2) = s.output.as_mut()
    {
        o2.n -= 1;
        if o2.n <= 0 {
            s.output = None;
        }
    }
    if !below.ore {
        return;
    }
    if sat <= 0.05 {
        return;
    }
    m.active = true;
    s.prog += TICK * 0.5 * sat;
    if s.prog >= 1.0 {
        s.prog = 0.0;
        let drop = below.drops.first().map(|d| d.item).unwrap_or("stone");
        if s.output.is_none() {
            s.output = Some(Slot {
                item: drop.to_string(),
                n: 0,
            });
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
    drops: &mut Vec<([i32; 3], String, i32)>,
    researched: &[String],
) {
    m.active = false;
    let Some(rid) = c.recipe else { return };
    // 装配机可制作 station=="hand" 的便携配方（JS where:'both' 语义）
    let Some(r) = data::RECIPES.iter().find(|r| {
        r.id == rid
            && data::recipe_unlocked(researched, r)
            && (r.station == where_
                || r.station == "both"
                || (where_ == "assembler" && r.station == "hand"))
    }) else {
        return;
    };
    let has_all = r
        .inputs
        .iter()
        .all(|(i, n)| c.input.get(*i).copied().unwrap_or(0) >= *n);
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
            if let Some(o) = c.output.as_mut()
                && o.item != out_item
            {
                drops.push((m.pos, o.item.clone(), o.n));
                c.output = None;
            }
            if c.output.is_none() {
                c.output = Some(Slot {
                    item: out_item.to_string(),
                    n: 0,
                });
            }
            if let Some(o) = c.output.as_mut() {
                o.n += r.output.1;
            }
        }
    }
    if let Some(o) = c.output.clone()
        && o.n > 0
        && try_output(m, &o.item, snap)
        && let Some(o2) = c.output.as_mut()
    {
        o2.n -= 1;
        if o2.n <= 0 {
            c.output = None;
        }
    }
}

fn belt_tick(m: &mut Machine, b: &mut BeltState, sat: f32, snap: &mut Snapshot) {
    let speed = match m.kind {
        MachineKind::Pipe => BELT_SPEED * 0.65,
        MachineKind::Pump => BELT_SPEED * 2.0 * sat,
        _ => BELT_SPEED,
    };
    m.active = !b.items.is_empty() && (m.kind != MachineKind::Pump || sat > 0.05);
    b.items.sort_by(|a, c| c.t.total_cmp(&a.t));
    let mut i = 0;
    while i < b.items.len() {
        let max_t = if i == 0 {
            1.0
        } else {
            (b.items[i - 1].t - BELT_GAP).max(0.0)
        };
        let t = b.items[i].t;
        b.items[i].t = (t + speed * TICK).min(max_t.max(t));
        if b.items[i].t >= 0.999 {
            let (dx, dz) = DIRS[dir_index(m.dir)];
            let item = b.items[i].item.clone();
            let nx = m.pos[0] + dx;
            let nz = m.pos[2] + dz;
            let mut moved = false;
            for y in [m.pos[1], m.pos[1] - 1, m.pos[1] + 1] {
                let tp = [nx, y, nz];
                let Some(te) = snap.pos_index.get(&tp).copied() else {
                    continue;
                };
                let Some(tm) = snap.machines.get(&te).cloned() else {
                    continue;
                };
                let Some(ts) = snap.states.get_mut(&te) else {
                    continue;
                };
                if matches!(tm.kind, MachineKind::Belt) {
                    if let MachineState::Belt(bs) = ts
                        && belt_insert(bs, &item, 0.0)
                    {
                        moved = true;
                        break;
                    }
                } else {
                    // 禁止向正面输入侧的装配/精炼推入（belt→machine 回流防护，
                    // JS: isInputFaceForBelt = (beltDir+2)%4 === next.dir）
                    let blocked = matches!(tm.kind, MachineKind::Assembler | MachineKind::Refinery)
                        && (m.dir + 2) % 4 == tm.dir
                        && (m.pos[0] + dx == tm.pos[0] && m.pos[2] + dz == tm.pos[2]);
                    if !blocked
                        && can_machine_accept(&tm, &item, ts)
                        && machine_insert(&tm, ts, &item)
                    {
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

fn insert_at_direction(m: &Machine, dir: u8, item: &str, snap: &mut Snapshot) -> bool {
    let (dx, dz) = DIRS[dir_index(dir)];
    for y in [m.pos[1], m.pos[1] - 1, m.pos[1] + 1] {
        let Some(target) = snap
            .pos_index
            .get(&[m.pos[0] + dx, y, m.pos[2] + dz])
            .copied()
        else {
            continue;
        };
        let Some(tm) = snap.machines.get(&target).cloned() else {
            continue;
        };
        let Some(ts) = snap.states.get_mut(&target) else {
            continue;
        };
        if can_machine_accept(&tm, item, ts) && machine_insert(&tm, ts, item) {
            return true;
        }
    }
    false
}

fn router_tick(m: &mut Machine, r: &mut RouterState, snap: &mut Snapshot) {
    m.active = !r.items.is_empty();
    r.items.sort_by(|a, b| b.t.total_cmp(&a.t));
    let mut i = 0;
    while i < r.items.len() {
        let max_t = if i == 0 {
            1.0
        } else {
            (r.items[i - 1].t - BELT_GAP).max(0.0)
        };
        let old_t = r.items[i].t;
        r.items[i].t = (old_t + BELT_SPEED * TICK).min(max_t.max(old_t));
        if r.items[i].t >= 0.999 {
            let item = r.items[i].item.clone();
            let dirs: Vec<u8> = if m.kind == MachineKind::Filter {
                if r.filter.as_deref() == Some(item.as_str()) {
                    vec![m.dir]
                } else {
                    vec![(m.dir + 1) % 4]
                }
            } else {
                let choices = [(m.dir + 3) % 4, m.dir, (m.dir + 1) % 4];
                (0..3)
                    .map(|n| choices[(r.route as usize + n) % 3])
                    .collect()
            };
            let mut moved = false;
            for dir in dirs {
                if insert_at_direction(m, dir, &item, snap) {
                    moved = true;
                    break;
                }
            }
            if moved {
                r.route = (r.route + 1) % 3;
                r.items.remove(i);
                continue;
            }
        }
        i += 1;
    }
}

fn tank_tick(m: &mut Machine, tank: &mut ChestState, snap: &mut Snapshot) {
    m.active = tank.slots.iter().any(Option::is_some);
    for slot in &mut tank.slots {
        let Some(stored) = slot.clone() else { continue };
        if try_output(m, &stored.item, snap)
            && let Some(current) = slot
        {
            current.n -= 1;
            if current.n <= 0 {
                *slot = None;
            }
        }
        break;
    }
}

fn colony_tick(
    m: &mut Machine,
    colony: &mut ColonyState,
    sat: f32,
    world: &GameWorld,
    snap: &mut Snapshot,
) -> Option<i32> {
    colony.scan_t -= TICK;
    if colony.scan_t <= 0.0 {
        colony.scan_t = 5.0;
        let mut habitat = 0;
        for x in (m.pos[0] - 8)..=(m.pos[0] + 8) {
            for y in (m.pos[1] - 4).max(0)..=(m.pos[1] + 4).min(data::WORLD_H - 1) {
                for z in (m.pos[2] - 8)..=(m.pos[2] + 8) {
                    if matches!(
                        world.get(x, y, z),
                        ids::HABITAT_FLOOR
                            | ids::WHITE_PANEL
                            | ids::DARK_PANEL
                            | ids::REINFORCED_GLASS
                    ) {
                        habitat += 1;
                    }
                }
            }
        }
        colony.habitat = habitat;
        colony.residents = colony_resident_capacity(habitat);
    }
    if let Some(output) = colony.output.clone()
        && output.n > 0
        && try_output(m, &output.item, snap)
        && let Some(current) = colony.output.as_mut()
    {
        current.n -= 1;
        if current.n <= 0 {
            colony.output = None;
        }
    }
    let supplied = colony_supplied(&colony.input);
    let output_room = colony_output_room(&colony.output);
    m.active = colony.residents > 0 && supplied && output_room && sat > 0.05;
    if !m.active {
        return None;
    }
    colony.prog += TICK * sat / 90.0;
    if colony.prog < 1.0 {
        return None;
    }
    colony.prog = 0.0;
    for (item, amount) in [("oxygen_cell", 1), ("medkit", 1), ("biofiber", 2)] {
        if let Some(stored) = colony.input.get_mut(item) {
            *stored -= amount;
        }
    }
    colony.input.retain(|_, amount| *amount > 0);
    match &mut colony.output {
        Some(slot) if slot.item == "data" => slot.n += 2,
        None => {
            colony.output = Some(Slot {
                item: "data".into(),
                n: 2,
            });
        }
        _ => {}
    }
    colony.cycles += 1;
    Some(200 + colony.residents * 25)
}

fn collector_tick(m: &mut Machine, c: &mut CollectorState, snap: &mut Snapshot) {
    m.active = c.slots.iter().any(|s| s.is_some());
    for i in 0..c.slots.len() {
        let Some(s) = c.slots[i].clone() else {
            continue;
        };
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

/// 伐木机器人（JS factory.js 状态机：扫描→移动→伐木→满载送货→等待）。
/// 常量：BOT_RANGE=32、BOT_CARGO_FULL=40、chop 1.1s、每段碳+4、整树+6、树叶 50% +1。
fn lumberbot_tick(
    m: &mut Machine,
    lb: &mut LumberbotState,
    world: &GameWorld,
    snap: &mut Snapshot,
    world_writes: &mut Vec<([i32; 3], u8)>,
) {
    const BOT_RANGE: i32 = 32;
    const BOT_CARGO_FULL: i32 = 40;
    m.active = true;
    let home = [
        m.pos[0] as f32 + 0.5,
        m.pos[1] as f32 + 0.5,
        m.pos[2] as f32 + 0.5,
    ];
    if lb.pos == [0.0, 0.0, 0.0] {
        lb.pos = home;
    }
    if lb.wait_t > 0.0 {
        lb.wait_t -= TICK;
        if lb.wait_t <= 0.0 {
            lb.phase = BotPhase::Scan;
        }
        return;
    }
    // 满载优先送货
    if lb.cargo >= BOT_CARGO_FULL && lb.phase != BotPhase::Deliver {
        lb.phase = BotPhase::Deliver;
        lb.target = None;
    }
    match lb.phase {
        BotPhase::Scan => {
            // 33×33（步长 2）列扫描，每 tick 6 列
            for _ in 0..6 {
                let n = 17;
                let off = lb.scan_off;
                lb.scan_off = (lb.scan_off + 1) % (n * n);
                let dx = ((off % n) as i32 - 8) * 2;
                let dz = ((off / n) as i32 - 8) * 2;
                let x = m.pos[0] + dx;
                let z = m.pos[2] + dz;
                if dx * dx + dz * dz > BOT_RANGE * BOT_RANGE {
                    continue;
                }
                if let Some(seg) = find_log_segment(world, x, z) {
                    lb.target = Some(seg);
                    lb.phase = BotPhase::Move;
                    return;
                }
            }
            if lb.scan_off == 0 {
                // 扫完一圈没树
                lb.phase = BotPhase::Wait;
                lb.wait_t = 5.0;
            }
        }
        BotPhase::Move => {
            let Some(t) = lb.target else {
                lb.phase = BotPhase::Scan;
                return;
            };
            let tp = [t[0] as f32 + 0.5, t[1] as f32 + 0.5, t[2] as f32 + 0.5];
            let d = ((tp[0] - lb.pos[0]).powi(2) + (tp[2] - lb.pos[2]).powi(2)).sqrt();
            if d < 1.5 {
                lb.phase = BotPhase::Chop;
                lb.chop_t = 0.0;
            } else {
                let step = 4.2 * TICK / d.max(1e-4);
                lb.pos[0] += (tp[0] - lb.pos[0]) * step;
                lb.pos[2] += (tp[2] - lb.pos[2]) * step;
            }
        }
        BotPhase::Chop => {
            let Some(mut t) = lb.target else {
                lb.phase = BotPhase::Scan;
                return;
            };
            // 目标已被挖走 → 重新找
            if world.get(t[0], t[1], t[2]) != ids::LOG {
                let Some(replacement) = find_log_segment(world, t[0], t[2]) else {
                    lb.phase = BotPhase::Scan;
                    return;
                };
                lb.target = Some(replacement);
                t = replacement;
                lb.chop_t = 0.0;
            }
            lb.chop_t += TICK;
            if lb.chop_t >= 1.1 {
                lb.chop_t = 0.0;
                // 砍一段：碳 +4
                world_writes.push((t, ids::AIR));
                lb.cargo = (lb.cargo + 4).min(BOT_CARGO_FULL);
                // 树干剩余？
                if let Some(next) = find_log_segment_except(world, t[0], t[2], Some(t)) {
                    lb.target = Some(next);
                } else {
                    // 整树砍完：碳 +6，清理 5×5×5 树叶（50% 各 +1）
                    lb.cargo = (lb.cargo + 6).min(BOT_CARGO_FULL);
                    for dy in -1..=3 {
                        for ox in -2..=2 {
                            for oz in -2..=2 {
                                let lx = t[0] + ox;
                                let ly = t[1] + dy;
                                let lz = t[2] + oz;
                                if world.get(lx, ly, lz) == ids::LEAVES {
                                    world_writes.push(([lx, ly, lz], ids::AIR));
                                    if crate::rng::Rng::new(
                                        (lx as u32).wrapping_mul(31) ^ (lz as u32).wrapping_mul(57),
                                    )
                                    .next()
                                        < 0.5
                                    {
                                        lb.cargo = (lb.cargo + 1).min(BOT_CARGO_FULL);
                                    }
                                }
                            }
                        }
                    }
                    lb.target = None;
                    if lb.cargo >= BOT_CARGO_FULL {
                        lb.phase = BotPhase::Deliver;
                    } else {
                        lb.phase = BotPhase::Scan;
                    }
                }
            }
        }
        BotPhase::Deliver => {
            // 找最近收集点
            let mut best: Option<([i32; 3], Entity)> = None;
            let mut best_d = f32::MAX;
            for (e, mm) in &snap.machines {
                if mm.kind != MachineKind::Collector {
                    continue;
                }
                let home_d2 = (mm.pos[0] as f32 + 0.5 - home[0]).powi(2)
                    + (mm.pos[2] as f32 + 0.5 - home[2]).powi(2);
                if home_d2 > (BOT_RANGE * 2) as f32 * (BOT_RANGE * 2) as f32 {
                    continue;
                }
                let d = ((mm.pos[0] as f32 + 0.5 - lb.pos[0]).powi(2)
                    + (mm.pos[2] as f32 + 0.5 - lb.pos[2]).powi(2))
                .sqrt();
                if d < best_d {
                    best_d = d;
                    best = Some((mm.pos, *e));
                }
            }
            match best {
                Some((cpos, ce)) => {
                    let cp = [
                        cpos[0] as f32 + 0.5,
                        cpos[1] as f32 + 0.5,
                        cpos[2] as f32 + 0.5,
                    ];
                    let d = ((cp[0] - lb.pos[0]).powi(2) + (cp[2] - lb.pos[2]).powi(2)).sqrt();
                    if d < 1.6 {
                        // 卸货（CollectorState 临时包装调用 machine_insert）
                        if let Some(ts) = snap.states.get_mut(&ce)
                            && let MachineState::Collector(cs) = ts
                        {
                            let mut wrap = MachineState::Collector(cs.clone());
                            while lb.cargo > 0 {
                                if !machine_insert(m, &mut wrap, "carbon") {
                                    break;
                                }
                                lb.cargo -= 1;
                            }
                            if let MachineState::Collector(nc) = &wrap {
                                *cs = nc.clone();
                            }
                        }
                        if lb.cargo <= 0 {
                            lb.phase = BotPhase::Scan;
                            lb.pos = home;
                        } else {
                            lb.phase = BotPhase::Wait;
                            lb.wait_t = 2.5;
                        }
                    } else {
                        let step = 4.2 * TICK / d.max(1e-4);
                        lb.pos[0] += (cp[0] - lb.pos[0]) * step;
                        lb.pos[2] += (cp[2] - lb.pos[2]) * step;
                    }
                }
                None => {
                    // 无收集点
                    lb.phase = BotPhase::Wait;
                    lb.wait_t = 3.0;
                }
            }
        }
        BotPhase::Wait => {}
    }
}

/// 在 (x,z) 列从地表向下最多 12 格找原木段（返回最上方的 log）。
fn find_log_segment(world: &GameWorld, x: i32, z: i32) -> Option<[i32; 3]> {
    find_log_segment_except(world, x, z, None)
}

fn find_log_segment_except(
    world: &GameWorld,
    x: i32,
    z: i32,
    skip: Option<[i32; 3]>,
) -> Option<[i32; 3]> {
    let top = world.top_at(x, z);
    for dy in 0..12 {
        let y = top - dy;
        if y < 1 {
            break;
        }
        let pos = [x, y, z];
        if Some(pos) != skip && world.get(x, y, z) == ids::LOG {
            return Some(pos);
        }
    }
    None
}

/// 伐木机器人悬浮视觉（挂在机器实体上，跟随 lb.pos）。
#[derive(Component)]
pub struct BotVis;

pub fn lumberbot_visual_system(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut mesh_h: Local<Option<Handle<Mesh>>>,
    mut mat_h: Local<Option<Handle<StandardMaterial>>>,
    q: Query<(Entity, &Machine, &MachineState), Without<BotVis>>,
    mut vis: Query<
        (&mut Transform, &mut Visibility, &Machine, &MachineState),
        (With<BotVis>, With<Machine>),
    >,
    time: Res<Time>,
) {
    let mesh = mesh_h
        .get_or_insert_with(|| meshes.add(Cuboid::new(0.42, 0.42, 0.42)))
        .clone();
    let mat = mat_h
        .get_or_insert_with(|| {
            mats.add(StandardMaterial {
                base_color: Color::srgb(0.62, 0.76, 0.82),
                emissive: LinearRgba::new(0.05, 0.3, 0.35, 1.0) * 0.8,
                ..default()
            })
        })
        .clone();
    for (e, _m, st) in &q {
        if let MachineState::Lumberbot(lb) = st
            && (lb.cargo > 0 || lb.phase != BotPhase::Scan)
        {
            commands.entity(e).try_insert((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(mat.clone()),
                BotVis,
            ));
        }
    }
    let t = time.elapsed_secs();
    for (mut tf, mut v, _m, st) in &mut vis {
        if let MachineState::Lumberbot(lb) = st {
            tf.translation = Vec3::new(
                lb.pos[0],
                lb.pos[1] + 0.8 + (t * 2.4).sin() * 0.08,
                lb.pos[2],
            );
            *v = Visibility::Visible;
        } else {
            *v = Visibility::Hidden;
        }
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

fn is_power_node(kind: MachineKind) -> bool {
    power_use(kind) > 0.0
        || power_gen(kind) > 0.0
        || matches!(
            kind,
            MachineKind::Wind | MachineKind::Cable | MachineKind::Battery
        )
}

fn power_components(snap: &Snapshot) -> Vec<Vec<Entity>> {
    let nodes: HashSet<Entity> = snap
        .machines
        .iter()
        .filter_map(|(entity, machine)| is_power_node(machine.kind).then_some(*entity))
        .collect();
    let cable_mode = snap
        .machines
        .values()
        .any(|machine| machine.kind == MachineKind::Cable);
    if !cable_mode {
        return vec![nodes.into_iter().collect()];
    }
    let offsets = [
        [1, 0, 0],
        [-1, 0, 0],
        [0, 1, 0],
        [0, -1, 0],
        [0, 0, 1],
        [0, 0, -1],
    ];
    let mut unseen = nodes;
    let mut out = Vec::new();
    while let Some(start) = unseen.iter().next().copied() {
        unseen.remove(&start);
        let mut queue = VecDeque::from([start]);
        let mut component = Vec::new();
        while let Some(entity) = queue.pop_front() {
            component.push(entity);
            let Some(machine) = snap.machines.get(&entity) else {
                continue;
            };
            for offset in offsets {
                let pos = [
                    machine.pos[0] + offset[0],
                    machine.pos[1] + offset[1],
                    machine.pos[2] + offset[2],
                ];
                if let Some(next) = snap.pos_index.get(&pos).copied()
                    && unseen.remove(&next)
                {
                    queue.push_back(next);
                }
            }
        }
        out.push(component);
    }
    // Backward compatibility: machines that are not attached to any cable
    // remain on the legacy wireless grid. Components containing a cable are
    // explicit local grids and no longer exchange power with that pool.
    let mut local = Vec::new();
    let mut legacy = Vec::new();
    for component in out {
        if component.iter().any(|entity| {
            snap.machines
                .get(entity)
                .is_some_and(|machine| machine.kind == MachineKind::Cable)
        }) {
            local.push(component);
        } else {
            legacy.extend(component);
        }
    }
    if !legacy.is_empty() {
        local.push(legacy);
    }
    local
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
    research: Option<Res<crate::ui::Research>>,
    mut flag_ev: MessageWriter<crate::quests::FlagEvent>,
) {
    acc.0 += time.delta_secs();
    if acc.0 < TICK {
        return;
    }
    // Keep the fractional/backlogged time instead of discarding it. A long
    // frame may catch up over following frames without making factories run
    // permanently slower than the rest of the game.
    acc.0 = (acc.0 - TICK).min(TICK * 4.0);

    // 阶段 A：快照
    let mut snap = Snapshot::new(&q);
    let day_f = daynight::day_factor(day.0);
    let mut gen_total = 0.0;
    let mut used = 0.0;
    let mut gen_by: HashMap<Entity, f32> = HashMap::new();
    let mut use_by: HashMap<Entity, f32> = HashMap::new();
    let wind_t = time.elapsed_secs();
    let Some(mut world) = world else { return };
    {
        let w: &GameWorld = &world;
        for (e, mut m, mut st) in q.iter_mut() {
            match &mut *st {
                MachineState::Plain => {
                    if m.kind == MachineKind::Solar {
                        let generated = power_gen(MachineKind::Solar) * day_f.max(0.0);
                        gen_total += generated;
                        gen_by.insert(e, generated);
                        m.active = generated > 0.05;
                    }
                    if m.kind == MachineKind::Wind {
                        let alt = (m.pos[1] as f32 - data::SEA_Y).max(0.0) * 0.18;
                        let gust = (wind_t * 0.5 + m.pos[0] as f32 * 0.7 + m.pos[2] as f32 * 1.3)
                            .sin()
                            * 3.0
                            + (wind_t * 0.13).sin() * 2.0;
                        let generated = (6.0 + alt + gust).clamp(2.0, 16.0);
                        gen_total += generated;
                        gen_by.insert(e, generated);
                        m.active = true;
                    }
                    if m.kind == MachineKind::Geothermal {
                        let below = w.get(m.pos[0], m.pos[1] - 1, m.pos[2]);
                        if matches!(below, ids::BASALT | ids::ASH | ids::OBSIDIAN) {
                            let generated = power_gen(MachineKind::Geothermal);
                            gen_total += generated;
                            gen_by.insert(e, generated);
                            m.active = true;
                        } else {
                            m.active = false;
                        }
                    }
                }
                MachineState::Burner(b) => {
                    if b.burn <= 0.0
                        && let Some(fuel) = b.fuel.as_mut()
                        && fuel.n > 0
                    {
                        b.burn = data::fuel_value(&fuel.item).max(4.0) * 1.5;
                        b.burn_max = b.burn;
                        fuel.n -= 1;
                        if fuel.n <= 0 {
                            b.fuel = None;
                        }
                    }
                    if b.burn > 0.0 {
                        b.burn -= TICK;
                        gen_total += power_gen(MachineKind::Burner);
                        gen_by.insert(e, power_gen(MachineKind::Burner));
                        m.active = true;
                    } else {
                        m.active = false;
                    }
                }
                MachineState::Reactor(r) => {
                    if r.fuel > 0.0 {
                        gen_total += power_gen(MachineKind::Reactor);
                        gen_by.insert(e, power_gen(MachineKind::Reactor));
                        r.fuel -= TICK;
                        m.active = true;
                    } else {
                        m.active = false;
                    }
                }
                MachineState::Miner(_) => {
                    if data::block_by_id(w.get(m.pos[0], m.pos[1] - 1, m.pos[2])).ore {
                        let draw = power_use(MachineKind::Miner);
                        used += draw;
                        use_by.insert(e, draw);
                    }
                }
                MachineState::Belt(b) if m.kind == MachineKind::Pump && !b.items.is_empty() => {
                    let draw = power_use(MachineKind::Pump);
                    used += draw;
                    use_by.insert(e, draw);
                }
                MachineState::Colony(c) => {
                    if c.residents > 0 && colony_supplied(&c.input) && colony_output_room(&c.output)
                    {
                        let draw = power_use(MachineKind::ColonyCore);
                        used += draw;
                        use_by.insert(e, draw);
                    }
                }
                MachineState::Turret(turret) => {
                    let draw = if turret.engaged {
                        power_use(MachineKind::Turret)
                    } else {
                        1.0
                    };
                    used += draw;
                    use_by.insert(e, draw);
                }
                MachineState::Crafter(c) => {
                    if let Some(rid) = c.recipe
                        && let Some(r) = data::RECIPES.iter().find(|r| r.id == rid)
                        && research
                            .as_ref()
                            .is_some_and(|state| data::recipe_unlocked(&state.techs, r))
                        && (c.prog > 0.0
                            || r.inputs
                                .iter()
                                .all(|(i, n)| c.input.get(*i).copied().unwrap_or(0) >= *n))
                    {
                        let draw = power_use(m.kind);
                        used += draw;
                        use_by.insert(e, draw);
                    }
                }
                MachineState::Medbay(_) => {
                    if let Some(pq) = player.as_ref()
                        && let Ok(p) = pq.single()
                        && medbay_wants(&m, p)
                    {
                        let draw = power_use(MachineKind::Medbay);
                        used += draw;
                        use_by.insert(e, draw);
                    }
                }
                _ => {}
            }
            // 同步快照（电力阶段的状态改动）
            snap.states.insert(e, st.clone());
            snap.machines.insert(e, m.clone());
        }
    }
    let mut sat_by: HashMap<Entity, f32> = HashMap::new();
    let mut battery_flow = 0.0;
    let mut served_total = 0.0;
    for component in power_components(&snap) {
        let generation: f32 = component
            .iter()
            .map(|entity| gen_by.get(entity).copied().unwrap_or(0.0))
            .sum();
        let demand: f32 = component
            .iter()
            .map(|entity| use_by.get(entity).copied().unwrap_or(0.0))
            .sum();
        let batteries: Vec<Entity> = component
            .iter()
            .copied()
            .filter(|entity| matches!(snap.states.get(entity), Some(MachineState::Battery(_))))
            .collect();
        let mut supplied = generation;
        if generation < demand {
            let mut energy_needed = (demand - generation) * TICK;
            for entity in &batteries {
                if let Some(MachineState::Battery(battery)) = snap.states.get_mut(entity) {
                    let taken = battery.charge.min(energy_needed);
                    battery.charge -= taken;
                    energy_needed -= taken;
                    battery_flow += taken / TICK;
                    supplied += taken / TICK;
                    if energy_needed <= f32::EPSILON {
                        break;
                    }
                }
            }
        } else if generation > demand {
            let mut surplus = (generation - demand) * TICK;
            for entity in &batteries {
                if let Some(MachineState::Battery(battery)) = snap.states.get_mut(entity) {
                    let stored = (BATTERY_CAPACITY - battery.charge).min(surplus);
                    battery.charge += stored;
                    surplus -= stored;
                    if surplus <= f32::EPSILON {
                        break;
                    }
                }
            }
        }
        let component_sat = if demand > 0.0 {
            (supplied / demand).clamp(0.0, 1.0)
        } else {
            1.0
        };
        served_total += demand * component_sat;
        for entity in component {
            sat_by.insert(entity, component_sat);
        }
    }
    let sat = if used > 0.0 {
        (served_total / used).min(1.0)
    } else {
        1.0
    };
    *power = Power {
        generation: (gen_total + battery_flow).round(),
        used,
        sat,
    };

    // 阶段 B：快照逻辑
    let mut drops: Vec<([i32; 3], String, i32)> = Vec::new();
    let mut world_writes: Vec<([i32; 3], u8)> = Vec::new();
    let mut order: Vec<Entity> = snap.machines.keys().copied().collect();
    order.sort();
    let researched = research
        .as_ref()
        .map(|state| state.techs.as_slice())
        .unwrap_or(&[]);
    for e in order {
        let Some(mut m) = snap.machines.get(&e).cloned() else {
            continue;
        };
        let Some(mut st) = snap.states.remove(&e) else {
            continue;
        };
        let machine_sat = sat_by.get(&e).copied().unwrap_or(sat);
        let mut colony_reward = None;
        match &mut st {
            MachineState::Furnace(f) => furnace_tick(
                &mut m,
                f,
                &mut snap,
                &mut drops,
                &mut commands,
                &sfx,
                researched,
            ),
            MachineState::Miner(ms) => miner_tick(
                &mut m,
                ms,
                machine_sat,
                &world,
                &mut snap,
                &mut world_writes,
            ),
            MachineState::Belt(bs) => belt_tick(&mut m, bs, machine_sat, &mut snap),
            MachineState::Router(router) => router_tick(&mut m, router, &mut snap),
            MachineState::Crafter(cs) => {
                let where_ = if m.kind == MachineKind::Refinery {
                    "refinery"
                } else {
                    "assembler"
                };
                crafter_tick(
                    &mut m,
                    cs,
                    machine_sat,
                    where_,
                    &mut snap,
                    &mut drops,
                    researched,
                );
            }
            MachineState::Collector(cs) => collector_tick(&mut m, cs, &mut snap),
            MachineState::Tank(tank) => tank_tick(&mut m, tank, &mut snap),
            MachineState::Colony(colony) => {
                colony_reward = colony_tick(&mut m, colony, machine_sat, &world, &mut snap)
            }
            MachineState::Turret(turret) => {
                // Low-voltage grids reduce fire rate instead of granting full
                // combat throughput from a tiny fraction of the 10 kW draw.
                turret.cooldown = (turret.cooldown - TICK * machine_sat).max(0.0);
                m.active = machine_sat > 0.05;
            }
            MachineState::Lumberbot(lb) => {
                lumberbot_tick(&mut m, lb, &world, &mut snap, &mut world_writes)
            }
            MachineState::Medbay(ms) => {
                m.active = false;
                if let Some(pq) = player.as_mut()
                    && let Ok(mut p) = pq.single_mut()
                    && medbay_wants(&m, &p)
                    && machine_sat > 0.05
                {
                    m.active = true;
                    ms.heal_acc += TICK * machine_sat;
                    while ms.heal_acc >= 1.0
                        && p.stats.hp < 8.0
                        && p.inv.count_item("sodium") >= 1
                        && p.inv.count_item("oxygen") >= 1
                    {
                        ms.heal_acc -= 1.0;
                        let removed_sodium = p.inv.remove_item("sodium", 1);
                        let removed_oxygen = p.inv.remove_item("oxygen", 1);
                        debug_assert!(removed_sodium && removed_oxygen);
                        p.stats.hp = (p.stats.hp + 3.0).min(8.0);
                        p.toast("医疗站：生命 +3");
                    }
                    ms.heal_acc = ms.heal_acc.min(0.999);
                }
            }
            _ => {}
        }
        if let Some(credits) = colony_reward {
            if let Some(pq) = player.as_mut()
                && let Ok(mut p) = pq.single_mut()
            {
                p.credits = p.credits.saturating_add(credits);
                p.toast(format!("殖民收益 +₪{credits} · 研究数据 ×2"));
            }
            flag_ev.write(crate::quests::FlagEvent {
                flag: "colonyOnline".into(),
            });
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
    for (pos, item, n) in drops {
        crate::creatures::spawn_drop(
            &mut commands,
            &world,
            &icons,
            Vec3::new(
                pos[0] as f32 + 0.5,
                pos[1] as f32 + 1.2,
                pos[2] as f32 + 0.5,
            ),
            Vec3::ZERO,
            item,
            n,
            0.4,
        );
    }
    // 世界改块（矿脉耗尽）
    for (pos, id) in world_writes {
        world.set(pos[0], pos[1], pos[2], id);
    }
}

/// Sync machines with world blocks: despawn machines whose block was removed.
pub fn machine_sync_system(
    mut world: ResMut<GameWorld>,
    machines: Query<(Entity, &Machine)>,
    mut commands: Commands,
) {
    let mut existing = std::collections::HashSet::new();
    let mut replacements = Vec::new();
    for (e, m) in &machines {
        // `World::get` intentionally returns AIR for an unloaded chunk. Do
        // not interpret that streaming placeholder as a deleted machine.
        let cx = m.pos[0].div_euclid(data::CHUNK);
        let cz = m.pos[2].div_euclid(data::CHUNK);
        if world.get_chunk(cx, cz).is_none() {
            if !existing.insert(m.pos) {
                commands.entity(e).despawn();
            }
            continue;
        }
        let def = data::block_by_id(world.get(m.pos[0], m.pos[1], m.pos[2]));
        if def.machine == Some(m.kind.block_key()) {
            if !existing.insert(m.pos) {
                commands.entity(e).despawn();
            }
        } else {
            commands.entity(e).despawn();
            if let Some(key) = def.machine
                && existing.insert(m.pos)
            {
                replacements.push((m.pos, key));
            }
        }
    }

    // Machine entities are logical state, while their block is stored in the
    // voxel chunk. A chunk can be evicted and later regenerated from a save;
    // recreate missing machine entities for those chunks so factories do not
    // silently stop after the player returns.
    let needs_scan = world.chunks.values().any(|chunk| chunk.machine_scan);
    if needs_scan {
        let mut to_spawn = replacements;
        for chunk in world.chunks.values_mut() {
            if !chunk.machine_scan {
                continue;
            }
            for (i, &id) in chunk.data.iter().enumerate() {
                let def = data::block_by_id(id);
                if def.machine.is_none() {
                    continue;
                }
                let pos = [
                    chunk.cx * data::CHUNK + (i % data::CHUNK as usize) as i32,
                    (i / (data::CHUNK as usize * data::CHUNK as usize)) as i32,
                    chunk.cz * data::CHUNK
                        + ((i / data::CHUNK as usize) % data::CHUNK as usize) as i32,
                ];
                if !existing.contains(&pos) {
                    to_spawn.push((pos, def.key));
                }
            }
            // Avoid rescanning this chunk on unrelated stream updates.
            chunk.machine_scan = false;
        }
        for (pos, key) in to_spawn {
            spawn_machine(&mut commands, pos, key, 0);
        }
    } else {
        for (pos, key) in replacements {
            spawn_machine(&mut commands, pos, key, 0);
        }
    }
}

/// Powered base-defense turret. Sentinels are always hostile; neutral fauna
/// is targeted only while retaliating, so a colony does not exterminate its
/// surrounding ecosystem by default.
fn turret_targetable(kind: &str, hp: f32, fading: bool, aggro_t: f32) -> bool {
    hp > 0.0
        && !fading
        && (kind == "sentinel" || (aggro_t > 0.0 && matches!(kind, "crab" | "beetle" | "hopper")))
}

pub fn turret_system(
    mut machines: Query<(&Machine, &mut MachineState)>,
    mut creatures: Query<(Entity, &mut Creature, &Transform)>,
    mut commands: Commands,
    sfx: Res<crate::audio::Sfx>,
) {
    let targets: Vec<(Entity, Vec3)> = creatures
        .iter_mut()
        .filter(|(_, creature, _)| {
            turret_targetable(
                creature.kind,
                creature.hp,
                creature.fading,
                creature.aggro_t,
            )
        })
        .map(|(entity, _, transform)| (entity, transform.translation))
        .collect();
    for (machine, mut state) in &mut machines {
        let MachineState::Turret(turret) = &mut *state else {
            continue;
        };
        let origin = Vec3::new(
            machine.pos[0] as f32 + 0.5,
            machine.pos[1] as f32 + 1.2,
            machine.pos[2] as f32 + 0.5,
        );
        let target = targets
            .iter()
            .filter_map(|(entity, pos)| {
                let distance_sq = origin.distance_squared(*pos);
                (distance_sq <= 24.0 * 24.0).then_some((*entity, *pos, distance_sq))
            })
            .min_by(|a, b| a.2.total_cmp(&b.2));
        turret.engaged = target.is_some();
        if !machine.active || turret.cooldown > 0.0 {
            continue;
        }
        let Some((target, pos, _)) = target else {
            continue;
        };
        let Ok((_, mut creature, _)) = creatures.get_mut(target) else {
            continue;
        };
        if creature.hp <= 0.0 {
            continue;
        }
        creature.hp -= 3.5;
        creature.hit_t = 0.25;
        creature.aggro_t = creature.aggro_t.max(8.0);
        turret.cooldown = 0.65;
        if creature.hp <= 0.0 {
            turret.kills += 1;
        }
        crate::audio::play_spatial(&mut commands, sfx.laser_hit.clone(), pos, 0.4, None);
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
    let mut positions = HashSet::new();
    for s in saves {
        let kind = MachineKind::from_block_key(&s.kind);
        if s.x.unsigned_abs() > 1_000_000
            || s.z.unsigned_abs() > 1_000_000
            || s.y < 0
            || s.y >= data::WORLD_H
            || kind == MachineKind::Other
            || !positions.insert([s.x, s.y, s.z])
        {
            continue;
        }
        let e = commands
            .spawn((
                Transform::from_xyz(s.x as f32 + 0.5, s.y as f32 + 0.5, s.z as f32 + 0.5),
                Machine {
                    pos: [s.x, s.y, s.z],
                    kind,
                    dir: s.dir % DIRS.len() as u8,
                    active: false,
                },
                MachineState::from_save(kind, &s.data),
                crate::InGame,
            ))
            .id();
        out.push((e, s.clone()));
    }
    out
}

// ---------- Plugin ----------

/// Factory plugin: machine world systems and power accounting.
pub struct FactoryPlugin;

impl Plugin for FactoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Power>()
            .init_resource::<TickAcc>()
            .add_systems(
                Update,
                (
                    factory_system.run_if(ground_mode),
                    turret_system.run_if(ground_mode),
                    machine_sync_system.run_if(ground_mode),
                    lumberbot_visual_system.run_if(ground_mode),
                )
                    .chain()
                    .in_set(GameSet::LateFactory)
                    .run_if(in_state(GameState::Playing)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine(kind: MachineKind, pos: [i32; 3], dir: u8) -> Machine {
        Machine {
            pos,
            kind,
            dir,
            active: false,
        }
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
            MachineKind::Splitter,
            MachineKind::Filter,
            MachineKind::Pipe,
            MachineKind::Pump,
            MachineKind::Tank,
            MachineKind::Battery,
            MachineKind::ColonyCore,
            MachineKind::Turret,
        ] {
            let st = MachineState::for_kind(kind);
            let save = st.to_save();
            let json = serde_json::to_string(&save).unwrap();
            let back: MachineDataSave = serde_json::from_str(&json).unwrap();
            let st2 = MachineState::from_save(kind, &back);
            // 往返后结构一致（槽位数量等）
            match (st, st2) {
                (MachineState::Chest(a), MachineState::Chest(b)) => {
                    assert_eq!(a.slots.len(), b.slots.len())
                }
                (MachineState::Collector(a), MachineState::Collector(b)) => {
                    assert_eq!(a.slots.len(), b.slots.len())
                }
                (MachineState::Belt(a), MachineState::Belt(b)) => {
                    assert_eq!(a.items.len(), b.items.len())
                }
                (MachineState::Furnace(a), MachineState::Furnace(b)) => {
                    assert_eq!(a.input.is_some(), b.input.is_some());
                    assert_eq!(a.fuel.is_some(), b.fuel.is_some());
                }
                (MachineState::Reactor(a), MachineState::Reactor(b)) => {
                    assert!((a.fuel - b.fuel).abs() < 0.01)
                }
                (MachineState::Burner(a), MachineState::Burner(b)) => {
                    assert_eq!(a.fuel.is_some(), b.fuel.is_some());
                }
                (MachineState::Beacon(a), MachineState::Beacon(b)) => assert_eq!(a.label, b.label),
                (MachineState::Router(a), MachineState::Router(b)) => {
                    assert_eq!(a.items.len(), b.items.len());
                    assert_eq!(a.filter, b.filter);
                }
                (MachineState::Tank(a), MachineState::Tank(b)) => {
                    assert_eq!(a.slots.len(), b.slots.len())
                }
                (MachineState::Battery(a), MachineState::Battery(b)) => {
                    assert!((a.charge - b.charge).abs() < 0.01)
                }
                (MachineState::Colony(a), MachineState::Colony(b)) => {
                    assert_eq!(a.input, b.input);
                    assert_eq!(a.habitat, b.habitat);
                    assert_eq!(a.residents, b.residents);
                    assert_eq!(a.cycles, b.cycles);
                }
                (MachineState::Turret(a), MachineState::Turret(b)) => {
                    assert!((a.cooldown - b.cooldown).abs() < 0.01);
                    assert_eq!(a.kills, b.kills);
                }
                _ => panic!("roundtrip kind mismatch"),
            }
        }
    }

    #[test]
    fn machine_state_save_is_sanitized() {
        let data = MachineDataSave {
            prog: f32::NAN,
            burn: f32::INFINITY,
            items: vec![("not-an-item".into(), f32::NAN), ("iron_ore".into(), 4.0)],
            slots: vec![Some(Slot {
                item: "not-an-item".into(),
                n: i32::MAX,
            })],
            ..default()
        };
        match MachineState::from_save(MachineKind::Belt, &data) {
            MachineState::Belt(b) => {
                assert_eq!(b.items.len(), 1);
                assert_eq!(b.items[0].item, "iron_ore");
            }
            _ => panic!("expected belt state"),
        }
        match MachineState::from_save(MachineKind::Furnace, &data) {
            MachineState::Furnace(f) => {
                assert_eq!(f.prog, 0.0);
                assert_eq!(f.burn, 0.0);
            }
            _ => panic!("expected furnace state"),
        }
        let MachineState::Chest(chest) = MachineState::from_save(MachineKind::Chest, &data) else {
            panic!("expected chest state");
        };
        assert_eq!(chest.slots.len(), 24);
        assert!(chest.slots.iter().all(Option::is_none));
    }

    #[test]
    fn power_use_gen_table() {
        assert_eq!(power_use(MachineKind::Miner), 8.0);
        assert_eq!(power_use(MachineKind::Assembler), 12.0);
        assert_eq!(power_use(MachineKind::Refinery), 20.0);
        assert_eq!(power_gen(MachineKind::Solar), 10.0);
        assert_eq!(power_gen(MachineKind::Reactor), 100.0);
        assert_eq!(power_gen(MachineKind::Burner), 25.0);
        assert_eq!(power_use(MachineKind::Pump), 2.0);
        assert_eq!(power_use(MachineKind::ColonyCore), 15.0);
        assert_eq!(power_use(MachineKind::Turret), 10.0);
        assert_eq!(power_gen(MachineKind::Geothermal), 45.0);
    }

    #[test]
    fn colony_accepts_only_bounded_supplies() {
        let m = machine(MachineKind::ColonyCore, [0, 0, 0], 0);
        let mut state = MachineState::for_kind(MachineKind::ColonyCore);
        assert!(!machine_insert(&m, &mut state, "iron"));
        for _ in 0..24 {
            assert!(machine_insert(&m, &mut state, "oxygen_cell"));
        }
        assert!(!machine_insert(&m, &mut state, "oxygen_cell"));
        assert!(machine_insert(&m, &mut state, "medkit"));
        assert!(machine_insert(&m, &mut state, "biofiber"));
        assert!(machine_insert(&m, &mut state, "biofiber"));
        let MachineState::Colony(colony) = state else {
            panic!("expected colony state");
        };
        assert!(colony_supplied(&colony.input));
    }

    #[test]
    fn colony_capacity_and_turret_hostility_are_conservative() {
        assert_eq!(colony_resident_capacity(11), 0);
        assert_eq!(colony_resident_capacity(12), 3);
        assert_eq!(colony_resident_capacity(200), 8);
        assert!(colony_output_room(&None));
        assert!(colony_output_room(&Some(Slot {
            item: "data".into(),
            n: 498,
        })));
        assert!(!colony_output_room(&Some(Slot {
            item: "data".into(),
            n: 499,
        })));
        assert!(turret_targetable("sentinel", 10.0, false, 0.0));
        assert!(!turret_targetable("crab", 10.0, false, 0.0));
        assert!(turret_targetable("crab", 10.0, false, 1.0));
        assert!(!turret_targetable("strider", 10.0, false, 10.0));
        assert!(!turret_targetable("sentinel", 0.0, false, 0.0));
        assert!(!turret_targetable("sentinel", 10.0, true, 0.0));
    }

    #[test]
    fn colony_save_discards_invalid_inventory_and_output() {
        let data = MachineDataSave {
            input_map: HashMap::from([
                ("oxygen_cell".into(), 2),
                ("iron".into(), 999),
                ("biofiber".into(), -3),
            ]),
            output: Some(Slot {
                item: "iron".into(),
                n: 10,
            }),
            habitat: i32::MAX,
            residents: i32::MAX,
            cycles: i32::MAX,
            ..default()
        };
        let MachineState::Colony(colony) = MachineState::from_save(MachineKind::ColonyCore, &data)
        else {
            panic!("expected colony state");
        };
        assert_eq!(colony.input, HashMap::from([("oxygen_cell".into(), 2)]));
        assert!(colony.output.is_none());
        assert_eq!(colony.habitat, 10_000);
        assert_eq!(colony.residents, 8);
        assert_eq!(colony.cycles, 1_000_000);
    }

    #[test]
    fn machine_block_registry_covers_all_declared_machine_ids() {
        let declared = data::BLOCKS
            .iter()
            .filter(|block| block.machine.is_some())
            .count();
        assert_eq!(MACHINE_BLOCK_IDS.len(), declared);
        let unique: HashSet<_> = MACHINE_BLOCK_IDS.into_iter().collect();
        assert_eq!(unique.len(), MACHINE_BLOCK_IDS.len());
        for id in MACHINE_BLOCK_IDS {
            let block = data::block_by_id(id);
            let kind = MachineKind::from_block_key(block.key);
            assert_ne!(kind, MachineKind::Other, "unmapped machine {}", block.key);
            assert_eq!(block.machine, Some(kind.block_key()));
            assert!(data::ITEMS.iter().any(|item| item.block == Some(block.key)));
        }
    }

    #[test]
    fn fluid_network_rejects_solid_items() {
        for kind in [MachineKind::Pipe, MachineKind::Pump, MachineKind::Tank] {
            let m = machine(kind, [0, 0, 0], 0);
            let mut state = MachineState::for_kind(kind);
            assert!(machine_insert(&m, &mut state, "coolant"));
            assert!(!machine_insert(&m, &mut state, "iron"));
        }
    }

    #[test]
    fn filter_router_persists_configuration() {
        let state = MachineState::Router(RouterState {
            filter: Some("iron_ore".into()),
            route: 2,
            items: vec![BeltItem {
                item: "copper_ore".into(),
                t: 0.5,
            }],
        });
        let save = state.to_save();
        match MachineState::from_save(MachineKind::Filter, &save) {
            MachineState::Router(router) => {
                assert_eq!(router.filter.as_deref(), Some("iron_ore"));
                assert_eq!(router.route, 2);
                assert_eq!(router.items.len(), 1);
            }
            _ => panic!("expected router state"),
        }
    }
}
