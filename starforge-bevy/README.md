# STARFORGE 星穹熔炉 · Bevy (Rust) 移植版

原项目的浏览器 Three.js 版本已归档到 `../legacy-web/`。本目录是当前主线的 **Bevy 0.19（Rust）移植版**：
原生 Windows 可执行文件，保留原作的球面体素星球、采矿建造、背包合成、科技树、生存系统、像素美术，
并已移植**太空飞船、空间站、星系跃迁、任务线、完整工厂电力网、太空战斗与外部 CC0/CC-BY 素材库**。

> 注意：外部飞船、空间站与起源星地球模型因体积较大不包含在 Git 仓库中。
> 请从本文档下方 `外部模型下载` 中的对应来源页面下载，并按
> `assets/licenses/models-directory-audit.md` 的目录结构解压到
> `assets/models/external/`（地球模型解压到 `assets/models/earth/`），
> 否则运行时会找不到 glTF、纹理和动画资源。

## 运行

```powershell
cargo run --release
cargo run -- --smoke   # 冒烟自测：自动建世界 → 地面游玩 → 进入太空 → SMOKE_OK 退出
```

发布后的可执行文件必须与资源目录放在同一目录层级：

```text
starforge-bevy.exe
assets/
  models/
  shaders/
```

程序会固定从 `starforge-bevy.exe` 所在目录的 `assets/` 读取模型、纹理、动画和着色器，
不依赖启动时的当前工作目录。外部飞船/空间站/地球模型也必须放在该目录下的
`assets/models/external/`（地球模型为 `assets/models/earth/`）；模型下载和目录映射见本文档的“外部模型下载”及 `CREDITS.md`。

首次构建需编译全部依赖（约 10~30 分钟，取决于机器）。需要 Rust ≥ 1.85（stable 即可）。

## 操作

### 地面（星球）

| 键位 | 功能 |
|---|---|
| `W A S D` | 移动 |
| `Shift` | 疾跑 |
| `空格` | 跳跃 / 长按喷气背包 / 水中上浮 |
| `鼠标左键` | 采矿激光（选中 0 号槽） |
| `鼠标右键` | 放置方块 / 机器（带虚影预览） |
| `R` | 旋转放置朝向 |
| `滚轮` / `0~9` | 切换快捷栏（`0` = 激光） |
| `E` | 交互（机器 / 飞船 / 村庄村民 / 自动充能） |
| `G` | 丢出手中物品（`Shift+G` 整组） |
| `C` | 扫描（脉冲 + 矿物标记，范围随科技 24/48/80，6s 冷却） |
| `Tab` | 背包与合成（左键取放/右键拆半/Shift 快速移动/垃圾桶/整理/充能，Shift+点击合成 ×5） |
| `T` | 科技树 |
| `M` | 星球全息地图（标记增删/全星系显示/信标与 POI 图钉） |
| `P` | 创造物品库（创造模式） |
| `O` | Bevy 原生联机（创建主机 / 加入 / 聊天 / 在线列表） |
| `F5` | 快速存档 |
| `Esc` | 关闭面板 / 系统菜单（设置 / 保存 / 返回主菜单） |

### 飞船

| 键位 | 功能 |
|---|---|
| `E`（近飞船） | 检查 / 修复 / 登船 |
| `W`（舱内） | 点火起飞（需发射燃料，或停在发射平台上） |
| `W/S`（飞行） | 油门 / 刹车 |
| `Shift` | 加力 |
| `A/D` | 滚转 |
| `鼠标` | 转向 |
| `鼠标左键`（太空） | 武器开火（双发弹道，可击碎小行星/击毁访客船） |
| `E`（大气中） | 就地降落 |
| `J`（太空） | 脉冲引擎（消耗氚，冲刺至 900 u/s） |
| `C`（太空） | 扫描（最近天体距离） |
| `M`（太空） | 星系地图（锁定目标星系，脉冲冲刺对准即自动跃迁，需曲率电池） |
| `F`（飞行） | 第一人称 / 第三人称镜头切换 |
| `滚轮`（第三人称） | 镜头距离缩放（6~60） |
| `鼠标右键`（飞行，按住） | 自由环视（只转镜头，不改变飞船航向） |
| `N`（太空/停泊） | 打开换船电脑 |
| `E`（太空） | 靠近星球无缝再入；靠近空间站顶部自动停泊 |

### 空间站

| 键位 | 功能 |
|---|---|
| `W`（停泊） | 离站 |
| `E`（停泊） | 打开空间站服务（贸易终端 / 买船中心 / 换船电脑） |
| `N`（太空/停泊） | 打开换船电脑 |

## 已移植（相对原版）

- **球面体素星球**：16×16×96 区块流式加载；16 种生态的全部地形公式、洞穴（5 种）、矿脉（6 种）、树木/巨菌、村庄与遗迹（3 种）、浮空岛；
- **星球曲率着色器**：顶点弯曲（250 格曲率半径）、水面波浪、发光方块、NMS 式扫描脉冲；
- **像素美术**：62 张 16×16 程序化方块贴图 + 全部物品图标（32×32，最近邻采样）；
- **玩家控制与生存**：六维状态、无条件氧气消耗、生态危险、危险低值警报、熔岩灼烧、充能、死亡重生、摔落伤害；
- **采矿与建造**：硬度→挖掘时间、掉率难度倍率、掉落物磁吸与同类合并（上限 90）、拆机内容退款、上方植物连带掉落、方块与机器放置、「需要采矿激光」提示；
- **完整工厂电力网**：太阳能、风力、火力、核能与地热发电；电缆组成独立局部电网，工业蓄电池自动充放电；装配、精炼、采矿、医疗、流体泵、殖民核心和炮塔按实际工况耗电，并在 HUD 显示满足率；
- **自动化与物流**：传送带转弯并向机器投料，智能分流器轮转出口，筛选分流器按物品分线；管道、储液罐和流体泵组成定向流体网络；另有伐木机器人（扫描→伐木→送货）、收集点、标记方块和免燃料起飞的发射平台；
- **殖民与防务闭环**：殖民核心扫描周围舱室规模，消耗压缩氧气瓶、医疗包与生物纤维，在稳定供电下周期产出研究数据和信用点；自动防御炮塔以 24 格射程保护基地，主动攻击遗迹守卫，并只反击已敌对的本地生物；
- **内容/背包/合成/科技**：87 种方块、114 种物品、93 个配方与 24 节点科技树；36 格背包支持取放、合并、拆半、Shift 快速移动、整理和物品提示，研究计时、完成播报与进度均可持久化；
- **太空飞船**：CC-BY-4.0 外部 glTF 飞船模型（等级差异化 C/B/A/S，含材质、纹理和动画）、登船/下船、燃料加注、大气层飞行（经纬环绕、地形碰撞细分步进防高速穿墙、转向侧倾压弯、再入俯冲保护、冲出大气交棒）、太空飞行（姿态/滚转/脉冲引擎/氚消耗）、恒星高温危险、**小行星群（GLB 陨石，可击碎掉落氚/金）**；
- **太空战斗**：鼠标左键双发激光弹道、武器威力随等级（C1/B1/A2/S4）、高速线段碰撞、**访客船队巡航/进站/停靠/离站 + 可击毁（战利品直入货仓 + 信用点，35-65s 补员）**，站体遇袭会升起护盾并封闭泊入通道；
- **无缝换系**：体素 ↔ 球面坐标精确互逆映射（有往返测试），大气层 ⇄ 太空零传送；**多星球**：同星系多星球再入，星球档案（建筑/机器/区块改动/地图标记）随离开归档、返回恢复；跨星系档案（`galaxyArchives`）；
- **空间站**：靠近站顶即自动悬停泊入（无需进机库/强制落地）、按模型实际包围盒的逐网格碰撞（镂空区域可穿越）、停泊后 `E` 打开服务菜单（贸易终端 / 买船中心 / 换船电脑）、`N` 键快捷换船、游商船停靠休息（不再对话卖船）、太空访客船停靠休息后离站；
- **星系与曲率跃迁**：初始星系 5 行星 + 空间站；随机星系生成（4~7 颗星球、市场波动、站体与星球分离校验）；可交互的旋转 3D 投影星图（55 邻域 + 已到访标记 + 回家锁定）、脉冲冲刺自动跃迁、180 条发光曲速星线与动态拉伸、抵达新星系（跨星系标记随档案换档）；
- **34 步双章主线任务线**：采集/合成/放置/研究/事件全类型推进，从求生、自动化和跃迁延伸到智能物流、外骨骼、深空海盗、殖民核心与基地防线；含任务日志 HUD、奖励 ₪ 与村庄支线委托；
- **星球全息地图（M）**：2D 全景地图（村庄/遗迹/信标/飞船/玩家箭头）、点击添加标记（名称/全星系显示）、标记列表（切换范围/删除）、存档持久化；
- **捏人**：主菜单角色创建（肤色/发型/发色/制服/饰条/裤装/靴子/目镜/头盔，🎲 随机），外观随存档；**CC0 GLB 角色模型**（站内 NPC/村民/游商）；
- **像素风低分辨率渲染模式**：640×360 渲染目标 + 最近邻全屏放大（主菜单/设置开启，重启生效）；
- **存档**：人物/世界分离 JSON（`saves/`），含外观/装备/飞船/机库/任务进度/旗标/动态市价/星系种子/太空船状态/**地图标记/跃迁锁定/放置计数/研究进度/机器库存与殖民统计/跨星系档案**；
- **Bevy 原生联机（O）**：内置权威 UDP 主机，无外部服务器依赖；同世界指纹校验、最多 32 人、玩家位置/状态插值、聊天与在线列表、方块及机器放置增量、迟加入增量回放、输入边界与超时校验。协议仅服务 Bevy 版，不兼容旧版 Node.js；
- **昼夜与气候**：480s 昼夜周期、16 种生态独立天气（雨/雪/灰烬/孢子等粒子）、生态化高空云团、太空可见的行星云层，云层和天气可在设置中独立关闭；**Minecraft 风格生物系统**（24m 兽群网格确定性生成 + 密度上限 16、24–128m 生成环带、>128m 卸载/<96m 重载、被杀生物永久消失、兽群与格子掩码随存档/跨星系档案持久化；淡入淡出 + 散步/休息状态机 + 行走摇摆/弹跳/呼吸/受击程序化动画）、**Sonniss GDC 2026 音效库（32 条 WAV，含雨声/引擎/喷气/爆炸/扫描/UI 全套；脚步沿用迁移前音效 + 主音量控制）**；
- **星球 LOD**：区块预生成一环 + 预网格化一环（跨越区块不再空荡）、远景挖空环 GPU 化（中心向外重建）、出大气地形淡出 + 星球球面淡入、**星球贴图采样真实体素地形（512×1024 线性过滤）**。

## 外部素材（全部可免费商用）

### 外部模型下载

外部飞船、空间站与起源星地球模型因体积较大不随 Git 仓库分发。请从下表对应的来源页面下载模型压缩包，解压后保留 `scene.gltf`、`scene.bin`、`textures/` 和 `license.txt`，并放入 `assets/models/external/` 下对应目录（地球模型解压到 `assets/models/earth/`）。完整 URL、作者署名和目录映射见 `CREDITS.md` 与 `assets/licenses/models-directory-audit.md`。

| 目录 | 来源 |
|---|---|
| `assets/models/earth/` | [Earth（起源星）](https://sketchfab.com/3d-models/earth-4de1bcbd22a444abb4f089b9b78ec96a) |
| `assets/models/external/ships/space_ship_b/` | [Space Ship B](https://sketchfab.com/3d-models/space-ship-356a3acb00164c698d657146caa5ebf3) |
| `assets/models/external/ships/space_ship_c/` | [Space Ship C](https://sketchfab.com/3d-models/space-ship-63ce372c1aa843e98bf1548109e055d8) |
| `assets/models/external/ships/space_ship_torb/` | [Space Ship “Torb”](https://sketchfab.com/3d-models/space-ship-torb-fb9cac9500d147528b6cdef8385cf926) |
| `assets/models/external/ships/supermatic_sky_cruiser/` | [Supermatic Sky Cruiser](https://sketchfab.com/3d-models/supermatic-sky-cruiser-d8e0d3253dfa45479f7637d3cff32c4c) |
| `assets/models/external/ships/unsa_destroyer/` | [UNSA Destroyer](https://sketchfab.com/3d-models/unsa-destroyer-spaceship-0fd8c6ecd9374392a1ed900e82d7417d) |
| `assets/models/external/stations/space_station/` | [Space Station](https://sketchfab.com/3d-models/space-station-0da4a24e7edd49159737675ffcc06228) |
| `assets/models/external/stations/space_station_3/` | [Space Station 3](https://sketchfab.com/3d-models/space-station-3-a7a6ad10261149cab31aa394bfcf8940) |
| `assets/models/external/stations/space_station_4/` | [Space Station 4](https://sketchfab.com/3d-models/space-station-4-cf80075368174bf9895f4fd266cf17e3) |
| `assets/models/external/stations/helveta/` | [HelVeta Space Battle Ship](https://sketchfab.com/3d-models/helveta-space-battle-ship-b743d59343834ec593aa6c2c02bf8473) |

| 素材 | 来源 | 许可 |
|---|---|---|
| 音效（assets/audio/，32 条 Sonniss WAV + 1 条旧版脚步 OGG） | Sonniss GDC 2026 Game Audio Bundle + Kenney Sci-Fi Sounds | Sonniss GDC Game Audio License + CC0 |
| 随仓库提供的飞船/宇航员/陨石模型 | Kenney Space Kit | CC0 1.0 |
| 外部飞船/空间站模型 | Sketchfab（作者见 `CREDITS.md`） | CC-BY-4.0，必须署名 |
| 起源星地球模型（assets/models/earth/） | Sketchfab（作者：SebastianSosnowski） | CC-BY-4.0，必须署名 |
| NPC 角色（冒险者 5 款） | KayKit Character Pack: Adventurers | CC0 |
| 遗迹守卫（骷髅） | KayKit Character Pack: Skeletons | CC0 |
| 生物模型（羊驼/鹿/狐/狼，带骨骼动画） | Quaternius Ultimate Animated Animal Pack | CC0 1.0 |

完整登记与署名要求见 **`CREDITS.md`**，许可证原文在 `assets/licenses/`。

## 迁移状态

核心玩法与最后一轮缺口已完成迁移：原生联机、生态天气与云层、访客泊站、空间站护盾、交互式星图和曲速星线均由 Bevy/Rust 实现。旧 Three.js/Node.js 版本只在 `../legacy-web/` 中归档，不参与新版本运行。

## 技术栈

- [Bevy 0.19](https://bevy.org)（MIT/Apache-2.0）—— ECS、PBR、窗口、音频、GLB 场景；
- [bevy_egui 0.41](https://crates.io/crates/bevy_egui)（MIT/Apache-2.0）—— UI；
- 自定义 `ExtendedMaterial<StandardMaterial, TerrainExtension>` 顶点着色器：曲率弯曲/水面波浪/发光/扫描脉冲；
- 程序化噪声（复刻原版 mulberry32 / 2D Perlin / 3D 值噪声，含 JS 黄金值回归测试）；
- 字体：[Noto Sans SC](https://github.com/google/fonts)（SIL OFL 1.1，`assets/fonts/`）。

## 测试

```powershell
cargo test --locked --all-targets --all-features
cargo clippy --locked --all-targets --all-features -- -D warnings
```

## 目录结构

```
src/
├── main.rs         # App 装配、状态机、流式加载、星球切换、保存、光标管理
├── data.rs         # 方块/物品/配方/科技/生态 + 任务线/贸易/飞船等级/星系生成（data.js 1:1）
├── world.rs        # 地形生成、区块、网格、射线、RLE、远景地形
├── player.rs       # 移动/液体/喷气背包/采矿激光/放置/快捷栏/生存
├── textures.rs     # 方块贴图 + 物品图标
├── materials.rs    # 地形扩展材质 + 曲率 uniform + 灯池（跟随灯块）
├── network.rs      # Bevy 原生 UDP 主机/客户端、聊天、玩家与体素增量同步
├── daynight.rs     # 昼夜循环、星空、太阳
├── weather.rs      # 16 生态天气、高空云团、太空行星云层
├── creatures.rs    # 生物（GLB 模型）+ 遗迹守卫 + 掉落物
├── factory.rs      # 局部电网 + 物流/流体网络 + 全部机器 + 殖民核心/自动炮塔
├── space.rs        # 太空飞行/大气层飞行/曲速跃迁/星系/星球换系/太空战斗/访客舰队
├── station.rs      # 空间站泊入/站内行走/贸易/购船
├── quests.rs       # 34 步双章任务线 + 村庄支线
├── char.rs         # 捏人外观 + CC0 GLB 角色模型
├── inventory.rs    # 背包
├── ui.rs           # egui HUD / 背包 / 科技树 / 机器面板 / 贸易 / 车库 / 星系图 / 星球地图 / 菜单
├── audio.rs        # Sonniss GDC 2026 音效库 + 迁移前脚步音效（内嵌 + 主音量）
├── save.rs         # 人物/世界 JSON 存档（含太空状态/标记/档案）
└── rng.rs          # mulberry32 / Perlin / 值噪声（含黄金值测试）
assets/
├── audio/          # 32 条 Sonniss GDC 2026 WAV + 迁移前 step.ogg
├── models/         # 仓库内模型；external/ 外部大模型需按上文下载
├── licenses/       # 第三方许可证原文
├── fonts/NotoSansSC.ttf
└── shaders/terrain_{vertex,prepass_vertex,fragment}.wgsl
```

## 许可

本项目代码 [MIT](LICENSE)（沿用原项目许可）；外部素材许可见 `CREDITS.md` 与 `assets/licenses/`；
体积云渲染使用 vendored 的 `bevy-volumetric-clouds` 0.2.0，来源为
<https://github.com/evroon/bevy-volumetric-clouds>，按 MIT 许可证使用；上游版权归
evroon（2025），许可证原文保留在 `vendor/bevy-volumetric-clouds/LICENSE`，登记见
`CREDITS.md`；
Noto Sans SC 字体按 [SIL Open Font License 1.1](https://scripts.sil.org/OFL) 分发。
