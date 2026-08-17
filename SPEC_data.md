# STARFORGE `js/data.js` — Complete Technical Specification for Rust Port

> Repository note (2026-08-17): the legacy source referenced by this specification is archived at `legacy-web/js/data.js`.

Source: `legacy-web/js/data.js` (431 lines). Header comment: *"方块 / 物品 / 配方 / 科技树 / 任务 / 星球生态 定义"* (Blocks / Items / Recipes / Tech tree / Quests / Planet ecology definitions). The file is a plain `'use strict'` ES module-style script exporting globals (`const`/`let`). All objects are keyed string maps, not arrays.

## ⚠️ Important scope notes (do not assume missing data)

1. **There is NO `TOOLS` definition in this file.** There are no tool objects with mining power / speed / damage / durability anywhere in `data.js`. The only mining-related numeric is the block field `hard` (dig time in seconds). If the game has tools, they live in another file — they are **not** here. Do not invent them.
2. **`mulberry32` is referenced but NOT defined in this file** (used at lines 373, 377). Its definition lives elsewhere. I document every call site and its exact usage, and give the canonical reference algorithm (flagged as "verify at source").
3. No explicit "light level", "color", "slot", "fuel", "tier", or "effect" numeric fields exist on blocks/items. I document every field that *does* exist and mark which requested fields are absent.

---

## 1. Data structure / shape

### 1.1 Block entry (`BLOCKS`)

Every key in `BLOCKS` is the block's string id. Each entry has:

| Field | Type | Required? | Meaning |
|---|---|---|---|
| `id` | integer | yes | Unique numeric id (0–59, with gaps 25–29 reserved) |
| `name` | string | yes | Chinese display name |
| `solid` | boolean | **no — defaults `true`** | Whether the block is collidable/solid. Post-loop sets `solid = true` if `undefined`. Explicitly `false` on `air`, `water`, and all `cross` plants. |
| `hard` | float \| `Infinity` | optional | Mining/dig time in **seconds**. `Infinity` = unbreakable. Absent on `air` and `water`. |
| `tiles` | object | optional | Texture binding map. Keys are face selectors: `all` (every face), `top`, `side`, `bottom`, `front`. Values are texture/atlas names (strings). `all` + `front` may co-occur (e.g. furnace). |
| `drops` | array | optional | Loot table: array of `{ item: string, n: int, chance?: float }`. `chance` default = 1.0 (always). `item` = item id string. |
| `transparent` | boolean | optional | Transparent (non-opaque) rendering/occlusion flag |
| `fancy` | boolean | optional | Fancy foliage flag (only `leaves`) |
| `cross` | boolean | optional | Cross-shaped plant sprite (two crossed quads); always `solid:false` |
| `liquid` | boolean | optional | Liquid block (only `water`) |
| `glow` | boolean | optional | Emits light (lamp, crystal, amber, glow_shroom) |
| `ore` | boolean | optional | Ore-block marker (used by scanner/miner) |
| `machine` | string | optional | Factory-machine type id. Visual/logic delegated to `factory.js`; invisible in block grid but has collision. Values: `furnace`, `miner`, `belt`, `assembler`, `solar`, `refinery`, `chest`, `reactor`, `launchpad`, `wind`, `burner`, `beacon`, `lumberbot`, `collector`, `medbay` |
| `lowbox` | boolean \| float | optional | Low-height collision box. `true` = flat/low (belt, solar, launchpad). Numeric `0.45` = 45% height (slab). |
| `key` | string | **added by loop** | Set to the object key (string id) |

Post-processing (line 71): `for (const k in BLOCKS){ BLOCKS[k].key = k; BLOCK_BY_ID[BLOCKS[k].id] = BLOCKS[k]; if (BLOCKS[k].solid === undefined) BLOCKS[k].solid = true; }` — i.e. a reverse index `BLOCK_BY_ID` (numeric id → block) is also produced, and `solid` defaults to `true`.

Header comment meanings: `hard` = dig seconds; `drops: [{item,n,chance}]`; `cross` = cross plant sprite; `machine` = factory machine (visual handled by factory.js, invisible in grid but has collision).

### 1.2 Item entry (`ITEMS`)

Every key is the item's string id. Fields:

| Field | Type | Required? | Meaning |
|---|---|---|---|
| `name` | string | yes | Chinese display name |
| `cat` | string | yes | Category: `res` (resource), `mat` (material), `blk` (placeable block), `mach` (machine), `tool` (special — declared in comment but **no `tool` item exists in data**) |
| `iconFn` | string | *either* | Icon renderer function name (used by `res`/`mat` items) |
| `iconBlock` | string | *either* | Icon is derived from this block's texture (used by `blk`/`mach` items) |
| `block` | string | optional | Block id placed when item is used (only `blk`/`mach`) |
| `stack` | int | **no — defaults 250** | Max stack size |
| `price` | int | yes | Star-coin (₪) price |
| `desc` | string | yes | Flavor/function description (Chinese) |
| `id` | string | **added by loop** | Set to the object key |

Post-processing (line 127): `for (const k in ITEMS){ ITEMS[k].id = k; if (!ITEMS[k].stack) ITEMS[k].stack = 250; }`.

### 1.3 Recipe entry (`RECIPES`) — an array, not a map

| Field | Type | Meaning |
|---|---|---|
| `id` | string | Unique recipe id |
| `out` | `{ itemId: count }` | Exactly one output key (value = count) |
| `in` | `{ itemId: count, ... }` | Inputs, one or more keys |
| `where` | string | Crafting station: `'furnace'`, `'refinery'`, or `'both'` (portable hand-craft **and** assembler). Header comment names `hand`/`furnace`/`assembler`/`refinery`, but the literal values used in data are only `furnace`, `refinery`, `both`. |
| `time` | float | Craft time in **seconds** |
| `tech` | string? | Optional tech requirement (recipe hidden/locked until researched) |

`RECIPE_BY_ID` map is built at line 176.

### 1.4 Tech entry (`TECH`)

| Field | Type | Meaning |
|---|---|---|
| `name` | string | Display name |
| `icon` | string | Icon reference (item id or block id, e.g. `'carbon'`, `'furnace_b'`) |
| `cost` | `{ itemId: count }` | Research cost (may be `{}` for free) |
| `time` | float | Research time in **seconds** |
| `pos` | `[x, y]` | Position in tech-tree UI (pixels) |
| `desc` | string | Description (Chinese) |
| `unlocked` | bool? | Only `survival` has `unlocked: true` (start tech) |
| `req` | string[] | Prerequisite tech ids |
| `id` | string | **added by loop** (`TECH[k].id = k`) |

---

## 2. Complete block inventory (55 blocks)

Numeric ids have a gap: 24 (barrier) → 30 (furnace). IDs 25–29 unused.

### 2.1 Natural terrain & basic blocks (ids 0–24)

| # | key | id | name | hard (s) | solid | transparent | tiles | drops | flags |
|---|---|---|---|---|---|---|---|---|---|
| 1 | `air` | 0 | 空气 (Air) | — | **false** | — | — | — | — |
| 2 | `grass` | 1 | 草方块 (Grass) | 0.75 | true | — | top:`grass_top`, side:`grass_side`, bottom:`dirt` | dirt ×1 | — |
| 3 | `dirt` | 2 | 泥土 (Dirt) | 0.7 | true | — | all:`dirt` | dirt ×1 | — |
| 4 | `stone` | 3 | 岩石 (Stone) | 1.6 | true | — | all:`stone` | stone ×1 | — |
| 5 | `sand` | 4 | 沙 (Sand) | 0.6 | true | — | all:`sand` | sand ×1 | — |
| 6 | `log` | 5 | 碳质木干 (Carbon Log) | 1.1 | true | — | top:`log_top`, side:`log_side`, bottom:`log_top` | carbon ×3 | — |
| 7 | `leaves` | 6 | 叶簇 (Leaves) | 0.3 | true | true | all:`leaves` | carbon ×1; oxygen ×1 (0.35) | fancy:true |
| 8 | `coal_ore` | 7 | 煤矿脉 (Coal Ore) | 2.2 | true | — | all:`coal_ore` | coal ×1; coal ×1 (0.3) | ore:true |
| 9 | `iron_ore` | 8 | 铁矿脉 (Iron Ore) | 2.6 | true | — | all:`iron_ore` | iron_ore ×1 | ore:true |
| 10 | `copper_ore` | 9 | 铜矿脉 (Copper Ore) | 2.6 | true | — | all:`copper_ore` | copper_ore ×1 | ore:true |
| 11 | `titanium_ore` | 10 | 钛矿脉 (Titanium Ore) | 3.6 | true | — | all:`titanium_ore` | titanium_ore ×1 | ore:true |
| 12 | `uranium_ore` | 11 | 铀矿脉 (Uranium Ore) | 4.2 | true | — | all:`uranium_ore` | uranium ×1 | ore:true |
| 13 | `gold_ore` | 12 | 金矿脉 (Gold Ore) | 3.0 | true | — | all:`gold_ore` | gold_ore ×1 | ore:true |
| 14 | `sodium_plant` | 13 | 钠素花 (Sodium Flower) | 0.05 | **false** | — | all:`sodium_plant` | sodium ×2 | cross:true |
| 15 | `oxygen_plant` | 14 | 氧素花 (Oxygen Flower) | 0.05 | **false** | — | all:`oxygen_plant` | oxygen ×2 | cross:true |
| 16 | `fern` | 15 | 碳蕨 (Carbon Fern) | 0.05 | **false** | — | all:`carbon_fern` | carbon ×1 | cross:true |
| 17 | `water` | 16 | 水 (Water) | — | **false** | true | all:`water` | — | liquid:true |
| 18 | `planks` | 17 | 碳板 (Planks) | 0.9 | true | — | all:`planks` | planks_b ×1 | — |
| 19 | `glass` | 18 | 玻璃 (Glass) | 0.4 | true | true | all:`glass` | glass_b ×1 | — |
| 20 | `lamp` | 19 | 光源方块 (Light Block) | 0.5 | true | — | all:`lamp_on` | lamp_b ×1 | glow:true |
| 21 | `ice` | 20 | 永冻冰 (Permafrost Ice) | 1.2 | true | — | all:`ice` | stone ×1 | — |
| 22 | `snow` | 21 | 雪被层 (Snow Layer) | 0.7 | true | — | top:`snow_top`, side:`snow_side`, bottom:`dirt` | dirt ×1 | — |
| 23 | `basalt` | 22 | 玄武岩 (Basalt) | 2.0 | true | — | all:`basalt` | stone ×1; coal ×1 (0.15) | — |
| 24 | `alien` | 23 | 荧紫菌毯 (Glowing Purple Mycelium) | 0.75 | true | — | top:`alien_top`, side:`alien_side`, bottom:`dirt` | dirt ×1; sodium ×1 (0.2) | — |
| 25 | `barrier` | 24 | 致密基岩 (Dense Bedrock) | **Infinity** | true | — | all:`barrier` | — | unbreakable |

### 2.2 Machines (ids 30–40)

| # | key | id | name | hard | machine | tiles | drops | lowbox |
|---|---|---|---|---|---|---|---|---|
| 26 | `furnace` | 30 | 熔炉 (Furnace) | 1.2 | furnace | all:`stone`, front:`furnace_front` | furnace_b ×1 | — |
| 27 | `miner` | 31 | 自动采矿机 (Auto Miner) | 1.2 | miner | all:`metal`, top:`miner_top` | miner_b ×1 | — |
| 28 | `belt` | 32 | 传送带 (Conveyor Belt) | 0.5 | belt | all:`belt` | belt_b ×1 | true |
| 29 | `assembler` | 33 | 装配机 (Assembler) | 1.4 | assembler | all:`metal`, top:`assembler_top` | assembler_b ×1 | — |
| 30 | `solar` | 34 | 太阳能板 (Solar Panel) | 0.8 | solar | all:`solar_top` | solar_b ×1 | true |
| 31 | `refinery` | 35 | 精炼厂 (Refinery) | 1.6 | refinery | all:`refinery_side` | refinery_b ×1 | — |
| 32 | `chest` | 36 | 储物箱 (Storage Chest) | 0.9 | chest | all:`chest_side`, top:`storage_top` | chest_b ×1 | — |
| 33 | `reactor` | 37 | 核子反应堆 (Nuclear Reactor) | 2.4 | reactor | all:`reactor_side` | reactor_b ×1 | — |
| 34 | `launchpad` | 38 | 发射平台 (Launch Pad) | 2.0 | launchpad | all:`launchpad_top` | launchpad_b ×1 | true |
| 35 | `wind` | 39 | 风力涡轮机 (Wind Turbine) | 1.0 | wind | all:`metal` | wind_b ×1 | — |
| 36 | `burner` | 40 | 火力发电机 (Burner Generator) | 1.2 | burner | all:`metal_dark`, front:`furnace_front` | burner_b ×1 | — |

All machines above are `solid: true` (default) and have no `transparent`/`glow`.

### 2.3 New planet blocks (ids 41–52)

| # | key | id | name | hard | solid | tiles | drops | flags |
|---|---|---|---|---|---|---|---|---|
| 37 | `crystal` | 41 | 氚晶簇 (Tritium Crystal) | 1.8 | true | all:`crystal` | tritium ×2; tritium ×2 (0.5) | glow:true |
| 38 | `mush_stem` | 42 | 巨菌柄 (Giant Mushroom Stem) | 0.8 | true | all:`mush_stem` | carbon ×2 | — |
| 39 | `mush_cap` | 43 | 巨菌盖 (Giant Mushroom Cap) | 0.5 | true | all:`mush_cap` | carbon ×1; oxygen ×1 (0.4); sodium ×1 (0.2) | — |
| 40 | `ash` | 44 | 灰烬土 (Ash Soil) | 0.8 | true | all:`ash` | dirt ×1; coal ×1 (0.12) | — |
| 41 | `amber` | 45 | 金珀岩 (Amber Rock) | 1.4 | true | all:`amber` | carbon ×2; gold_ore ×1 (0.08) | glow:true |
| 42 | `rust` | 46 | 锈蚀铁壤 (Rusty Iron Soil) | 1.0 | true | all:`rust` | dirt ×1; iron_ore ×1 (0.25) | — |
| 43 | `salt` | 47 | 盐晶块 (Salt Crystal) | 0.7 | true | all:`salt` | sodium ×1; sodium ×1 (0.4) | — |
| 44 | `obsidian` | 48 | 黑曜岩 (Obsidian) | 2.6 | true | all:`obsidian` | stone ×1; titanium_ore ×1 (0.1) | — |
| 45 | `redmoss` | 49 | 红藓被 (Red Moss Cover) | 0.75 | true | top:`redmoss_top`, side:`redmoss_side`, bottom:`dirt` | dirt ×1; carbon ×1 (0.25) | — |
| 46 | `hive` | 50 | 蜂窝晶壁 (Hive Crystal Wall) | 1.1 | true | all:`hive` | dirt ×1; carbon ×1 (0.35) | — |
| 47 | `murk` | 51 | 荧沼菌毯 (Glowing Swamp Mycelium) | 0.75 | true | top:`murk_top`, side:`murk_side`, bottom:`dirt` | dirt ×1; oxygen ×1 (0.15) | — |
| 48 | `glow_shroom` | 52 | 荧光蕈 (Glowing Mushroom) | 0.05 | **false** | all:`glow_shroom` | oxygen ×2; sodium ×1 (0.5) | cross:true, glow:true |

### 2.4 More machines & decorative blocks (ids 53–59)

| # | key | id | name | hard | machine | tiles | drops | lowbox / flags |
|---|---|---|---|---|---|---|---|---|
| 49 | `beacon` | 53 | 标记方块 (Beacon/Marker) | 0.8 | beacon | all:`metal_dark`, top:`lamp_on` | beacon_b ×1 | — |
| 50 | `lumberbot` | 54 | 伐木机器人 (Lumber Bot) | 1.0 | lumberbot | all:`vent`, top:`metal_dark` | lumberbot_b ×1 | — |
| 51 | `collector` | 55 | 收集点 (Collection Point) | 0.9 | collector | all:`chest_side`, top:`storage_top` | collector_b ×1 | — |
| 52 | `medbay` | 56 | 医疗站 (Medbay) | 1.4 | medbay | all:`metal_dark`, top:`medbay_top` | medbay_b ×1 | — |
| 53 | `slab` | 57 | 石半砖 (Stone Half-Slab) | 1.0 | — | all:`slab` | slab_b ×1 | lowbox: **0.45** |
| 54 | `metal` | 58 | 金属块 (Metal Block) | 2.0 | — | all:`metal` | metal_b ×1 | — |
| 55 | `concrete` | 59 | 混凝土块 (Concrete Block) | 1.6 | — | all:`concrete` | concrete_b ×1 | — |

`beacon`/`lumberbot`/`collector`/`medbay` are `solid:true` (default). `slab`/`metal`/`concrete` are decorative building blocks.

---

## 3. Tools

**None.** No `TOOLS` object, no tool items, and no mining-power / speed / damage / durability values exist in `data.js`. The game's only mining stat in this file is the block-level `hard` (seconds to dig). If tools are defined elsewhere, they are out of scope for this file.

---

## 4. Complete item inventory (46 items)

Columns: key (id), name, cat, icon source, block, stack, price.

### 4.1 Elemental resources (`cat: res`)

| id | name | iconFn | stack | price | desc |
|---|---|---|---|---|---|
| `carbon` | 碳 (Carbon) | carbon | 250 | 4 | 一切有机物的基础，也是基础燃料。 |
| `oxygen` | 氧气 (Oxygen) | oxygen | 250 | 6 | 为生命维持系统充能。 |
| `sodium` | 钠 (Sodium) | sodium | 250 | 8 | 为危险防护装置充能。 |
| `coal` | 煤 (Coal) | coal | 250 | 10 | 高能燃料，熔炉的最爱。 |
| `iron_ore` | 铁矿石 (Iron Ore) | iron_ore | 250 | 8 | 需熔炼成铁锭。 |
| `copper_ore` | 铜矿石 (Copper Ore) | copper_ore | 250 | 8 | 需熔炼成铜锭。 |
| `titanium_ore` | 钛矿石 (Titanium Ore) | titanium_ore | 250 | 24 | 稀有轻金属矿。 |
| `gold_ore` | 金矿石 (Gold Ore) | gold_ore | 250 | 40 | 闪闪发光，星站高价收购。 |
| `uranium` | 铀-235 (Uranium-235) | uranium | **100** | 60 | 微微发热…核反应堆燃料。 |
| `tritium` | 氚 (Tritium) | tritium | **500** | 12 | 脉冲引擎燃料，击碎小行星获取。 |

### 4.2 Processed materials (`cat: mat`)

| id | name | iconFn | stack | price | desc |
|---|---|---|---|---|---|
| `iron` | 铁锭 (Iron Ingot) | iron | 250 | 18 | 工业的骨架。 |
| `copper` | 铜锭 (Copper Ingot) | copper | 250 | 18 | 导电材料。 |
| `titanium` | 钛锭 (Titanium Ingot) | titanium | 250 | 55 | 航天级合金。 |
| `gold` | 金锭 (Gold Ingot) | gold | 250 | 90 | 贵金属，硬通货。 |
| `gear` | 齿轮 (Gear) | gear | 250 | 42 | 机械传动核心。 |
| `wire` | 铜线圈 (Copper Coil) | wire | 250 | 24 | 缠绕的铜线。 |
| `circuit` | 电路板 (Circuit Board) | circuit | **200** | 110 | 所有智能机器的大脑。 |
| `plate` | 装甲板 (Armor Plate) | plate | **200** | 60 | 飞船与机器的外壳。 |
| `data` | 研究数据 (Research Data) | data | **500** | 150 | 科技矩阵的解锁密钥。 |
| `fuel` | 发射燃料 (Launch Fuel) | fuel | **20** | 320 | 让飞船挣脱引力的怒吼。 |
| `antimatter` | 反物质 (Antimatter) | antimatter | **10** | 45000 | 被磁场囚禁的湮灭之光——曲率引擎的心脏。 |
| `warpcell` | 曲率电池 (Warp Cell) | warp | **10** | 240000 | 跨星系跃迁的船票。第一章的终点，自由的起点。 |

### 4.3 Placeable block items (`cat: blk`)

| id | name | iconBlock | block | stack | price | desc |
|---|---|---|---|---|---|---|
| `dirt` | 泥土 (Dirt) | dirt | dirt | 250 | 1 | 朴实无华的土。 |
| `stone` | 岩石 (Stone) | stone | stone | 250 | 2 | 基础建材，可烧炼加工。 |
| `sand` | 沙 (Sand) | sand | sand | 250 | 2 | 可烧制成玻璃。 |
| `planks_b` | 碳板块 (Planks) | planks | planks | 250 | 6 | 压缩碳建材。 |
| `glass_b` | 玻璃 (Glass) | glass | glass | 250 | 12 | 透明建材。 |
| `lamp_b` | 光源方块 (Light Block) | lamp | lamp | **100** | 30 | 照亮黑夜。 |
| `slab_b` | 石半砖 (Stone Half-Slab) | slab | slab | 250 | 5 | 半格高的石板：台阶、屋顶、花坛的优雅选择。 |
| `metal_b` | 金属块 (Metal Block) | metal | metal | 250 | 40 | 锃亮的工业板材，科幻基地外墙。 |
| `concrete_b` | 混凝土块 (Concrete Block) | concrete | concrete | 250 | 12 | 素雅灰白的现代建材。 |

### 4.4 Machine items (`cat: mach`)

| id | name | iconBlock | block | stack | price | desc (function notes) |
|---|---|---|---|---|---|---|
| `furnace_b` | 熔炉 (Furnace) | furnace | furnace | 50 | 80 | 烧炼矿石。燃料：碳/煤。 |
| `miner_b` | 自动采矿机 (Auto Miner) | miner | miner | 50 | 500 | 放置在矿脉上自动开采。需电力。 |
| `belt_b` | 传送带 (Conveyor Belt) | belt | belt | **200** | 60 | 运输物品。朝放置者视线方向传送。 |
| `assembler_b` | 装配机 (Assembler) | assembler | assembler | 50 | 700 | 自动合成部件。需电力。 |
| `solar_b` | 太阳能板 (Solar Panel) | solar | solar | **100** | 350 | 白天发电 **10kW**。 |
| `refinery_b` | 精炼厂 (Refinery) | refinery | refinery | 50 | 900 | 精炼高级化合物。需电力。 |
| `chest_b` | 储物箱 (Storage Chest) | chest | chest | 50 | 90 | **24 格**储存空间。 |
| `reactor_b` | 核子反应堆 (Nuclear Reactor) | reactor | reactor | **20** | 4000 | 全天候发电 **100kW**，消耗铀。 |
| `launchpad_b` | 发射平台 (Launch Pad) | launchpad | launchpad | **10** | 1500 | 飞船停泊于此免耗燃料起飞。 |
| `wind_b` | 风力涡轮机 (Wind Turbine) | wind | wind | 50 | 420 | 全天候发电 **2~16kW**，海拔越高风越大。 |
| `burner_b` | 火力发电机 (Burner Generator) | burner | burner | 50 | 260 | 烧煤/碳发电 **25kW**，工业的第一缕黑烟。 |
| `beacon_b` | 标记方块 (Beacon) | beacon | beacon | **20** | 120 | 放置后显示定位标记，按 E 设置名称与全星系显示。 |
| `lumberbot_b` | 伐木机器人 (Lumber Bot) | lumberbot | lumberbot | **10** | 320 | 放置充电桩后自动巡林伐木，采满碳后送往收集点。 |
| `collector_b` | 收集点 (Collection Point) | collector | collector | **20** | 110 | 伐木机器人卸货站（**12格**），自动输出到面前传送带/机器，可直通装配机。 |
| `medbay_b` | 医疗站 (Medbay) | medbay | medbay | 50 | 900 | 站旁边自动治疗：每消耗 **1 钠 + 1 氧气回复 3 点生命**。需电力。 |

---

## 5. Complete recipe list (37 recipes)

`where` values: `furnace` (smelter only), `refinery` (refinery only), `both` (portable hand-craft AND assembler). `tech` = gating tech id.

### 5.1 Furnace recipes

| id | inputs | output | time (s) | tech |
|---|---|---|---|---|
| `iron` | iron_ore ×1 | iron ×1 | 2.4 | — |
| `copper` | copper_ore ×1 | copper ×1 | 2.4 | — |
| `titanium` | titanium_ore ×1 | titanium ×1 | 3.6 | — |
| `gold` | gold_ore ×1 | gold ×1 | 3.0 | — |
| `glass_b` | sand ×2 | glass_b ×1 | 2.0 | — |

### 5.2 Hand + assembler (`both`) — components & building blocks

| id | inputs | output | time (s) | tech |
|---|---|---|---|---|
| `gear` | iron ×2 | gear ×1 | 1.6 | — |
| `wire` | copper ×1 | wire ×2 | 1.2 | — |
| `circuit` | wire ×3, iron ×1 | circuit ×1 | 3.2 | — |
| `plate` | iron ×3, carbon ×2 | plate ×1 | 2.8 | — |
| `data` | circuit ×1, carbon ×5 | data ×1 | 4.0 | — |
| `planks_b` | carbon ×4 | planks_b ×4 | 1.0 | — |
| `lamp_b` | glass_b ×2, wire ×1 | lamp_b ×2 | 1.5 | — |
| `slab_b` | stone ×2 | slab_b ×4 | 1.0 | — |
| `metal_b` | iron ×4 | metal_b ×4 | 1.5 | — |
| `concrete_b` | stone ×2, sand ×2 | concrete_b ×4 | 1.5 | — |

### 5.3 Machine recipes (`both`) — tech-gated

| id | inputs | output | time (s) | tech |
|---|---|---|---|---|
| `furnace_b` | stone ×12 | furnace_b ×1 | 2.0 | — |
| `beacon_b` | iron ×4, glass_b ×2, wire ×2 | beacon_b ×1 | 2.0 | — |
| `burner_b` | iron ×8, gear ×4, stone ×6 | burner_b ×1 | 4.0 | automation |
| `wind_b` | iron ×6, gear ×4, circuit ×1 | wind_b ×1 | 4.0 | power |
| `chest_b` | planks_b ×6, iron ×2 | chest_b ×1 | 2.0 | logistics |
| `collector_b` | planks_b ×4, iron ×4 | collector_b ×1 | 2.0 | logistics |
| `lumberbot_b` | iron ×6, gear ×2, wire ×2 | lumberbot_b ×1 | 3.0 | automation |
| `miner_b` | iron ×10, gear ×4, circuit ×1 | miner_b ×1 | 5.0 | automation |
| `belt_b` | iron ×2, gear ×1 | belt_b ×2 | 1.4 | automation |
| `solar_b` | iron ×5, glass_b ×3, circuit ×1 | solar_b ×1 | 4.0 | power |
| `assembler_b` | iron ×12, gear ×6, circuit ×3 | assembler_b ×1 | 6.0 | assembly |
| `refinery_b` | iron ×10, copper ×6, circuit ×2, stone ×8 | refinery_b ×1 | 6.0 | refining |
| `reactor_b` | titanium ×12, circuit ×8, plate ×4, uranium ×4 | reactor_b ×1 | 12.0 | nuclear |
| `launchpad_b` | titanium ×8, plate ×6, circuit ×4 | launchpad_b ×1 | 8.0 | spaceport |
| `medbay_b` | plate ×2, wire ×3, circuit ×1, glass_b ×2 | medbay_b ×1 | 4.0 | power |

### 5.4 Refinery & fuel/compound recipes

| id | inputs | output | time (s) | where | tech |
|---|---|---|---|---|---|
| `fuel` | carbon ×25, oxygen ×10 | fuel ×1 | 8.0 | both | — |
| `fuel2` | coal ×15, oxygen ×12 | fuel ×2 | 9.0 | refinery | refining |
| `carbon_x` | coal ×1 | carbon ×3 | 1.5 | refinery | — |
| `oxy_x` | sodium ×1, carbon ×1 | oxygen ×2 | 2.0 | refinery | — |

### 5.5 Warp/endgame chain

| id | inputs | output | time (s) | where | tech |
|---|---|---|---|---|---|
| `antimatter` | uranium ×20, tritium ×100, circuit ×10, gold ×5 | antimatter ×1 | 30.0 | refinery | nuclear |
| `warpcell` | antimatter ×3, gold ×20, titanium ×30, data ×20 | warpcell ×1 | 60.0 | refinery | warp |
| `warp_hand` | antimatter ×4, gold ×25, titanium ×40, data ×25, fuel ×5 | warpcell ×1 | 90.0 | both | warp |

### 5.6 Furnace fuel values (burn seconds)

```js
const FUEL_VALUE = { carbon: 4, coal: 16, planks_b: 5 };
```
Per unit: carbon = 4 s, coal = 16 s, planks_b = 5 s of furnace burn time.

---

## 6. Tech tree (13 techs)

| id | name | icon | cost | time (s) | pos | req | desc | unlocked |
|---|---|---|---|---|---|---|---|---|
| `survival` | 生存本能 (Survival Instinct) | carbon | {} | 0 | [60,380] | [] | 基础采集与合成。 | **true** |
| `scan1` | 扫描增幅 I (Scan Boost I) | data | data ×4 | 10 | [230,200] | [survival] | 矿物扫描范围 24→48 格（按 C 扫描）。 | — |
| `scan2` | 扫描增幅 II (Scan Boost II) | circuit | data ×15, circuit ×4 | 20 | [400,120] | [scan1] | 矿物扫描范围 48→80 格。 | — |
| `metallurgy` | 冶金学 (Metallurgy) | furnace_b | data ×2 | 8 | [230,380] | [survival] | 解锁熔炉高效冶炼。 | — |
| `automation` | 自动化 (Automation) | miner_b | data ×5 | 15 | [400,260] | [metallurgy] | 解锁自动采矿机、传送带与火力发电机。 | — |
| `logistics` | 物流学 (Logistics) | chest_b | data ×4 | 12 | [400,500] | [metallurgy] | 解锁储物箱与物品分流。 | — |
| `power` | 清洁能源 (Clean Energy) | solar_b | data ×8 | 20 | [570,260] | [automation] | 解锁太阳能板与风力涡轮机。 | — |
| `assembly` | 装配流水线 (Assembly Line) | assembler_b | data ×12 | 25 | [570,440] | [automation, logistics] | 解锁装配机，自动制造部件。 | — |
| `refining` | 化学精炼 (Chemical Refining) | refinery_b | data ×15 | 30 | [740,340] | [power, assembly] | 解锁精炼厂：高效燃料与化合物。 | — |
| `spaceport` | 航天工程 (Spaceport Engineering) | launchpad_b | data ×20, titanium ×10 | 35 | [910,260] | [refining] | 解锁发射平台与飞船舱位扩容。 | — |
| `nuclear` | 核裂变 (Nuclear Fission) | reactor_b | data ×30, uranium ×5 | 45 | [910,440] | [refining] | 解锁核子反应堆，能源自由！ | — |
| `trade_ai` | 贸易协议 (Trade Agreement) | gold | data ×18, gold ×3 | 25 | [1080,340] | [spaceport] | 空间站交易价格优惠 15%。 | — |
| `warp` | 曲率理论 (Warp Theory) | warpcell | data ×60, tritium ×50 | 60 | [1250,340] | [trade_ai, nuclear] | 解锁曲率电池——通往群星的船票。 | — |

Dependency graph: `survival → {scan1, metallurgy}`; `scan1 → scan2`; `metallurgy → {automation, logistics}`; `automation → power`; `{automation, logistics} → assembly`; `{power, assembly} → refining`; `refining → {spaceport, nuclear}`; `spaceport → trade_ai`; `{trade_ai, nuclear} → warp`.

---

## 7. Biomes (16) — full parameters

Common fields per biome: `grass` (surface block), `dirt` (subsurface), `deep` (deep rock), `sky` [r,g,b] floats 0–1, `fog` [r,g,b], `haz` (hazard type: `null`|`heat`|`cold`|`toxic`|`rad`|`storm`), `hazName` (emoji label), `hazRate` (hazard drain rate, omitted when no hazard), `trees` (tree density), `flowers` (flower density), `oreMul` (ore multiplier), `tint` (0xRRGGBB planet tint), `terrain.type`, optional `terrain.caves`, `waterTint` (0xRRGGBB). Optional flags: `dry`, `lava`, `seaLift` (int), `crystals` (density), `mushroom` (bool), `flora` (plant spawn list), `desc`, `sub` (sub-biome blend list), `skywings`, `animal`.

`sub` entries are `{ t: float, f: float, g?: string }` — `t` = threshold/weight, `f` = frequency/multiplier, `g` = optional surface-block override.

| key | name | grass/dirt/deep | sky | fog | haz | hazRate | trees | flowers | oreMul | tint | terrain.type | caves | waterTint | flags |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `lush` | 翠绿星球 (Lush) | grass/dirt/stone | [0.48,0.72,0.95] | [0.7,0.85,1.0] | null | — | 0.012 | 0.02 | 1.0 | 0x7cc44f | continental | — | 0x3e6bd6 | — |
| `desert` | 灼热荒漠 (Desert) | sand/sand/stone | [0.95,0.75,0.5] | [0.98,0.85,0.65] | heat | 1.6 | 0.001 | 0.008 | 1.3 | 0xe0d29a | dunes | — | 0x6db8c8 | — |
| `frozen` | 冰封世界 (Frozen) | snow/dirt/ice | [0.7,0.8,0.95] | [0.85,0.9,1.0] | cold | 1.4 | 0.004 | 0.006 | 1.2 | 0xf2f6fa | glacial | ice | 0x9fd4e8 | — |
| `volcanic` | 熔火之地 (Volcanic) | basalt/basalt/basalt | [0.5,0.28,0.2] | [0.6,0.4,0.3] | heat | 2.2 | 0.0 | 0.004 | 2.0 | 0x3a3a42 | volcanic | lava_tubes | 0xff6a1a | dry:true, lava:true |
| `alien` | 异星菌境 (Alien) | alien/dirt/stone | [0.45,0.3,0.6] | [0.6,0.45,0.75] | toxic | 1.8 | 0.008 | 0.03 | 1.5 | 0x9a5fd0 | alien | — | 0x7a4ad8 | — |
| `ocean` | 蔚蓝海球 (Ocean) | grass/sand/stone | [0.35,0.62,0.88] | [0.6,0.8,0.95] | null | — | 0.007 | 0.014 | 0.9 | 0x3e8ed6 | archipelago | — | 0x2b62c8 | seaLift:7 |
| `crystal` | 晶簇冻土 (Crystal Tundra) | snow/dirt/ice | [0.55,0.75,0.85] | [0.75,0.9,0.95] | cold | 1.7 | 0 | 0.004 | 1.4 | 0x7fe8e0 | glacial | geodes | 0x8fd8e8 | crystals:0.02 |
| `fungal` | 巨菌之森 (Fungal Forest) | alien/dirt/stone | [0.5,0.38,0.55] | [0.68,0.55,0.72] | toxic | 1.3 | 0.010 | 0.02 | 1.2 | 0xc06fd8 | continental | — | 0x6a4a8a | mushroom:true |
| `ashen` | 灰烬荒原 (Ashen Wastes) | ash/ash/basalt | [0.45,0.42,0.4] | [0.6,0.58,0.55] | rad | 2.0 | 0 | 0.003 | 1.8 | 0x8a8a8a | flats | — | 0x9a7a5a | — |
| `amber` | 金珀沙海 (Amber Sands) | amber/sand/stone | [0.92,0.72,0.42] | [0.98,0.85,0.6] | heat | 1.2 | 0.001 | 0.006 | 1.1 | 0xe0a63a | dunes | — | 0xd8b048 | — |
| `ferrous` | 磁暴铁原 (Ferrous Plains) | rust/rust/basalt | [0.55,0.4,0.32] | [0.7,0.55,0.45] | storm | 1.5 | 0 | 0.004 | 1.6 | 0xa86a4a | shatter | — | 0x8a5a3a | — |
| `murk` | 荧光沼泽 (Glowing Swamp) | murk/dirt/stone | [0.16,0.3,0.28] | [0.25,0.42,0.38] | toxic | 1.1 | 0.004 | 0.035 | 1.0 | 0x2e8a72 | swamp | swamp_caves | 0x2f7a5a | seaLift:4, mushroom:true |
| `salt` | 盐晶滩 (Salt Flats) | salt/salt/stone | [0.8,0.85,0.9] | [0.92,0.95,0.98] | null | — | 0 | 0.008 | 1.0 | 0xe8ecf0 | flats | — | 0xcfe8f0 | — |
| `obsidian` | 黑曜熔壁 (Obsidian Wall) | obsidian/obsidian/basalt | [0.28,0.22,0.35] | [0.4,0.32,0.48] | heat | 1.9 | 0 | 0.002 | 1.7 | 0x2a2a35 | shatter | — | 0x4a3a6a | dry:true |
| `redmoss` | 红藓高原 (Red Moss Plateau) | redmoss/dirt/stone | [0.75,0.5,0.42] | [0.88,0.68,0.58] | cold | 1.1 | 0.003 | 0.012 | 1.15 | 0xc25a48 | mesa | — | 0xb06050 | — |
| `hive` | 蜂窝穹丘 (Hive Domes) | hive/hive/stone | [0.85,0.6,0.3] | [0.95,0.75,0.45] | toxic | 1.5 | 0 | 0.01 | 1.3 | 0xd8862a | hive | — | 0xd89830 | — |

**`sub` blend lists (per biome):**

| key | sub array |
|---|---|
| `lush` | `[{t:1,f:1},{t:0.25,f:2.2},{g:'murk',t:0.6,f:1.2}]` (森林/草原/湿地) |
| `desert` | `[{t:1,f:1},{g:'stone',t:0.05,f:1.6}]` |
| `frozen` | `[{t:1,f:1},{g:'ice',t:0.1,f:0.5}]` |
| `volcanic` | `[{t:0,f:1},{g:'basalt',t:0,f:0.3}]` |
| `alien` | `[{t:1,f:1},{g:'alien',t:0.15,f:2.5}]` |
| `ocean` | `[{t:1,f:1},{g:'sand',t:0.8,f:1.5}]` |
| `crystal` | `[{t:0,f:1},{g:'ice',t:0,f:0.5}]` |
| `fungal` | `[{t:1,f:1},{g:'murk',t:0.5,f:1.8}]` |
| `ashen` | `[{t:0,f:1},{g:'basalt',t:0,f:0.2}]` |
| `amber` | `[{t:1,f:1},{g:'amber',t:0.3,f:1.2}]` |
| `ferrous` | `[{t:0,f:1},{g:'rust',t:0,f:0.4}]` |
| `murk` | `[{t:1,f:1},{g:'murk',t:0.3,f:2.2}]` |
| `salt` | `[{t:0,f:1},{g:'sand',t:0,f:0.5}]` |
| `obsidian` | `[{t:0,f:1},{g:'basalt',t:0,f:0.2}]` |
| `redmoss` | `[{t:1,f:1},{g:'redmoss',t:0.4,f:1.6}]` |
| `hive` | `[{t:0,f:1},{g:'hive',t:0,f:0.5}]` |

**`flora` lists (only two biomes):** `murk` → `['glow_shroom','glow_shroom','oxygen_plant']`; `salt` → `['sodium_plant','sodium_plant','fern']`.

**`desc` (only for the later biomes):** amber, ferrous, murk, salt, obsidian, redmoss, hive (see source lines 234–260 for exact Chinese text).

**`skywings` (ambient flying creature colors, `{body, wing}` 0xRRGGBB):** lush `{0xe8e8dc,0x7ab8d8}`; ocean `{0xd8e8f0,0x5a9ac0}`; crystal `{0xe0f8f4,0x8ad8e0}`; fungal `{0xe8d0f0,0xb08ad8}`; murk `{0xc8f0d8,0x5ac88a}`. (Important comment: these must NOT overwrite the `sky` array field.)

**`animal` (per-biome creature `{body,legs,eye,count,name,type}`):**

| biome | body | legs | eye | count | name | type |
|---|---|---|---|---|---|---|
| lush | 0x8a9e56 | 0x5e7038 | 0x2a2a2a | 10 | 草原跳羚 | strider |
| desert | 0xd8b878 | 0xa8895a | 0x442200 | 7 | 沙壳甲虫 | crab |
| frozen | 0xdce8f0 | 0xb8c8d4 | 0x3399ff | 6 | 霜绒兽 | blob |
| volcanic | 0x5a4038 | 0xc94f1e | 0xff6600 | 5 | 熔壳蟹 | crab |
| alien | 0x9a6fd8 | 0x7c4fba | 0xffd14d | 8 | 孢子爬行者 | strider |
| ocean | 0x4da6c8 | 0x2e7893 | 0xffffff | 8 | 碧波滑行兽 | blob |
| crystal | 0xaef0ea | 0x5ec8c0 | 0x0a4f6e | 5 | 晶背蟹 | crab |
| fungal | 0xd8a8e8 | 0x9a5fd0 | 0xff5a4e | 9 | 菌帽跳虫 | strider |
| ashen | 0x6e6a66 | 0x3a3a3a | 0x7dff56 | 4 | 灰烬潜行者 | crab |
| amber | 0xe8c060 | 0xa87828 | 0x5e3808 | 6 | 珀壳掘虫 | crab |
| ferrous | 0x8a5a3a | 0x4a4a52 | 0x35e0e8 | 5 | 磁尘甲兽 | crab |
| murk | 0x2e8a72 | 0x1a5244 | 0x4ee8b8 | 9 | 沼灯浮蜓 | blob |
| salt | 0xf0f2f4 | 0xc2c9ce | 0x222222 | 7 | 盐羽鹬 | strider |
| obsidian | 0x2a2a35 | 0x6a5a9a | 0xff6600 | 4 | 曜甲蟹 | crab |
| redmoss | 0xc25a48 | 0x8a3a2c | 0xffe8a0 | 8 | 藓原掠行者 | strider |
| hive | 0xd8862a | 0x8a5210 | 0x1a1a1a | 10 | 蜂窝守卫 | strider |

### Creature types (`CREATURE_TYPES`, 6)

| key | w | h | d | headW | speed | jump | fly | hostile | hp | drops |
|---|---|---|---|---|---|---|---|---|---|---|
| `crab` | 0.55 | 0.4 | 0.7 | 0.2 | 0.7 | false | — | — | — | — |
| `strider` | 0.35 | 1.1 | 0.35 | 0.22 | 1.8 | true | — | — | — | — |
| `blob` | 0.7 | 0.5 | 0.7 | 0.0 | 0.35 | false | — | — | — | — |
| `drone` | 0.3 | 0.3 | 0.6 | 0.15 | 2.4 | true | true | — | — | — |
| `sentinel` | 0.5 | 0.5 | 0.5 | 0.0 | 2.4 | false | true | true | 10 | circuit ×1; plate ×1 (0.5) |
| `skywing` | 0.4 | 0.3 | 0.75 | 0.12 | 2.8 | false | true | — | — | — |

(`w`/`h`/`d` = body width/height/depth; `headW` = head width.)

---

## 8. Constants & embedded numeric facts

**Explicit constants:**
- `FUEL_VALUE = { carbon: 4, coal: 16, planks_b: 5 }` — furnace burn seconds per item.
- `HOME_GALAXY_SEED = 7777`.
- `GALAXY_PREFIX` (20 strings): 天琴, 杜鹃, 狐尾, 鲸落, 银帆, 烛龙, 雾马, 环蛇, 曙光, 霜港, 孤灯, 奔雷, 碎星, 拾荒, 眠沙, 赤弦, 夜莺, 枯苇, 潮汐, 洄游.
- `GALAXY_SUFFIX` (12 strings): -α, -β, -γ, -δ, -Ω, -Ⅲ, -Ⅶ, -Ⅸ, -Ⅻ, -Prime, -Minor, -Deep.
- `STAT_CLEAR = 230` — station/planet clearance distance (planet radius + station domain 213 + margin).
- Default station position `[700, 200, -500]`.

**Constants embedded only in item descriptions (not code fields):**
- Solar panel: **10 kW** (daytime only).
- Reactor: **100 kW** (all-day, consumes uranium).
- Wind turbine: **2–16 kW** (all-day, altitude-scaled).
- Burner generator: **25 kW** (coal/carbon).
- Chest: **24 slots**; Collector: **12 slots**.
- Medbay: **1 sodium + 1 oxygen → 3 HP**.
- `trade_ai` tech: station prices **−15%**.
- Scan ranges: scan1 24→48, scan2 48→80 (scan default 24).

**Stack defaults:** 250 (unless overridden — see §4). **No hotbar size, difficulty multiplier, damage values, conveyor speed, or smelt-time constants exist in this file** (smelt times are per-recipe `time`).

---

## 9. Trade, blueprints, quests, planets, galaxy

**`TRADE_GOODS` (23 ids, in order):** carbon, oxygen, sodium, coal, iron_ore, copper_ore, titanium_ore, gold_ore, uranium, tritium, iron, copper, titanium, gold, gear, wire, circuit, plate, data, fuel, glass_b, antimatter, warpcell.

**`STATION_BLUEPRINTS` (4):**
| tech | price | name |
|---|---|---|
| logistics | 800 | 蓝图：物流学 |
| power | 1500 | 蓝图：光伏能源 |
| refining | 3000 | 蓝图：化学精炼 |
| nuclear | 8000 | 蓝图：核裂变 |

**`QUESTS` (21, in order):** `q_wake, q_carbon, q_sodium, q_stone, q_furnace, q_iron, q_repair, q_tech, q_auto, q_belt, q_power, q_refinery, q_fuel, q_launch, q_station, q_trade, q_explore, q_nuclear, q_antimatter, q_warp, q_leave`. Each: `{id, title, desc, type, ...}` where `type` ∈ `event|collect|craft|place|tech`; `collect` adds `{item, n}`, `place` adds `{block, n?}`, `tech` adds `{tech}`, `event` adds `{flag}`; some add `dialog`. Exact text/values in source lines 313–347.

**`DEFAULT_PLANETS` (5):**
| id | biome | name | pos [x,y,z] | radius |
|---|---|---|---|---|
| 0 | lush | 始源星 | [0,0,0] | 150 |
| 1 | desert | 赤沙 | [1800,120,-900] | 130 |
| 2 | frozen | 霜白 | [-1500,-200,-1700] | 140 |
| 3 | volcanic | 熔核 | [900,-100,2300] | 120 |
| 4 | alien | 紫瘴 | [-2400,250,1100] | 145 |

---

## 10. Functions (exact logic)

**`resetGalaxy()`** — deep-copies defaults into mutable globals:
```
SYSTEM_PLANETS = DEFAULT_PLANETS.map(p => ({ ...p, pos: [...p.pos] }));
STATION_POS = [...DEFAULT_STATION];
```

**`galaxyName(seed)`**:
- if `seed === 7777` → return `'起源星系'`.
- else `rnd = mulberry32(seed ^ 0x6A09E667)`; return `GALAXY_PREFIX[(rnd()*20)|0] + GALAXY_SUFFIX[(rnd()*12)|0]`.

**`generateGalaxy(seed)`** (pure, no side effects):
1. `rnd = mulberry32(seed)`.
2. `biomePool` = all 16 biome ids.
3. `names` = 30-name list (翠风, 赤岭, 霜穹, 灰烬, 荒星, 渊蓝, 绿溪, 灼岩, 冰环, 晶尘, 紫涌, 绯沙, 苍脊, 黯潮, 辉冠, 裂星, 流火, 雾原, 雪锋, 熔渊, 澜礁, 菌歌, 空悬, 曜壁, 沉塔, 洄湾, 铁穗, 昙丘, 烬柱, 虹隙).
4. `count = 4 + ((rnd()*4)|0)` → **4–7 planets**.
5. For each planet i: unique name (re-roll while in `used` set); `biome = biomePool[(rnd()*16)|0]`; `ang = i/count*2π + rnd()*0.8`; `dist = 800 + rnd()*2400` (800–3200); `el = (rnd()-0.5)*700` (−350–350); `pos = [cos(ang)*dist, el, sin(ang)*dist]`; `radius = 105 + rnd()*70` (105–175).
6. Guarantee at least one carbon-rich biome: if no planet has biome ∈ {lush, ocean, fungal, alien}, set `planets[0].biome = ['lush','ocean','fungal'][(rnd()*3)|0]`.
7. Station placement: up to **200 tries**, candidate `[1200*(rnd()-0.5), 300+rnd()*400, 1200*(rnd()-0.5)]`; reject if `dx²+dy²+dz² < (p.radius + 230)²` for any planet; fallback `[0,900,0]`.
8. `market`: for each good in `TRADE_GOODS`, `market[g] = 0.75 + rnd()*0.5` (0.75–1.25 multiplier).
9. Return `{ planets, station: stat, market, seed, name: galaxyName(seed) }`.

**`setGalaxy(gal)`** — `SYSTEM_PLANETS = gal.planets; STATION_POS = gal.station;` (save-load restore).

**Initialization (runs at load):** `SYSTEM_PLANETS = DEFAULT_PLANETS.map(...)`, `STATION_POS = [...DEFAULT_STATION]` (deep copies).

---

## 11. Seed / random utilities

`mulberry32(seed)` **is not defined in this file** — it is imported/global from another module. Call sites (exact contract): `mulberry32(seed)` returns a function `rnd()` with no args returning a float in `[0, 1)`.

For the Rust port, the canonical mulberry32 (public-domain) that matches this name is:

```rust
pub fn mulberry32(seed: u32) -> impl FnMut() -> f32 {
    let mut a = seed;
    move || {
        a = a.wrapping_add(0x6D2B79F5);
        let mut t = a;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        (t ^ (t >> 14)) as f32 / 4294967296.0
    }
}
```

⚠️ Verify this matches the game's actual definition (it is **not** in `data.js`). Note the two call sites use it differently: `galaxyName` uses `mulberry32(seed ^ 0x6A09E667)`; `generateGalaxy` uses `mulberry32(seed)` directly. The `0x6A09E667` constant is SHA-256's initial hash fraction.

---

## Summary

**Total counts: 55 blocks, 46 items, 37 recipes, 13 techs** (plus 16 biomes, 6 creature types, 21 quests, 23 trade goods, 4 station blueprints, 5 default planets; no tools and no `mulberry32` definition are present in this file).
