//! Game data: blocks, items, recipes, tech tree, biomes — 1:1 port of js/data.js.

pub const CHUNK: i32 = 16;
pub const WORLD_H: i32 = 96;
pub const SEA: i32 = 32;
pub const SEA_Y: f32 = 28.0; // neutral height / curvature grow anchor

/// Difficulty settings (drop multiplier per mode, from main.js).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Difficulty {
    Easy,
    Normal,
    Hard,
    Creative,
}

impl Difficulty {
    pub fn drop_mult(&self) -> f32 {
        match self {
            Difficulty::Easy => 7.0,
            Difficulty::Normal => 4.0,
            Difficulty::Hard => 1.0,
            Difficulty::Creative => 1.0,
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Difficulty::Easy => "轻松",
            Difficulty::Normal => "标准",
            Difficulty::Hard => "硬核",
            Difficulty::Creative => "创造",
        }
    }
}

#[derive(Clone, Debug)]
pub struct DropEntry {
    pub item: &'static str,
    pub n: i32,
    pub chance: f32, // 1.0 = always
}

#[derive(Clone, Debug)]
pub struct Tiles {
    pub all: Option<&'static str>,
    pub top: Option<&'static str>,
    pub side: Option<&'static str>,
    pub bottom: Option<&'static str>,
    pub front: Option<&'static str>,
}

impl Tiles {
    const fn new(all: &'static str) -> Self {
        Self {
            all: Some(all),
            top: None,
            side: None,
            bottom: None,
            front: None,
        }
    }
    const fn full(top: &'static str, side: &'static str, bottom: &'static str) -> Self {
        Self {
            all: None,
            top: Some(top),
            side: Some(side),
            bottom: Some(bottom),
            front: None,
        }
    }
    const fn front(all: &'static str, front: &'static str) -> Self {
        Self {
            all: Some(all),
            top: None,
            side: None,
            bottom: None,
            front: Some(front),
        }
    }
    /// Tile name for a face: 0..5 = +X,-X,+Y,-Y,+Z,-Z (matches FACES order)
    pub fn for_face(&self, face: usize) -> &'static str {
        if self.all.is_some() && self.top.is_none() && self.front.is_none() {
            return self.all.unwrap();
        }
        match face {
            2 => self.top.or(self.all).or(self.side).unwrap(),
            3 => self.bottom.or(self.all).or(self.side).unwrap(),
            4 => self.front.or(self.side).or(self.all).or(self.top).unwrap(),
            _ => self.side.or(self.all).or(self.top).unwrap(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Block {
    pub id: u8,
    pub key: &'static str,
    pub name: &'static str,
    pub solid: bool,
    pub hard: f32, // seconds to mine; f32::INFINITY = unbreakable
    pub transparent: bool,
    pub fancy: bool,
    pub cross: bool,
    pub liquid: bool,
    pub glow: bool,
    pub ore: bool,
    pub machine: Option<&'static str>,
    pub lowbox: Option<f32>, // None=full, Some(0.2)=lowbox true, Some(0.45)=slab
    pub tiles: Tiles,
    pub drops: &'static [DropEntry],
}

impl Block {
    pub const fn def(
        id: u8,
        key: &'static str,
        name: &'static str,
        hard: f32,
        tiles: Tiles,
        drops: &'static [DropEntry],
    ) -> Self {
        Self {
            id,
            key,
            name,
            solid: true,
            hard,
            transparent: false,
            fancy: false,
            cross: false,
            liquid: false,
            glow: false,
            ore: false,
            machine: None,
            lowbox: None,
            tiles,
            drops,
        }
    }
}

macro_rules! block {
    ($id:expr, $key:expr, $name:expr, $hard:expr, $tiles:expr, $drops:expr $(, $field:ident : $val:expr)*) => {{
        let mut b = Block::def($id, $key, $name, $hard, $tiles, $drops);
        $(b.$field = $val;)*
        b
    }};
}

/// Ids used by world generation (u8, stable ABI of chunk data).
pub mod ids {
    pub const AIR: u8 = 0;
    pub const GRASS: u8 = 1;
    pub const DIRT: u8 = 2;
    pub const STONE: u8 = 3;
    pub const SAND: u8 = 4;
    pub const LOG: u8 = 5;
    pub const LEAVES: u8 = 6;
    pub const COAL_ORE: u8 = 7;
    pub const IRON_ORE: u8 = 8;
    pub const COPPER_ORE: u8 = 9;
    pub const TITANIUM_ORE: u8 = 10;
    pub const URANIUM_ORE: u8 = 11;
    pub const GOLD_ORE: u8 = 12;
    pub const SODIUM_PLANT: u8 = 13;
    pub const OXYGEN_PLANT: u8 = 14;
    pub const FERN: u8 = 15;
    pub const WATER: u8 = 16;
    pub const PLANKS: u8 = 17;
    pub const GLASS: u8 = 18;
    pub const LAMP: u8 = 19;
    pub const ICE: u8 = 20;
    pub const SNOW: u8 = 21;
    pub const BASALT: u8 = 22;
    pub const ALIEN: u8 = 23;
    pub const BARRIER: u8 = 24;
    pub const FURNACE: u8 = 30;
    pub const MINER: u8 = 31;
    pub const BELT: u8 = 32;
    pub const ASSEMBLER: u8 = 33;
    pub const SOLAR: u8 = 34;
    pub const REFINERY: u8 = 35;
    pub const CHEST: u8 = 36;
    pub const REACTOR: u8 = 37;
    pub const LAUNCHPAD: u8 = 38;
    pub const WIND: u8 = 39;
    pub const BURNER: u8 = 40;
    pub const CRYSTAL: u8 = 41;
    pub const MUSH_STEM: u8 = 42;
    pub const MUSH_CAP: u8 = 43;
    pub const ASH: u8 = 44;
    pub const AMBER: u8 = 45;
    pub const RUST: u8 = 46;
    pub const SALT: u8 = 47;
    pub const OBSIDIAN: u8 = 48;
    pub const REDMOSS: u8 = 49;
    pub const HIVE: u8 = 50;
    pub const MURK: u8 = 51;
    pub const GLOW_SHROOM: u8 = 52;
    pub const BEACON: u8 = 53;
    pub const LUMBERBOT: u8 = 54;
    pub const COLLECTOR: u8 = 55;
    pub const MEDBAY: u8 = 56;
    pub const SLAB: u8 = 57;
    pub const METAL: u8 = 58;
    pub const CONCRETE: u8 = 59;
}

const NO_DROPS: &[DropEntry] = &[];
const D1: &[DropEntry] = &[DropEntry { item: "dirt", n: 1, chance: 1.0 }];
const D_STONE: &[DropEntry] = &[DropEntry { item: "stone", n: 1, chance: 1.0 }];
const D_CARBON3: &[DropEntry] = &[DropEntry { item: "carbon", n: 3, chance: 1.0 }];
const D_LEAVES: &[DropEntry] = &[
    DropEntry { item: "carbon", n: 1, chance: 1.0 },
    DropEntry { item: "oxygen", n: 1, chance: 0.35 },
];
const D_COAL: &[DropEntry] = &[
    DropEntry { item: "coal", n: 1, chance: 1.0 },
    DropEntry { item: "coal", n: 1, chance: 0.3 },
];
const D_IRON_ORE: &[DropEntry] = &[DropEntry { item: "iron_ore", n: 1, chance: 1.0 }];
const D_COPPER_ORE: &[DropEntry] = &[DropEntry { item: "copper_ore", n: 1, chance: 1.0 }];
const D_TITANIUM_ORE: &[DropEntry] = &[DropEntry { item: "titanium_ore", n: 1, chance: 1.0 }];
const D_URANIUM: &[DropEntry] = &[DropEntry { item: "uranium", n: 1, chance: 1.0 }];
const D_GOLD_ORE: &[DropEntry] = &[DropEntry { item: "gold_ore", n: 1, chance: 1.0 }];
const D_SODIUM2: &[DropEntry] = &[DropEntry { item: "sodium", n: 2, chance: 1.0 }];
const D_OXYGEN2: &[DropEntry] = &[DropEntry { item: "oxygen", n: 2, chance: 1.0 }];
const D_CARBON1: &[DropEntry] = &[DropEntry { item: "carbon", n: 1, chance: 1.0 }];
const D_PLANKS: &[DropEntry] = &[DropEntry { item: "planks_b", n: 1, chance: 1.0 }];
const D_GLASS: &[DropEntry] = &[DropEntry { item: "glass_b", n: 1, chance: 1.0 }];
const D_LAMP: &[DropEntry] = &[DropEntry { item: "lamp_b", n: 1, chance: 1.0 }];
const D_BASALT: &[DropEntry] = &[
    DropEntry { item: "stone", n: 1, chance: 1.0 },
    DropEntry { item: "coal", n: 1, chance: 0.15 },
];
const D_ALIEN: &[DropEntry] = &[
    DropEntry { item: "dirt", n: 1, chance: 1.0 },
    DropEntry { item: "sodium", n: 1, chance: 0.2 },
];
const D_FURNACE: &[DropEntry] = &[DropEntry { item: "furnace_b", n: 1, chance: 1.0 }];
const D_MINER: &[DropEntry] = &[DropEntry { item: "miner_b", n: 1, chance: 1.0 }];
const D_BELT: &[DropEntry] = &[DropEntry { item: "belt_b", n: 1, chance: 1.0 }];
const D_ASSEMBLER: &[DropEntry] = &[DropEntry { item: "assembler_b", n: 1, chance: 1.0 }];
const D_SOLAR: &[DropEntry] = &[DropEntry { item: "solar_b", n: 1, chance: 1.0 }];
const D_REFINERY: &[DropEntry] = &[DropEntry { item: "refinery_b", n: 1, chance: 1.0 }];
const D_CHEST: &[DropEntry] = &[DropEntry { item: "chest_b", n: 1, chance: 1.0 }];
const D_REACTOR: &[DropEntry] = &[DropEntry { item: "reactor_b", n: 1, chance: 1.0 }];
const D_LAUNCHPAD: &[DropEntry] = &[DropEntry { item: "launchpad_b", n: 1, chance: 1.0 }];
const D_WIND: &[DropEntry] = &[DropEntry { item: "wind_b", n: 1, chance: 1.0 }];
const D_BURNER: &[DropEntry] = &[DropEntry { item: "burner_b", n: 1, chance: 1.0 }];
const D_CRYSTAL: &[DropEntry] = &[
    DropEntry { item: "tritium", n: 2, chance: 1.0 },
    DropEntry { item: "tritium", n: 2, chance: 0.5 },
];
const D_MUSH_STEM: &[DropEntry] = &[DropEntry { item: "carbon", n: 2, chance: 1.0 }];
const D_MUSH_CAP: &[DropEntry] = &[
    DropEntry { item: "carbon", n: 1, chance: 1.0 },
    DropEntry { item: "oxygen", n: 1, chance: 0.4 },
    DropEntry { item: "sodium", n: 1, chance: 0.2 },
];
const D_ASH: &[DropEntry] = &[
    DropEntry { item: "dirt", n: 1, chance: 1.0 },
    DropEntry { item: "coal", n: 1, chance: 0.12 },
];
const D_AMBER: &[DropEntry] = &[
    DropEntry { item: "carbon", n: 2, chance: 1.0 },
    DropEntry { item: "gold_ore", n: 1, chance: 0.08 },
];
const D_RUST: &[DropEntry] = &[
    DropEntry { item: "dirt", n: 1, chance: 1.0 },
    DropEntry { item: "iron_ore", n: 1, chance: 0.25 },
];
const D_SALT: &[DropEntry] = &[
    DropEntry { item: "sodium", n: 1, chance: 1.0 },
    DropEntry { item: "sodium", n: 1, chance: 0.4 },
];
const D_OBSIDIAN: &[DropEntry] = &[
    DropEntry { item: "stone", n: 1, chance: 1.0 },
    DropEntry { item: "titanium_ore", n: 1, chance: 0.1 },
];
const D_REDMOSS: &[DropEntry] = &[
    DropEntry { item: "dirt", n: 1, chance: 1.0 },
    DropEntry { item: "carbon", n: 1, chance: 0.25 },
];
const D_HIVE: &[DropEntry] = &[
    DropEntry { item: "dirt", n: 1, chance: 1.0 },
    DropEntry { item: "carbon", n: 1, chance: 0.35 },
];
const D_MURK: &[DropEntry] = &[
    DropEntry { item: "dirt", n: 1, chance: 1.0 },
    DropEntry { item: "oxygen", n: 1, chance: 0.15 },
];
const D_GLOW_SHROOM: &[DropEntry] = &[
    DropEntry { item: "oxygen", n: 2, chance: 1.0 },
    DropEntry { item: "sodium", n: 1, chance: 0.5 },
];
const D_BEACON: &[DropEntry] = &[DropEntry { item: "beacon_b", n: 1, chance: 1.0 }];
const D_LUMBERBOT: &[DropEntry] = &[DropEntry { item: "lumberbot_b", n: 1, chance: 1.0 }];
const D_COLLECTOR: &[DropEntry] = &[DropEntry { item: "collector_b", n: 1, chance: 1.0 }];
const D_MEDBAY: &[DropEntry] = &[DropEntry { item: "medbay_b", n: 1, chance: 1.0 }];
const D_SLAB: &[DropEntry] = &[DropEntry { item: "slab_b", n: 1, chance: 1.0 }];
const D_METAL: &[DropEntry] = &[DropEntry { item: "metal_b", n: 1, chance: 1.0 }];
const D_CONCRETE: &[DropEntry] = &[DropEntry { item: "concrete_b", n: 1, chance: 1.0 }];

pub const BLOCKS: &[Block] = &[
    // 0
    block!(0, "air", "空气", f32::NAN, Tiles::new("air"), NO_DROPS, solid: false),
    block!(1, "grass", "草方块", 0.75, Tiles::full("grass_top", "grass_side", "dirt"), D1),
    block!(2, "dirt", "泥土", 0.7, Tiles::new("dirt"), D1),
    block!(3, "stone", "岩石", 1.6, Tiles::new("stone"), D_STONE),
    block!(4, "sand", "沙", 0.6, Tiles::new("sand"), D1),
    block!(5, "log", "碳质木干", 1.1, Tiles::full("log_top", "log_side", "log_top"), D_CARBON3),
    block!(6, "leaves", "叶簇", 0.3, Tiles::new("leaves"), D_LEAVES, transparent: true, fancy: true),
    block!(7, "coal_ore", "煤矿脉", 2.2, Tiles::new("coal_ore"), D_COAL, ore: true),
    block!(8, "iron_ore", "铁矿脉", 2.6, Tiles::new("iron_ore"), D_IRON_ORE, ore: true),
    block!(9, "copper_ore", "铜矿脉", 2.6, Tiles::new("copper_ore"), D_COPPER_ORE, ore: true),
    block!(10, "titanium_ore", "钛矿脉", 3.6, Tiles::new("titanium_ore"), D_TITANIUM_ORE, ore: true),
    block!(11, "uranium_ore", "铀矿脉", 4.2, Tiles::new("uranium_ore"), D_URANIUM, ore: true),
    block!(12, "gold_ore", "金矿脉", 3.0, Tiles::new("gold_ore"), D_GOLD_ORE, ore: true),
    block!(13, "sodium_plant", "钠素花", 0.05, Tiles::new("sodium_plant"), D_SODIUM2, solid: false, cross: true),
    block!(14, "oxygen_plant", "氧素花", 0.05, Tiles::new("oxygen_plant"), D_OXYGEN2, solid: false, cross: true),
    block!(15, "fern", "碳蕨", 0.05, Tiles::new("carbon_fern"), D_CARBON1, solid: false, cross: true),
    block!(16, "water", "水", f32::NAN, Tiles::new("water"), NO_DROPS, solid: false, transparent: true, liquid: true),
    block!(17, "planks", "碳板", 0.9, Tiles::new("planks"), D_PLANKS),
    block!(18, "glass", "玻璃", 0.4, Tiles::new("glass"), D_GLASS, transparent: true),
    block!(19, "lamp", "光源方块", 0.5, Tiles::new("lamp_on"), D_LAMP, glow: true),
    block!(20, "ice", "永冻冰", 1.2, Tiles::new("ice"), D_STONE),
    block!(21, "snow", "雪被层", 0.7, Tiles::full("snow_top", "snow_side", "dirt"), D1),
    block!(22, "basalt", "玄武岩", 2.0, Tiles::new("basalt"), D_BASALT),
    block!(23, "alien", "荧紫菌毯", 0.75, Tiles::full("alien_top", "alien_side", "dirt"), D_ALIEN),
    block!(24, "barrier", "致密基岩", f32::INFINITY, Tiles::new("barrier"), NO_DROPS),
    // 25..29 reserved (unused)
    block!(30, "furnace", "熔炉", 1.2, Tiles::front("stone", "furnace_front"), D_FURNACE, machine: Some("furnace")),
    block!(31, "miner", "自动采矿机", 1.2, Tiles::full("miner_top", "metal", "metal"), D_MINER, machine: Some("miner")),
    block!(32, "belt", "传送带", 0.5, Tiles::new("belt"), D_BELT, machine: Some("belt"), lowbox: Some(0.2)),
    block!(33, "assembler", "装配机", 1.4, Tiles::full("assembler_top", "metal", "metal"), D_ASSEMBLER, machine: Some("assembler")),
    block!(34, "solar", "太阳能板", 0.8, Tiles::new("solar_top"), D_SOLAR, machine: Some("solar"), lowbox: Some(0.2)),
    block!(35, "refinery", "精炼厂", 1.6, Tiles::new("refinery_side"), D_REFINERY, machine: Some("refinery")),
    block!(36, "chest", "储物箱", 0.9, Tiles::full("storage_top", "chest_side", "chest_side"), D_CHEST, machine: Some("chest")),
    block!(37, "reactor", "核子反应堆", 2.4, Tiles::new("reactor_side"), D_REACTOR, machine: Some("reactor")),
    block!(38, "launchpad", "发射平台", 2.0, Tiles::new("launchpad_top"), D_LAUNCHPAD, machine: Some("launchpad"), lowbox: Some(0.2)),
    block!(39, "wind", "风力涡轮机", 1.0, Tiles::new("metal"), D_WIND, machine: Some("wind")),
    block!(40, "burner", "火力发电机", 1.2, Tiles::front("metal_dark", "furnace_front"), D_BURNER, machine: Some("burner")),
    block!(41, "crystal", "氚晶簇", 1.8, Tiles::new("crystal"), D_CRYSTAL, glow: true),
    block!(42, "mush_stem", "巨菌柄", 0.8, Tiles::new("mush_stem"), D_MUSH_STEM),
    block!(43, "mush_cap", "巨菌盖", 0.5, Tiles::new("mush_cap"), D_MUSH_CAP),
    block!(44, "ash", "灰烬土", 0.8, Tiles::new("ash"), D_ASH),
    block!(45, "amber", "金珀岩", 1.4, Tiles::new("amber"), D_AMBER, glow: true),
    block!(46, "rust", "锈蚀铁壤", 1.0, Tiles::new("rust"), D_RUST),
    block!(47, "salt", "盐晶块", 0.7, Tiles::new("salt"), D_SALT),
    block!(48, "obsidian", "黑曜岩", 2.6, Tiles::new("obsidian"), D_OBSIDIAN),
    block!(49, "redmoss", "红藓被", 0.75, Tiles::full("redmoss_top", "redmoss_side", "dirt"), D_REDMOSS),
    block!(50, "hive", "蜂窝晶壁", 1.1, Tiles::new("hive"), D_HIVE),
    block!(51, "murk", "荧沼菌毯", 0.75, Tiles::full("murk_top", "murk_side", "dirt"), D_MURK),
    block!(52, "glow_shroom", "荧光蕈", 0.05, Tiles::new("glow_shroom"), D_GLOW_SHROOM, solid: false, cross: true, glow: true),
    block!(53, "beacon", "标记方块", 0.8, Tiles::full("lamp_on", "metal_dark", "metal_dark"), D_BEACON, machine: Some("beacon")),
    block!(54, "lumberbot", "伐木机器人", 1.0, Tiles::full("metal_dark", "vent", "vent"), D_LUMBERBOT, machine: Some("lumberbot")),
    block!(55, "collector", "收集点", 0.9, Tiles::full("storage_top", "chest_side", "chest_side"), D_COLLECTOR, machine: Some("collector")),
    block!(56, "medbay", "医疗站", 1.4, Tiles::full("medbay_top", "metal_dark", "metal_dark"), D_MEDBAY, machine: Some("medbay")),
    block!(57, "slab", "石半砖", 1.0, Tiles::new("slab"), D_SLAB, lowbox: Some(0.45)),
    block!(58, "metal", "金属块", 2.0, Tiles::new("metal"), D_METAL),
    block!(59, "concrete", "混凝土块", 1.6, Tiles::new("concrete"), D_CONCRETE),
];

pub fn block_by_id(id: u8) -> &'static Block {
    BLOCKS.iter().find(|b| b.id == id).unwrap_or(&BLOCKS[0])
}

pub fn block_by_key(key: &str) -> &'static Block {
    BLOCKS.iter().find(|b| b.key == key).unwrap_or(&BLOCKS[0])
}

/// Item definition.
#[derive(Clone, Debug)]
pub struct Item {
    pub key: &'static str,
    pub name: &'static str,
    pub cat: &'static str, // res | mat | blk | mach
    pub icon_fn: Option<&'static str>,
    pub icon_block: Option<&'static str>,
    pub block: Option<&'static str>,
    pub stack: i32,
    pub price: i32,
    pub desc: &'static str,
}

macro_rules! item {
    (res $key:expr, $name:expr, $icon:expr, $stack:expr, $price:expr, $desc:expr) => {
        Item {
            key: $key,
            name: $name,
            cat: "res",
            icon_fn: Some($icon),
            icon_block: None,
            block: None,
            stack: $stack,
            price: $price,
            desc: $desc,
        }
    };
    (mat $key:expr, $name:expr, $icon:expr, $stack:expr, $price:expr, $desc:expr) => {
        Item {
            key: $key,
            name: $name,
            cat: "mat",
            icon_fn: Some($icon),
            icon_block: None,
            block: None,
            stack: $stack,
            price: $price,
            desc: $desc,
        }
    };
    (blk $key:expr, $name:expr, $block:expr, $stack:expr, $price:expr, $desc:expr) => {
        Item {
            key: $key,
            name: $name,
            cat: "blk",
            icon_fn: None,
            icon_block: Some($block),
            block: Some($block),
            stack: $stack,
            price: $price,
            desc: $desc,
        }
    };
    (mach $key:expr, $name:expr, $block:expr, $stack:expr, $price:expr, $desc:expr) => {
        Item {
            key: $key,
            name: $name,
            cat: "mach",
            icon_fn: None,
            icon_block: Some($block),
            block: Some($block),
            stack: $stack,
            price: $price,
            desc: $desc,
        }
    };
}

pub const ITEMS: &[Item] = &[
    item!(res "carbon", "碳", "carbon", 250, 4, "一切有机物的基础，也是基础燃料。"),
    item!(res "oxygen", "氧气", "oxygen", 250, 6, "为生命维持系统充能。"),
    item!(res "sodium", "钠", "sodium", 250, 8, "为危险防护装置充能。"),
    item!(res "coal", "煤", "coal", 250, 10, "高能燃料，熔炉的最爱。"),
    item!(res "iron_ore", "铁矿石", "iron_ore", 250, 8, "需熔炼成铁锭。"),
    item!(res "copper_ore", "铜矿石", "copper_ore", 250, 8, "需熔炼成铜锭。"),
    item!(res "titanium_ore", "钛矿石", "titanium_ore", 250, 24, "稀有轻金属矿。"),
    item!(res "gold_ore", "金矿石", "gold_ore", 250, 40, "闪闪发光，星站高价收购。"),
    item!(res "uranium", "铀-235", "uranium", 100, 60, "微微发热…核反应堆燃料。"),
    item!(res "tritium", "氚", "tritium", 500, 12, "脉冲引擎燃料，击碎小行星获取。"),
    item!(mat "iron", "铁锭", "iron", 250, 18, "工业的骨架。"),
    item!(mat "copper", "铜锭", "copper", 250, 18, "导电材料。"),
    item!(mat "titanium", "钛锭", "titanium", 250, 55, "航天级合金。"),
    item!(mat "gold", "金锭", "gold", 250, 90, "贵金属，硬通货。"),
    item!(mat "gear", "齿轮", "gear", 250, 42, "机械传动核心。"),
    item!(mat "wire", "铜线圈", "wire", 250, 24, "缠绕的铜线。"),
    item!(mat "circuit", "电路板", "circuit", 200, 110, "所有智能机器的大脑。"),
    item!(mat "plate", "装甲板", "plate", 200, 60, "飞船与机器的外壳。"),
    item!(mat "data", "研究数据", "data", 500, 150, "科技矩阵的解锁密钥。"),
    item!(mat "fuel", "发射燃料", "fuel", 20, 320, "让飞船挣脱引力的怒吼。"),
    item!(mat "antimatter", "反物质", "antimatter", 10, 45000, "被磁场囚禁的湮灭之光——曲率引擎的心脏。"),
    item!(mat "warpcell", "曲率电池", "warp", 10, 240000, "跨星系跃迁的船票。第一章的终点，自由的起点。"),
    item!(blk "dirt", "泥土", "dirt", 250, 1, "朴实无华的土。"),
    item!(blk "stone", "岩石", "stone", 250, 2, "基础建材，可烧炼加工。"),
    item!(blk "sand", "沙", "sand", 250, 2, "可烧制成玻璃。"),
    item!(blk "planks_b", "碳板块", "planks", 250, 6, "压缩碳建材。"),
    item!(blk "glass_b", "玻璃", "glass", 250, 12, "透明建材。"),
    item!(blk "lamp_b", "光源方块", "lamp", 100, 30, "照亮黑夜。"),
    item!(blk "slab_b", "石半砖", "slab", 250, 5, "半格高的石板：台阶、屋顶、花坛的优雅选择。"),
    item!(blk "metal_b", "金属块", "metal", 250, 40, "锃亮的工业板材，科幻基地外墙。"),
    item!(blk "concrete_b", "混凝土块", "concrete", 250, 12, "素雅灰白的现代建材。"),
    item!(mach "furnace_b", "熔炉", "furnace", 50, 80, "烧炼矿石。燃料：碳/煤。"),
    item!(mach "miner_b", "自动采矿机", "miner", 50, 500, "放置在矿脉上自动开采。需电力。"),
    item!(mach "belt_b", "传送带", "belt", 200, 60, "运输物品。朝放置者视线方向传送。"),
    item!(mach "assembler_b", "装配机", "assembler", 50, 700, "自动合成部件。需电力。"),
    item!(mach "solar_b", "太阳能板", "solar", 100, 350, "白天发电 10kW。"),
    item!(mach "refinery_b", "精炼厂", "refinery", 50, 900, "精炼高级化合物。需电力。"),
    item!(mach "chest_b", "储物箱", "chest", 50, 90, "24 格储存空间。"),
    item!(mach "reactor_b", "核子反应堆", "reactor", 20, 4000, "全天候发电 100kW，消耗铀。"),
    item!(mach "launchpad_b", "发射平台", "launchpad", 10, 1500, "飞船停泊于此免耗燃料起飞。"),
    item!(mach "wind_b", "风力涡轮机", "wind", 50, 420, "全天候发电 2~16kW，海拔越高风越大。"),
    item!(mach "burner_b", "火力发电机", "burner", 50, 260, "烧煤/碳发电 25kW，工业的第一缕黑烟。"),
    item!(mach "beacon_b", "标记方块", "beacon", 20, 120, "放置后在屏幕上显示定位标记，按 E 设置名称与全星系显示。永不迷路。"),
    item!(mach "lumberbot_b", "伐木机器人", "lumberbot", 10, 320, "放置充电桩后悬浮机器人自动巡林伐木，采集碳装满后自动送往附近的收集点。"),
    item!(mach "collector_b", "收集点", "collector", 20, 110, "伐木机器人的卸货站（12格），库存自动输出到面前的传送带/机器，可直通装配机。"),
    item!(mach "medbay_b", "医疗站", "medbay", 50, 900, "站在旁边自动治疗：每消耗 1 钠 + 1 氧气回复 3 点生命。需电力。"),
];

pub fn item_by_key(key: &str) -> Option<&'static Item> {
    ITEMS.iter().find(|i| i.key == key)
}

/// 生态动物体型类型（JS BIOMES[].animal.type：strider/crab/blob）。
pub fn biome_animal_kind(biome_key: &str) -> &'static str {
    match biome_key {
        "lush" | "alien" | "fungal" | "salt" | "redmoss" | "hive" => "strider",
        "desert" | "volcanic" | "crystal" | "ashen" | "amber" | "ferrous" | "obsidian" => "crab",
        "frozen" | "ocean" | "murk" => "blob",
        _ => "strider",
    }
}

/// Fuel value in furnace burn-seconds per item.
pub fn fuel_value(item: &str) -> f32 {
    match item {
        "carbon" => 4.0,
        "coal" => 16.0,
        "planks_b" => 5.0,
        _ => 0.0,
    }
}

#[derive(Clone, Debug)]
pub struct Recipe {
    pub id: &'static str,
    pub inputs: &'static [(&'static str, i32)],
    pub output: (&'static str, i32),
    /// "hand" (craft anywhere), "furnace", "refinery"
    pub station: &'static str,
    pub time: f32,
    pub tech: Option<&'static str>,
}

macro_rules! recipe {
    ($id:expr, [$(( $in:expr, $n:expr )),*], $out:expr => $on:expr, $where:expr, $time:expr, $tech:expr) => {
        Recipe {
            id: $id,
            inputs: &[$(( $in, $n )),*],
            output: ($out, $on),
            station: $where,
            time: $time,
            tech: $tech,
        }
    };
}

pub const RECIPES: &[Recipe] = &[
    // furnace smelting
    recipe!("iron", [("iron_ore", 1)], "iron" => 1, "furnace", 2.4, None),
    recipe!("copper", [("copper_ore", 1)], "copper" => 1, "furnace", 2.4, None),
    recipe!("titanium", [("titanium_ore", 1)], "titanium" => 1, "furnace", 3.6, None),
    recipe!("gold", [("gold_ore", 1)], "gold" => 1, "furnace", 3.0, None),
    recipe!("glass", [("sand", 2)], "glass_b" => 1, "furnace", 2.0, None),
    // hand craft
    recipe!("gear", [("iron", 2)], "gear" => 1, "hand", 1.6, None),
    recipe!("wire", [("copper", 1)], "wire" => 2, "hand", 1.2, None),
    recipe!("circuit", [("wire", 3), ("iron", 1)], "circuit" => 1, "hand", 3.2, None),
    recipe!("plate", [("iron", 3), ("carbon", 2)], "plate" => 1, "hand", 2.8, None),
    recipe!("data", [("circuit", 1), ("carbon", 5)], "data" => 1, "hand", 4.0, None),
    recipe!("planks", [("carbon", 4)], "planks_b" => 4, "hand", 1.0, None),
    recipe!("lamp", [("glass_b", 2), ("wire", 1)], "lamp_b" => 2, "hand", 1.5, None),
    recipe!("slab", [("stone", 2)], "slab_b" => 4, "hand", 1.0, None),
    recipe!("metal_b", [("iron", 4)], "metal_b" => 4, "hand", 1.5, None),
    recipe!("concrete", [("stone", 2), ("sand", 2)], "concrete_b" => 4, "hand", 1.5, None),
    recipe!("furnace_b", [("stone", 12)], "furnace_b" => 1, "hand", 2.0, None),
    recipe!("beacon_b", [("iron", 4), ("glass_b", 2), ("wire", 2)], "beacon_b" => 1, "hand", 2.0, None),
    recipe!("burner_b", [("iron", 8), ("gear", 4), ("stone", 6)], "burner_b" => 1, "hand", 4.0, Some("automation")),
    recipe!("wind_b", [("iron", 6), ("gear", 4), ("circuit", 1)], "wind_b" => 1, "hand", 4.0, Some("power")),
    recipe!("chest_b", [("planks_b", 6), ("iron", 2)], "chest_b" => 1, "hand", 2.0, Some("logistics")),
    recipe!("collector_b", [("planks_b", 4), ("iron", 4)], "collector_b" => 1, "hand", 2.0, Some("logistics")),
    recipe!("lumberbot_b", [("iron", 6), ("gear", 2), ("wire", 2)], "lumberbot_b" => 1, "hand", 3.0, Some("automation")),
    recipe!("miner_b", [("iron", 10), ("gear", 4), ("circuit", 1)], "miner_b" => 1, "hand", 5.0, Some("automation")),
    recipe!("belt_b", [("iron", 2), ("gear", 1)], "belt_b" => 2, "hand", 1.4, Some("automation")),
    recipe!("solar_b", [("iron", 5), ("glass_b", 3), ("circuit", 1)], "solar_b" => 1, "hand", 4.0, Some("power")),
    recipe!("assembler_b", [("iron", 12), ("gear", 6), ("circuit", 3)], "assembler_b" => 1, "hand", 6.0, Some("assembly")),
    recipe!("refinery_b", [("iron", 10), ("copper", 6), ("circuit", 2), ("stone", 8)], "refinery_b" => 1, "hand", 6.0, Some("refining")),
    recipe!("reactor_b", [("titanium", 12), ("circuit", 8), ("plate", 4), ("uranium", 4)], "reactor_b" => 1, "hand", 12.0, Some("nuclear")),
    recipe!("launchpad_b", [("titanium", 8), ("plate", 6), ("circuit", 4)], "launchpad_b" => 1, "hand", 8.0, Some("spaceport")),
    recipe!("medbay_b", [("plate", 2), ("wire", 3), ("circuit", 1), ("glass_b", 2)], "medbay_b" => 1, "hand", 4.0, Some("power")),
    recipe!("fuel", [("carbon", 25), ("oxygen", 10)], "fuel" => 1, "hand", 8.0, None),
    recipe!("fuel2", [("coal", 15), ("oxygen", 12)], "fuel" => 2, "refinery", 9.0, Some("refining")),
    recipe!("carbon_x", [("coal", 1)], "carbon" => 3, "refinery", 1.5, None),
    recipe!("oxy_x", [("sodium", 1), ("carbon", 1)], "oxygen" => 2, "refinery", 2.0, None),
    recipe!("antimatter", [("uranium", 20), ("tritium", 100), ("circuit", 10), ("gold", 5)], "antimatter" => 1, "refinery", 30.0, Some("nuclear")),
    recipe!("warpcell", [("antimatter", 3), ("gold", 20), ("titanium", 30), ("data", 20)], "warpcell" => 1, "refinery", 60.0, Some("warp")),
    recipe!("warp_hand", [("antimatter", 4), ("gold", 25), ("titanium", 40), ("data", 25), ("fuel", 5)], "warpcell" => 1, "hand", 90.0, Some("warp")),
];

/// Recipes available at a given station; tech filter applied by caller.
pub fn recipes_for(station: &str) -> Vec<&'static Recipe> {
    RECIPES
        .iter()
        .filter(|r| r.station == station || r.station == "hand" && station == "hand")
        .collect()
}

#[derive(Clone, Debug)]
pub struct Tech {
    pub id: &'static str,
    pub name: &'static str,
    pub icon: &'static str,
    pub cost: &'static [(&'static str, i32)],
    pub time: f32,
    pub pos: (f32, f32),
    pub req: &'static [&'static str],
    pub desc: &'static str,
    pub unlocked: bool,
}

pub const TECHS: &[Tech] = &[
    Tech { id: "survival", name: "生存本能", icon: "carbon", cost: &[], time: 0.0, pos: (60.0, 380.0), req: &[], desc: "基础采集与合成。", unlocked: true },
    Tech { id: "scan1", name: "扫描增幅 I", icon: "data", cost: &[("data", 4)], time: 10.0, pos: (230.0, 200.0), req: &["survival"], desc: "矿物扫描范围 24→48 格（按 C 扫描）。", unlocked: false },
    Tech { id: "scan2", name: "扫描增幅 II", icon: "circuit", cost: &[("data", 15), ("circuit", 4)], time: 20.0, pos: (400.0, 120.0), req: &["scan1"], desc: "矿物扫描范围 48→80 格（按 C 扫描）。", unlocked: false },
    Tech { id: "metallurgy", name: "冶金学", icon: "furnace_b", cost: &[("data", 2)], time: 8.0, pos: (230.0, 380.0), req: &["survival"], desc: "解锁熔炉高效冶炼。", unlocked: false },
    Tech { id: "automation", name: "自动化", icon: "miner_b", cost: &[("data", 5)], time: 15.0, pos: (400.0, 260.0), req: &["metallurgy"], desc: "解锁自动采矿机、传送带与火力发电机。", unlocked: false },
    Tech { id: "logistics", name: "物流学", icon: "chest_b", cost: &[("data", 4)], time: 12.0, pos: (400.0, 500.0), req: &["metallurgy"], desc: "解锁储物箱与物品分流。", unlocked: false },
    Tech { id: "power", name: "清洁能源", icon: "solar_b", cost: &[("data", 8)], time: 20.0, pos: (570.0, 260.0), req: &["automation"], desc: "解锁太阳能板与风力涡轮机。", unlocked: false },
    Tech { id: "assembly", name: "装配流水线", icon: "assembler_b", cost: &[("data", 12)], time: 25.0, pos: (570.0, 440.0), req: &["automation", "logistics"], desc: "解锁装配机，自动制造部件。", unlocked: false },
    Tech { id: "refining", name: "化学精炼", icon: "refinery_b", cost: &[("data", 15)], time: 30.0, pos: (740.0, 340.0), req: &["power", "assembly"], desc: "解锁精炼厂：高效燃料与化合物。", unlocked: false },
    Tech { id: "spaceport", name: "航天工程", icon: "launchpad_b", cost: &[("data", 20), ("titanium", 10)], time: 35.0, pos: (910.0, 260.0), req: &["refining"], desc: "解锁发射平台与飞船舱位扩容。", unlocked: false },
    Tech { id: "nuclear", name: "核裂变", icon: "reactor_b", cost: &[("data", 30), ("uranium", 5)], time: 45.0, pos: (910.0, 440.0), req: &["refining"], desc: "解锁核子反应堆，能源自由！", unlocked: false },
    Tech { id: "trade_ai", name: "贸易协议", icon: "gold", cost: &[("data", 18), ("gold", 3)], time: 25.0, pos: (1080.0, 340.0), req: &["spaceport"], desc: "空间站交易价格优惠 15%。", unlocked: false },
    Tech { id: "warp", name: "曲率理论", icon: "warpcell", cost: &[("data", 60), ("tritium", 50)], time: 60.0, pos: (1250.0, 340.0), req: &["trade_ai", "nuclear"], desc: "解锁曲率电池——通往群星的船票。", unlocked: false },
];

/// Biome definition (16 biomes).
#[derive(Clone, Debug)]
pub struct Biome {
    pub key: &'static str,
    pub name: &'static str,
    pub grass: &'static str,
    pub dirt: &'static str,
    pub deep: &'static str,
    pub sky: (f32, f32, f32),
    pub fog: (f32, f32, f32),
    pub haz: Option<&'static str>, // heat|cold|toxic|rad|storm
    pub haz_name: &'static str,
    pub haz_rate: f32, // 0 when no hazard
    pub trees: f32,
    pub flowers: f32,
    pub ore_mul: f32,
    pub tint: u32,
    pub terrain: &'static str,
    pub caves: Option<&'static str>,
    pub water_tint: u32,
    pub dry: bool,
    pub lava: bool,
    pub sea_lift: i32,
    pub crystals: f32,
    pub mushroom: bool,
    pub sub: &'static [(&'static str, f32, f32)], // (ground_override, t, f) — ground "" = none
    pub animal: Option<(&'static str, u32, u32, u32, i32)>, // (name, body, legs, eye, count)
}

macro_rules! biome {
    ($key:expr, $name:expr, $grass:expr, $dirt:expr, $deep:expr, $sky:expr, $fog:expr, $haz:expr, $hazname:expr, $hazrate:expr,
     $trees:expr, $flowers:expr, $oremul:expr, $tint:expr, $terrain:expr, $caves:expr, $watertint:expr,
     $dry:expr, $lava:expr, $sealift:expr, $crystals:expr, $mushroom:expr, $sub:expr, $animal:expr) => {
        Biome {
            key: $key, name: $name, grass: $grass, dirt: $dirt, deep: $deep, sky: $sky, fog: $fog,
            haz: $haz, haz_name: $hazname, haz_rate: $hazrate, trees: $trees, flowers: $flowers,
            ore_mul: $oremul, tint: $tint, terrain: $terrain, caves: $caves, water_tint: $watertint,
            dry: $dry, lava: $lava, sea_lift: $sealift, crystals: $crystals, mushroom: $mushroom,
            sub: $sub, animal: $animal,
        }
    };
}

pub const BIOMES: &[Biome] = &[
    biome!("lush", "翠绿星球", "grass", "dirt", "stone", (0.48, 0.72, 0.95), (0.7, 0.85, 1.0), None, "", 0.0,
        0.012, 0.02, 1.0, 0x7cc44f, "continental", None, 0x3e6bd6, false, false, 0, 0.0, false,
        &[("", 1.0, 1.0), ("", 0.25, 2.2), ("murk", 0.6, 1.2)],
        Some(("草原跳羚", 0x8a9e56, 0x5e7038, 0x2a2a2a, 10))),
    biome!("desert", "灼热荒漠", "sand", "sand", "stone", (0.95, 0.75, 0.5), (0.98, 0.85, 0.65), Some("heat"), "☀ 极端高温", 1.6,
        0.001, 0.008, 1.3, 0xe0d29a, "dunes", None, 0x6db8c8, false, false, 0, 0.0, false,
        &[("", 1.0, 1.0), ("stone", 0.05, 1.6)],
        Some(("沙壳甲虫", 0xd8b878, 0xa8895a, 0x442200, 7))),
    biome!("frozen", "冰封世界", "snow", "dirt", "ice", (0.7, 0.8, 0.95), (0.85, 0.9, 1.0), Some("cold"), "❄ 酷寒", 1.4,
        0.004, 0.006, 1.2, 0xf2f6fa, "glacial", Some("ice"), 0x9fd4e8, false, false, 0, 0.0, false,
        &[("", 1.0, 1.0), ("ice", 0.1, 0.5)],
        Some(("霜绒兽", 0xdce8f0, 0xb8c8d4, 0x3399ff, 6))),
    biome!("volcanic", "熔火之地", "basalt", "basalt", "basalt", (0.5, 0.28, 0.2), (0.6, 0.4, 0.3), Some("heat"), "🌋 炽热大气", 2.2,
        0.0, 0.004, 2.0, 0x3a3a42, "volcanic", Some("lava_tubes"), 0xff6a1a, true, true, 0, 0.0, false,
        &[("", 0.0, 1.0), ("basalt", 0.0, 0.3)],
        Some(("熔壳蟹", 0x5a4038, 0xc94f1e, 0xff6600, 5))),
    biome!("alien", "异星菌境", "alien", "dirt", "stone", (0.45, 0.3, 0.6), (0.6, 0.45, 0.75), Some("toxic"), "☣ 剧毒孢子", 1.8,
        0.008, 0.03, 1.5, 0x9a5fd0, "alien", None, 0x7a4ad8, false, false, 0, 0.0, false,
        &[("", 1.0, 1.0), ("alien", 0.15, 2.5)],
        Some(("孢子爬行者", 0x9a6fd8, 0x7c4fba, 0xffd14d, 8))),
    biome!("ocean", "蔚蓝海球", "grass", "sand", "stone", (0.35, 0.62, 0.88), (0.6, 0.8, 0.95), None, "", 0.0,
        0.007, 0.014, 0.9, 0x3e8ed6, "archipelago", None, 0x2b62c8, false, false, 7, 0.0, false,
        &[("", 1.0, 1.0), ("sand", 0.8, 1.5)],
        Some(("碧波滑行兽", 0x4da6c8, 0x2e7893, 0xffffff, 8))),
    biome!("crystal", "晶簇冻土", "snow", "dirt", "ice", (0.55, 0.75, 0.85), (0.75, 0.9, 0.95), Some("cold"), "❄ 晶界酷寒", 1.7,
        0.0, 0.004, 1.4, 0x7fe8e0, "glacial", Some("geodes"), 0x8fd8e8, false, false, 0, 0.02, false,
        &[("", 0.0, 1.0), ("ice", 0.0, 0.5)],
        Some(("晶背蟹", 0xaef0ea, 0x5ec8c0, 0x0a4f6e, 5))),
    biome!("fungal", "巨菌之森", "alien", "dirt", "stone", (0.5, 0.38, 0.55), (0.68, 0.55, 0.72), Some("toxic"), "☣ 菌孢瘴气", 1.3,
        0.010, 0.02, 1.2, 0xc06fd8, "continental", None, 0x6a4a8a, false, false, 0, 0.0, true,
        &[("", 1.0, 1.0), ("murk", 0.5, 1.8)],
        Some(("菌帽跳虫", 0xd8a8e8, 0x9a5fd0, 0xff5a4e, 9))),
    biome!("ashen", "灰烬荒原", "ash", "ash", "basalt", (0.45, 0.42, 0.4), (0.6, 0.58, 0.55), Some("rad"), "☢ 辐射尘暴", 2.0,
        0.0, 0.003, 1.8, 0x8a8a8a, "flats", None, 0x9a7a5a, false, false, 0, 0.0, false,
        &[("", 0.0, 1.0), ("basalt", 0.0, 0.2)],
        Some(("灰烬潜行者", 0x6e6a66, 0x3a3a3a, 0x7dff56, 4))),
    biome!("amber", "金珀沙海", "amber", "sand", "stone", (0.92, 0.72, 0.42), (0.98, 0.85, 0.6), Some("heat"), "☀ 灼金热浪", 1.2,
        0.001, 0.006, 1.1, 0xe0a63a, "dunes", None, 0xd8b048, false, false, 0, 0.0, false,
        &[("", 1.0, 1.0), ("amber", 0.3, 1.2)],
        Some(("珀壳掘虫", 0xe8c060, 0xa87828, 0x5e3808, 6))),
    biome!("ferrous", "磁暴铁原", "rust", "rust", "basalt", (0.55, 0.4, 0.32), (0.7, 0.55, 0.45), Some("storm"), "⚡ 磁暴侵蚀", 1.5,
        0.0, 0.004, 1.6, 0xa86a4a, "shatter", None, 0x8a5a3a, false, false, 0, 0.0, false,
        &[("", 0.0, 1.0), ("rust", 0.0, 0.4)],
        Some(("磁尘甲兽", 0x8a5a3a, 0x4a4a52, 0x35e0e8, 5))),
    biome!("murk", "荧光沼泽", "murk", "dirt", "stone", (0.16, 0.3, 0.28), (0.25, 0.42, 0.38), Some("toxic"), "☣ 沼气瘴雾", 1.1,
        0.004, 0.035, 1.0, 0x2e8a72, "swamp", Some("swamp_caves"), 0x2f7a5a, false, false, 4, 0.0, true,
        &[("", 1.0, 1.0), ("murk", 0.3, 2.2)],
        Some(("沼灯浮蜓", 0x2e8a72, 0x1a5244, 0x4ee8b8, 9))),
    biome!("salt", "盐晶滩", "salt", "salt", "stone", (0.8, 0.85, 0.9), (0.92, 0.95, 0.98), None, "", 0.0,
        0.0, 0.008, 1.0, 0xe8ecf0, "flats", None, 0xcfe8f0, false, false, 0, 0.0, false,
        &[("", 0.0, 1.0), ("sand", 0.0, 0.5)],
        Some(("盐羽鹬", 0xf0f2f4, 0xc2c9ce, 0x222222, 7))),
    biome!("obsidian", "黑曜熔壁", "obsidian", "obsidian", "basalt", (0.28, 0.22, 0.35), (0.4, 0.32, 0.48), Some("heat"), "☀ 曜岩余温", 1.9,
        0.0, 0.002, 1.7, 0x2a2a35, "shatter", None, 0x4a3a6a, true, false, 0, 0.0, false,
        &[("", 0.0, 1.0), ("basalt", 0.0, 0.2)],
        Some(("曜甲蟹", 0x2a2a35, 0x6a5a9a, 0xff6600, 4))),
    biome!("redmoss", "红藓高原", "redmoss", "dirt", "stone", (0.75, 0.5, 0.42), (0.88, 0.68, 0.58), Some("cold"), "❄ 稀薄冷风", 1.1,
        0.003, 0.012, 1.15, 0xc25a48, "mesa", None, 0xb06050, false, false, 0, 0.0, false,
        &[("", 1.0, 1.0), ("redmoss", 0.4, 1.6)],
        Some(("藓原掠行者", 0xc25a48, 0x8a3a2c, 0xffe8a0, 8))),
    biome!("hive", "蜂窝穹丘", "hive", "hive", "stone", (0.85, 0.6, 0.3), (0.95, 0.75, 0.45), Some("toxic"), "☣ 信息素迷雾", 1.5,
        0.0, 0.01, 1.3, 0xd8862a, "hive", None, 0xd89830, false, false, 0, 0.0, false,
        &[("", 0.0, 1.0), ("hive", 0.0, 0.5)],
        Some(("蜂窝守卫", 0xd8862a, 0x8a5210, 0x1a1a1a, 10))),
];

pub fn biome_by_key(key: &str) -> &'static Biome {
    BIOMES.iter().find(|b| b.key == key).unwrap_or(&BIOMES[0])
}

/// Glow emissive colors (additive after fog) — world.js GLOW_EMIT.
pub fn glow_emit(block_key: &str) -> (f32, f32, f32) {
    match block_key {
        "lamp" => (0.62, 0.48, 0.24),
        "crystal" => (0.20, 0.60, 0.54),
        "glow_shroom" => (0.16, 0.55, 0.38),
        "amber" => (0.30, 0.22, 0.10),
        _ => (0.0, 0.0, 0.0),
    }
}

/// Recharge definitions (E key / panel) — CHARGE_DEFS from player.js.
/// (system, item, cost, gain)
pub const CHARGE_DEFS: &[(&str, &str, i32, f32)] = &[
    ("laser", "carbon", 3, 30.0),
    ("shield", "sodium", 2, 2.0),
    ("hp", "oxygen", 4, 2.0),
    ("o2", "oxygen", 1, 30.0),
    ("haz", "sodium", 1, 25.0),
];

/// Planet names for the world-creation screen.
pub const PLANET_NAME_POOL: &[&str] = &[
    "始源星", "赤沙", "霜白", "熔核", "紫瘴", "翠风", "赤岭", "霜穹", "灰烬", "荒星",
    "渊蓝", "绿溪", "灼岩", "冰环", "晶尘", "紫涌", "绯沙", "苍脊", "黯潮", "辉冠",
];

pub const GALAXY_PREFIX: &[&str] = &[
    "天琴", "杜鹃", "狐尾", "鲸落", "银帆", "烛龙", "雾马", "环蛇", "曙光", "霜港",
    "孤灯", "奔雷", "碎星", "拾荒", "眠沙", "赤弦", "夜莺", "枯苇", "潮汐", "洄游",
];

pub const GALAXY_SUFFIX: &[&str] = &[
    "-α", "-β", "-γ", "-δ", "-Ω", "-Ⅲ", "-Ⅶ", "-Ⅸ", "-Ⅻ", "-Prime", "-Minor", "-Deep",
];

// ==================== 交易 / 空间站 ====================

/// Goods tradeable at the station terminal (price base = ITEMS[].price).
pub const TRADE_GOODS: &[&str] = &[
    "carbon", "oxygen", "sodium", "coal", "iron_ore", "copper_ore", "titanium_ore",
    "gold_ore", "uranium", "tritium", "iron", "copper", "titanium", "gold", "gear",
    "wire", "circuit", "plate", "data", "fuel", "glass_b", "antimatter", "warpcell",
];

pub struct StationBlueprint {
    pub tech: &'static str,
    pub price: i32,
    pub name: &'static str,
}

pub const STATION_BLUEPRINTS: &[StationBlueprint] = &[
    StationBlueprint { tech: "logistics", price: 800, name: "蓝图：物流学" },
    StationBlueprint { tech: "power", price: 1500, name: "蓝图：光伏能源" },
    StationBlueprint { tech: "refining", price: 3000, name: "蓝图：化学精炼" },
    StationBlueprint { tech: "nuclear", price: 8000, name: "蓝图：核裂变" },
];

// ==================== 任务线（21 步）====================

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum QuestType {
    /// 事件（flag 置位）
    Event,
    /// 拥有 n 个物品
    Collect,
    /// 放置 n 个方块
    Place,
    /// 研究科技
    Tech,
}

#[derive(Clone, Copy)]
pub struct Quest {
    pub id: &'static str,
    pub title: &'static str,
    pub desc: &'static str,
    pub qtype: QuestType,
    pub flag: Option<&'static str>,
    pub item: Option<&'static str>,
    pub n: i32,
    pub block: Option<&'static str>,
    pub tech: Option<&'static str>,
    pub dialog: Option<&'static str>,
}

pub const QUESTS: &[Quest] = &[
    Quest { id: "q_wake", title: "苏醒", desc: "检查坠毁的飞船（靠近并按 E）", qtype: QuestType::Event, flag: Some("checkedShip"), item: None, n: 0, block: None, tech: None,
        dialog: Some("警报……船体完整性 34%。发射推进器损毁。旅行者，你需要资源来修复它。") },
    Quest { id: "q_carbon", title: "生命之碳", desc: "采集碳 ×15（挖掘树木与蕨类）", qtype: QuestType::Collect, flag: None, item: Some("carbon"), n: 15, block: None, tech: None,
        dialog: Some("激光采矿器已校准。瞄准植物长按左键。") },
    Quest { id: "q_sodium", title: "防护充能", desc: "采集钠 ×8（黄色花朵）", qtype: QuestType::Collect, flag: None, item: Some("sodium"), n: 8, block: None, tech: None,
        dialog: Some("环境防护正在耗尽，钠素花能为它充能。") },
    Quest { id: "q_stone", title: "开采岩层", desc: "采集岩石 ×12", qtype: QuestType::Collect, flag: None, item: Some("stone"), n: 12, block: None, tech: None, dialog: None },
    Quest { id: "q_furnace", title: "第一座熔炉", desc: "合成并放置一座熔炉", qtype: QuestType::Place, flag: None, item: None, n: 1, block: Some("furnace"), tech: None,
        dialog: Some("按 Tab 打开合成面板。熔炉是文明的第一束火光。") },
    Quest { id: "q_iron", title: "钢铁意志", desc: "熔炼铁锭 ×10（熔炉需要碳/煤作燃料）", qtype: QuestType::Collect, flag: None, item: Some("iron"), n: 10, block: None, tech: None, dialog: None },
    Quest { id: "q_repair", title: "修复推进器", desc: "带着铁锭×10、碳×20 检查飞船", qtype: QuestType::Event, flag: Some("shipRepaired"), item: None, n: 0, block: None, tech: None,
        dialog: Some("推进器修复完毕！但燃料罐是空的……") },
    Quest { id: "q_tech", title: "科研起步", desc: "合成研究数据 ×2 并研究「冶金学」(按 T)", qtype: QuestType::Tech, flag: None, item: None, n: 0, block: None, tech: Some("metallurgy"), dialog: None },
    Quest { id: "q_auto", title: "自动化黎明", desc: "研究「自动化」，放置自动采矿机于矿脉上", qtype: QuestType::Place, flag: None, item: None, n: 1, block: Some("miner"), tech: None,
        dialog: Some("让机器为你工作。采矿机需要电力——先研究光伏能源，或用它旁边的手摇模式（效率减半）。") },
    Quest { id: "q_belt", title: "流水线", desc: "放置传送带 ×6，把矿石送进熔炉", qtype: QuestType::Place, flag: None, item: None, n: 6, block: Some("belt"), tech: None, dialog: None },
    Quest { id: "q_power", title: "电力时代", desc: "研究「光伏能源」并放置 2 块太阳能板", qtype: QuestType::Place, flag: None, item: None, n: 2, block: Some("solar"), tech: None, dialog: None },
    Quest { id: "q_refinery", title: "化学工厂", desc: "研究「化学精炼」并放置精炼厂", qtype: QuestType::Place, flag: None, item: None, n: 1, block: Some("refinery"), tech: None, dialog: None },
    Quest { id: "q_fuel", title: "飞向天空的燃料", desc: "合成发射燃料 ×2（Tab便携合成：碳×25+氧×10，精炼厂更高效）", qtype: QuestType::Collect, flag: None, item: Some("fuel"), n: 2, block: None, tech: None,
        dialog: Some("发射燃料配方已同步：碳×25 + 氧气×10。可在背包合成面板直接合成，或交给精炼厂批量生产。") },
    Quest { id: "q_launch", title: "起飞！", desc: "为飞船加注燃料并起飞（对飞船按 E，机上再按 E 可随处降落）", qtype: QuestType::Event, flag: Some("launched"), item: None, n: 0, block: None, tech: None,
        dialog: Some("所有系统就绪。点火倒计时……祝好运，旅行者。") },
    Quest { id: "q_station", title: "轨道灯塔", desc: "持续拉升冲出大气层，飞向空间站停靠（靠近按 E）", qtype: QuestType::Event, flag: Some("docked"), item: None, n: 0, block: None, tech: None,
        dialog: Some("侦测到空间站信号。拉起机头爬升，冲出大气层就能看到它。") },
    Quest { id: "q_trade", title: "第一桶金", desc: "在空间站完成一次交易", qtype: QuestType::Event, flag: Some("traded"), item: None, n: 0, block: None, tech: None, dialog: None },
    Quest { id: "q_explore", title: "新世界", desc: "降落在另一颗星球上", qtype: QuestType::Event, flag: Some("newPlanet"), item: None, n: 0, block: None, tech: None,
        dialog: Some("每颗星球都有独特的生态与矿藏。熔火之地矿产翻倍……但小心高温。") },
    Quest { id: "q_nuclear", title: "原子之心", desc: "研究「核裂变」并建造核子反应堆", qtype: QuestType::Place, flag: None, item: None, n: 1, block: Some("reactor"), tech: None, dialog: None },
    Quest { id: "q_antimatter", title: "囚禁湮灭之光", desc: "精炼反物质 ×3（铀×20+氚×100+电路×10+金锭×5 each）", qtype: QuestType::Collect, flag: None, item: Some("antimatter"), n: 3, block: None, tech: None,
        dialog: Some("反物质——宇宙中最昂贵的物质。深挖铀矿、粉碎小行星采氚，或者用星币在空间站堆出来。") },
    Quest { id: "q_warp", title: "群星的船票", desc: "获得一枚曲率电池（精炼合成 或 空间站 ₪240000 购买）", qtype: QuestType::Collect, flag: None, item: Some("warpcell"), n: 1, block: None, tech: None,
        dialog: Some("曲率电池充能完毕。打开星系地图（太空中按 M），选一颗你喜欢的恒星。") },
    Quest { id: "q_leave", title: "第一章 · 飞出初始星系", desc: "在星系地图（M）中选择目标星系，执行曲速跃迁", qtype: QuestType::Event, flag: Some("warpedOut"), item: None, n: 0, block: None, tech: None,
        dialog: Some("跃迁成功——起源星系在身后化为一粒尘埃。第一章完结，而宇宙没有边界。旅行者，继续前进吧。") },
];

pub fn quest_by_id(id: &str) -> &'static Quest {
    QUESTS.iter().find(|q| q.id == id).unwrap_or(&QUESTS[0])
}

// ==================== 星系 ====================

pub const HOME_GALAXY_SEED: u32 = 7777;
pub const DEFAULT_STATION: [f32; 3] = [700.0, 200.0, -500.0];

#[derive(Clone, Debug)]
pub struct PlanetDef {
    pub id: usize,
    pub biome: &'static str,
    pub name: &'static str,
    pub pos: [f32; 3],
    pub radius: f32,
}

/// 初始星系（固定布局，每档案随机种子着色）
pub const DEFAULT_PLANETS: &[PlanetDef] = &[
    PlanetDef { id: 0, biome: "lush", name: "始源星", pos: [0.0, 0.0, 0.0], radius: 150.0 },
    PlanetDef { id: 1, biome: "desert", name: "赤沙", pos: [1800.0, 120.0, -900.0], radius: 130.0 },
    PlanetDef { id: 2, biome: "frozen", name: "霜白", pos: [-1500.0, -200.0, -1700.0], radius: 140.0 },
    PlanetDef { id: 3, biome: "volcanic", name: "熔核", pos: [900.0, -100.0, 2300.0], radius: 120.0 },
    PlanetDef { id: 4, biome: "alien", name: "紫瘴", pos: [-2400.0, 250.0, 1100.0], radius: 145.0 },
];

#[derive(Clone, Debug)]
pub struct Galaxy {
    pub seed: u32,
    pub name: String,
    pub planets: Vec<PlanetDef>,
    pub station: [f32; 3],
    /// item → price multiplier
    pub market: std::collections::HashMap<String, f32>,
}

pub fn galaxy_name(seed: u32) -> String {
    if seed == HOME_GALAXY_SEED {
        return "起源星系".to_string();
    }
    let mut rnd = crate::rng::Rng::new(seed ^ 0x6A09_E667);
    format!(
        "{}{}",
        GALAXY_PREFIX[rnd.range(GALAXY_PREFIX.len())],
        GALAXY_SUFFIX[rnd.range(GALAXY_SUFFIX.len())]
    )
}

fn home_market() -> std::collections::HashMap<String, f32> {
    let mut rnd = crate::rng::Rng::new(HOME_GALAXY_SEED);
    let mut m = std::collections::HashMap::new();
    for g in TRADE_GOODS {
        m.insert(g.to_string(), 0.75 + rnd.next() * 0.5);
    }
    m
}

pub fn home_galaxy() -> Galaxy {
    Galaxy {
        seed: HOME_GALAXY_SEED,
        name: galaxy_name(HOME_GALAXY_SEED),
        planets: DEFAULT_PLANETS.to_vec(),
        station: DEFAULT_STATION,
        market: home_market(),
    }
}

/// 生成随机星系（纯函数，seed 决定内容，与 data.js generateGalaxy 一致）
pub fn generate_galaxy(seed: u32) -> Galaxy {
    let mut rnd = crate::rng::Rng::new(seed);
    let biome_pool = [
        "lush", "desert", "frozen", "volcanic", "alien", "ocean", "crystal", "fungal", "ashen",
        "amber", "ferrous", "murk", "salt", "obsidian", "redmoss", "hive",
    ];
    let names = [
        "翠风", "赤岭", "霜穹", "灰烬", "荒星", "渊蓝", "绿溪", "灼岩", "冰环", "晶尘",
        "紫涌", "绯沙", "苍脊", "黯潮", "辉冠", "裂星", "流火", "雾原", "雪锋", "熔渊",
        "澜礁", "菌歌", "空悬", "曜壁", "沉塔", "洄湾", "铁穗", "昙丘", "烬柱", "虹隙",
    ];
    let mut used: Vec<usize> = Vec::new();
    let mut planets: Vec<PlanetDef> = Vec::new();
    let count = 4 + rnd.range(4); // 4~7
    for i in 0..count {
        let mut n = rnd.range(names.len());
        while used.contains(&n) {
            n = rnd.range(names.len());
        }
        used.push(n);
        let b = biome_pool[rnd.range(biome_pool.len())];
        let ang = i as f32 / count as f32 * std::f32::consts::PI * 2.0 + rnd.next() * 0.8;
        let dist = 800.0 + rnd.next() * 2400.0;
        let el = (rnd.next() - 0.5) * 700.0;
        planets.push(PlanetDef {
            id: i,
            biome: b,
            name: names[n],
            pos: [ang.cos() * dist, el, ang.sin() * dist],
            radius: 105.0 + rnd.next() * 70.0,
        });
    }
    // 保证至少一颗富碳星球
    if !planets
        .iter()
        .any(|p| matches!(p.biome, "lush" | "ocean" | "fungal" | "alien"))
    {
        let pick = ["lush", "ocean", "fungal"][rnd.range(3)];
        planets[0].biome = pick;
    }
    // 空间站：与所有星球保持安全分离
    const STAT_CLEAR: f32 = 230.0;
    let mut stat = [0.0, 900.0, 0.0];
    for _ in 0..200 {
        let cand = [
            1200.0 * (rnd.next() - 0.5),
            300.0 + rnd.next() * 400.0,
            1200.0 * (rnd.next() - 0.5),
        ];
        let mut ok = true;
        for p in &planets {
            let dx = cand[0] - p.pos[0];
            let dy = cand[1] - p.pos[1];
            let dz = cand[2] - p.pos[2];
            if dx * dx + dy * dy + dz * dz < (p.radius + STAT_CLEAR) * (p.radius + STAT_CLEAR) {
                ok = false;
                break;
            }
        }
        if ok {
            stat = cand;
            break;
        }
    }
    let mut market = std::collections::HashMap::new();
    for g in TRADE_GOODS {
        market.insert(g.to_string(), 0.75 + rnd.next() * 0.5);
    }
    Galaxy {
        seed,
        name: galaxy_name(seed),
        planets,
        station: stat,
        market,
    }
}

// ==================== 飞船等级体系 ====================

pub struct ShipClass {
    pub key: &'static str,
    pub weight: f32,
    pub price: i32,
    pub weapon: &'static str,
    pub weapon_name: &'static str,
    pub slots: usize,
    pub color: &'static str,
}

pub const SHIP_CLASSES: &[ShipClass] = &[
    ShipClass { key: "C", weight: 0.55, price: 45000, weapon: "pulse", weapon_name: "脉冲机炮", slots: 12, color: "#9aa6b2" },
    ShipClass { key: "B", weight: 0.25, price: 140000, weapon: "twin", weapon_name: "双联流火炮", slots: 16, color: "#35e0e8" },
    ShipClass { key: "A", weight: 0.15, price: 350000, weapon: "phase", weapon_name: "相位光矛", slots: 20, color: "#b58aff" },
    ShipClass { key: "S", weight: 0.05, price: 900000, weapon: "annihil", weapon_name: "湮灭重炮", slots: 24, color: "#ffd94d" },
];

pub const SHIP_MODEL_NAMES: &[(&str, &str)] = &[
    ("ship_striker", "掠袭者"),
    ("ship_dispatcher", "调度者"),
    ("ship_insurgent", "叛徒"),
    ("ship", "拓荒矿船"),
];

/// NMS 式按权重掷等级。
pub fn roll_ship_class(r: f32) -> &'static ShipClass {
    let mut acc = 0.0;
    for c in SHIP_CLASSES {
        acc += c.weight;
        if r < acc {
            return c;
        }
    }
    &SHIP_CLASSES[0]
}

pub fn ship_class_by_key(key: &str) -> &'static ShipClass {
    SHIP_CLASSES
        .iter()
        .find(|c| c.key == key)
        .unwrap_or(&SHIP_CLASSES[0])
}

pub const PILOT_NAMES: &[&str] = &[
    "游商·卡洛", "飞手·薇拉", "老练的走私客", "星途旅人·顿", "佣兵·赤羽", "货运队长·穆",
];

/// 交易计算：买 = max(1, round(price*mod*1.25*discount))，卖 = max(1, round(price*mod*0.8))，discount=0.85 有 trade_ai。
pub fn trade_buy_price(item: &str, market: &std::collections::HashMap<String, f32>, has_trade_ai: bool) -> i32 {
    let base = item_by_key(item).map(|i| i.price).unwrap_or(1) as f32;
    let mult = market.get(item).copied().unwrap_or(1.0);
    let discount = if has_trade_ai { 0.85 } else { 1.0 };
    ((base * mult * 1.25 * discount).round() as i32).max(1)
}

pub fn trade_sell_price(item: &str, market: &std::collections::HashMap<String, f32>) -> i32 {
    let base = item_by_key(item).map(|i| i.price).unwrap_or(1) as f32;
    let mult = market.get(item).copied().unwrap_or(1.0);
    ((base * mult * 0.8).round() as i32).max(1)
}
