# STARFORGE Bevy 移植版 · 第三方素材 Credits

> 本登记簿记录所有引入的外部素材。**全部素材均允许免费商用**（CC0 免署名 / CC-BY 署名）。
> 许可证原文已存档于 `assets/licenses/`。
> 素材下载日期：2026-08-18（以当前仓库 `assets/` 内文件为准）。

> **外部模型不随 Git 仓库分发。** 飞船和空间站模型总计约 557 MB，因体积过大被排除在仓库之外。
> 请从下方表格或 `assets/licenses/models-directory-audit.md` 中的对应来源页面下载，按目录名解压到
> `assets/models/external/`，并保留每个包中的 `license.txt`、`scene.gltf`、`scene.bin` 和 `textures/`。

## 音效（assets/audio/*.ogg，共 32 条）

| 素材 | 来源 | 许可证 | 署名要求 |
|---|---|---|---|
| Kenney UI Audio（click/rollover/switch 系列，用于 click/hover/craft/ui_open/ui_close/coin/insert/error 等） | https://kenney.nl/assets/ui-audio | CC0 1.0 | 无 |
| Kenney Sci-Fi Sounds（impactMetal/explosionCrunch/thrusterFire/laser/spaceEngine/computerNoise/slime/door/forceField 系列，用于 shoot/explosion/engine_loop/jet/laser_hit/jump/land/hurt/creature_hit/creature_die/break_block/place/dig/pickup/open_chest/step/warp/alarm/dock/takeoff/land_ship/pulse/scan/research 等） | https://kenney.nl/assets/sci-fi-sounds | CC0 1.0 | 无 |

> 32 条文件清单：alarm、break_block、click、coin、craft、creature_die、creature_hit、dig、dock、engine_loop、error、explosion、hover、hurt、insert、jet、jump、land、land_ship、laser_hit、open_chest、pickup、place、pulse、research、scan、shoot、step、takeoff、ui_close、ui_open、warp（均为 .ogg）。

## 3D 模型（assets/models/，含 GLB / glTF）

### `models/` 中导入的 CC-BY-4.0 飞船与空间站

这些模型均允许免费修改和商业使用，但发行物必须保留作者署名；原始
`license.txt` 随模型目录保留，完整审计见 `assets/licenses/models-directory-audit.md`。

| 文件 | 用途 | 作者 | 来源 |
|---|---|---|---|
| `models/earth/` | 起源星（始源星）太空侧模型 | SebastianSosnowski | https://sketchfab.com/3d-models/earth-4de1bcbd22a444abb4f089b9b78ec96a |
| `models/external/ships/space_ship_b/` | B 级飞船 | yanix | https://sketchfab.com/3d-models/space-ship-356a3acb00164c698d657146caa5ebf3 |
| `models/external/ships/space_ship_c/` | C 级飞船 | Comrade1280 | https://sketchfab.com/3d-models/space-ship-63ce372c1aa843e98bf1548109e055d8 |
| `models/external/ships/space_ship_torb/` | `ship_striker` 变体 | tramkar | https://sketchfab.com/3d-models/space-ship-torb-fb9cac9500d147528b6cdef8385cf926 |
| `models/external/ships/supermatic_sky_cruiser/` | A 级飞船 | VertaScan | https://sketchfab.com/3d-models/supermatic-sky-cruiser-d8e0d3253dfa45479f7637d3cff32c4c |
| `models/external/ships/unsa_destroyer/` | S 级飞船 | xaxary | https://sketchfab.com/3d-models/unsa-destroyer-spaceship-0fd8c6ecd9374392a1ed900e82d7417d |
| `models/external/stations/space_station/` | 家园空间站 | re1monsen | https://sketchfab.com/3d-models/space-station-0da4a24e7edd49159737675ffcc06228 |
| `models/external/stations/space_station_3/` | 其他星系空间站 | re1monsen | https://sketchfab.com/3d-models/space-station-3-a7a6ad10261149cab31aa394bfcf8940 |
| `models/external/stations/space_station_4/` | 其他星系空间站 | re1monsen | https://sketchfab.com/3d-models/space-station-4-cf80075368174bf9895f4fd266cf17e3 |
| `models/external/stations/helveta/` | 其他星系大型空间站 | Inditrion Dradnon | https://sketchfab.com/3d-models/helveta-space-battle-ship-b743d59343834ec593aa6c2c02bf8473 |

> 每个条目的完整 CC-BY-4.0 署名文本在对应目录的 `license.txt` 中。损坏的 `borderlands_style_space_ship_A_level.zip` 未导入。

### Kenney Space Kit（CC0 1.0，免署名）— https://kenney.nl/assets/space-kit

> 注意：Kenney 原始 GLB 的根节点带有一个 `t(2, 0, 1.5)` 平移（模型原点不在脚底/中心）。本移植版在素材层移除了该根节点平移，使模型原点对齐游戏逻辑（脚底着地/几何中心），渲染位置与命中判定一致。

| 文件 | 对应 Kenney 素材 |
|---|---|
| models/ships/ship_a.glb | craft_speederA |
| models/ships/ship_b.glb | craft_speederB |
| models/ships/ship_c.glb | craft_speederC |
| models/ships/ship_s.glb | craft_speederD |
| models/ships/visitor1.glb | racer |
| models/ships/visitor2.glb | miner |
| models/ships/visitor3.glb | cargoA |
| models/ships/visitor4.glb | cargoB |
| models/npc/alien.glb | alien |
| models/npc/astronaut_a.glb | astronautA |
| models/npc/astronaut_b.glb | astronautB |
| models/asteroids/meteor.glb | meteor |
| models/asteroids/meteor_detailed.glb | meteor_detailed |

### KayKit Character Packs（CC0，免署名）

| 文件 | 来源 | 许可证 |
|---|---|---|
| models/npc/adventurer_barbarian.glb | KayKit Character Pack: Adventurers | CC0 |
| models/npc/adventurer_knight.glb | KayKit Character Pack: Adventurers | CC0 |
| models/npc/adventurer_mage.glb | KayKit Character Pack: Adventurers | CC0 |
| models/npc/adventurer_rogue.glb | KayKit Character Pack: Adventurers | CC0 |
| models/npc/adventurer_rogue_hooded.glb | KayKit Character Pack: Adventurers | CC0 |
| models/creatures/sentinel.glb | KayKit Character Pack: Skeletons（Skeleton_Warrior） | CC0 |

- KayKit Character Pack: Adventurers：https://github.com/KayKit-Game-Assets/KayKit-Character-Pack-Adventures-1.0
- KayKit Character Pack: Skeletons：https://github.com/KayKit-Game-Assets/KayKit-Character-Pack-Skeletons-1.0

### Quaternius Ultimate Animated Animals（CC0 1.0，免署名）

> 官方包提供 glTF，并包含 Idle、Walk、Gallop、Jump、Death 等动画。本项目使用内嵌数据的 glTF 文件，避免额外纹理依赖。

| 文件 | 对应模型 |
|---|---|
| models/creatures/quaternius_alpaca.gltf | Alpaca |
| models/creatures/quaternius_deer.gltf | Deer |
| models/creatures/quaternius_fox.gltf | Fox |
| models/creatures/quaternius_wolf.gltf | Wolf |

- 官方页面：https://quaternius.com/packs/ultimateanimatedanimals.html
- glTF 下载目录：https://drive.google.com/drive/folders/1uJ3N5HfB7jKTseJUNQr3N4YaN0UuEtHk?usp=sharing
- 许可证原文：`assets/licenses/quaternius_ultimate_animated_animals_LICENSE.txt`

### 需署名条目（CC-BY 3.0）

> 来源：Poly Pizza（https://poly.pizza）。模型使用 CC-BY 3.0 许可，署名如下：

| 文件 | 作者 | 署名文本 | 许可链接 |
|---|---|---|---|
| models/creatures/crab.glb（"Crab"） | Poly by Google（Poly Pizza） | Crab by Poly by Google, via Poly Pizza（CC-BY 3.0） | https://poly.pizza/m/2DgM36qZW2u |
| models/creatures/blob.glb（"Slime"） | Quaternius（Poly Pizza） | Slime by Quaternius, via Poly Pizza（CC-BY 3.0） | https://poly.pizza/m/LyjSUKHKnh |
| models/creatures/strider.glb（"Deer"） | Poly by Google（Poly Pizza） | Deer by Poly by Google, via Poly Pizza（CC-BY 3.0） | https://poly.pizza/m/002f5e83 |

## 字体

| 素材 | 来源 | 许可证 |
|---|---|---|
| Noto Sans SC（assets/fonts/NotoSansSC.ttf） | Google Noto | SIL OFL 1.1 |

## 备注

- 程序化贴图（方块/物品图标）与程序化星球/空间站几何属于原版 1:1 移植的一部分，保持程序生成。
- 被动生物使用 Quaternius 的带骨骼动画 glTF（CC0）；生物 AI 状态会在 Idle 与 Walk 之间切换，模型自带四肢、尾巴和头部动作随动画播放。
- 旧版 Poly Pizza 生物文件仍保留在仓库中，但不再作为当前被动生物的默认模型；玩家飞船与访客飞船模型来自 Kenney Space Kit（CC0）；哨兵（sentinel）来自 KayKit Skeletons（CC0）；冒险者 NPC 来自 KayKit Adventurers（CC0）。
- 素材进入仓库前已做轻量清洗：移除 Kenney GLB 根节点平移、修正 blob 节点缩放（100 倍）造成的渲染尺寸偏差；仅修改变换，未改动任何网格/材质/动画数据。

## 发行与署名要求

- **免署名素材**（CC0 / KayKit CC0）：可自由使用、修改、商用，无需署名；建议在发行物中附带本文件与 `assets/licenses/` 作为致谢。
- **需署名素材**（Poly Pizza CC-BY 3.0）：随发行物分发、展示或商用本游戏的二进制/截图/宣传材料时，必须保留上述署名文本（Crab / Slime / Deer by ... , via Poly Pizza），并在发行物中附带本文件。
- 若发行二进制，请随发行物附带本文件与 `assets/licenses/` 目录。
