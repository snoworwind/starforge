# STARFORGE Bevy 移植版 · 外部素材接入研究报告

> **本文档用途**：给后续 AI Agent / 开发者直接使用的工作交接文档。
> 涵盖：① 项目素材现状（代码级定位）；② Bevy 0.19.1 技术可行性（已对源码核实）；③ 网上可免费商用的
> 音效 / 生物模型 / 飞船模型素材清单（许可证已逐项核实）；④ 落地实施路线图；⑤ 许可合规规范与避坑清单。
>
> **调研日期**：2026 年（基于当时网络快照；**下载素材当天请以资源页面实际标注的许可证为最终依据**）
> **适用范围**：`starforge-bevy/`（Bevy 0.19.1 / Rust 2024 / edition 2024）

---

## 0. TL;DR（30 秒速览）

| 问题 | 结论 |
|---|---|
| 项目现在用什么素材？ | **100% 程序化生成**，零外部媒体素材。`assets/` 只有字体 + 3 个 WGSL 着色器 |
| 能不能引入外部素材？ | ✅ 能。**GLB 模型 + OGG/WAV 音频开箱即用**，无需改 `Cargo.toml`（Bevy 0.19 默认特性已含 `bevy_gltf` + `gltf_animation` + `vorbis`；项目已开 `wav`） |
| 用什么协议最省心？ | **CC0 / Public Domain（免署名）**：Kenney、Quaternius、KayKit 三大作者全线 CC0，是主力来源 |
| 推荐组合 | 音效 = Kenney 音频 + Signature Sounds CC0 + Sonniss GDC 大包；生物 = Quaternius 动画动物 + Kenney Animal Pack + KayKit 人物；飞船 = Kenney Modular Space Kit + Quaternius Ultimate Spaceships |
| 最大坑 | ① Bevy 0.19 **不支持 Draco 压缩 GLB**；② 混合许可平台（OGA / Freesound / Poly Pizza / Sketchfab / itch.io）必须逐条核对，**排除 NC / SA / GPL**；③ Bevy 0.19 场景 API 大改（`SceneRoot` 已移除，用 `WorldAssetRoot`） |

---

## 1. 项目素材现状（代码级定位）

### 1.1 `assets/` 目录现状

```
starforge-bevy/assets/
├── fonts/NotoSansSC.ttf        # 唯一的外部媒体文件（SIL OFL 1.1，随发行包）
└── shaders/terrain_*.wgsl      # 3 个自写着色器（不是素材）
```

### 1.2 音频 —— `src/audio.rs`（222 行，全部程序合成）

- 启动时用 `synth()`（正弦/锯齿波 + 指数衰减包络）合成 16-bit PCM 单声道 WAV（22.05kHz），经 `Sfx::build(&mut Assets<AudioSource>)` 注册为 `AudioSource` 资产。
- 现有 **12 个音效**：

| 字段 | 效果 | 合成方式 | 时长 | 音量 |
|---|---|---|---|---|
| `dig` | 挖掘 | 90→0Hz 下降锯齿 | 0.09s | 0.5 |
| `place` | 放置方块 | 70Hz 正弦 + 衰减 | 0.12s | 0.7 |
| `break_block` | 方块碎裂 | 160→0Hz 下降锯齿 | 0.16s | 0.6 |
| `jump` | 跳跃 | 240→1140Hz 上升正弦 | 0.18s | 0.5 |
| `hurt` | 受伤 | 320→0Hz 下降锯齿 | 0.25s | 0.6 |
| `pickup` | 拾取 | 660→1320Hz 上升正弦 | 0.09s | 0.45 |
| `click` | UI 点击 | 1400Hz 正弦 | 0.03s | 0.4 |
| `craft` | 合成 | 440→660Hz 双音 | 0.14s | 0.4 |
| `jet` | 喷气背包 | 70-100Hz 噪声相位循环（可循环） | 0.5s | 0.6 |
| `laser_hit` | 激光命中 | 900→0Hz 下降锯齿 | 0.1s | 0.4 |
| `error` | 错误提示 | 140Hz 正弦 | 0.15s | 0.5 |
| `alarm` | 警报 | 400Hz±180Hz 颤音 | 0.9s | 0.5 |

- **缺口**：无背景音乐、无环境音（风/水/洞穴/昼夜氛围）、无生物叫声、无飞船引擎/爆炸/曲速音、无 UI 之外的反馈音。
- 关键 API（替换时保持兼容）：
  - `pub struct Sfx { pub dig: Handle<AudioSource>, ... }`（Resource）
  - `pub fn play(commands: &mut Commands, handle: Handle<AudioSource>, volume: f32, pitch: Option<f32>)` —— 基于 `AudioPlayer` + `PlaybackSettings { mode: PlaybackMode::Despawn, volume, speed }`
  - `#[derive(Component)] pub struct JetSound;` —— 循环音实体（despawn 即停）

### 1.3 生物 —— `src/creatures.rs`（带骨骼动画的 Quaternius glTF）

- 被动生物使用 Quaternius 的 Alpaca / Deer / Fox / Wolf glTF，文件内嵌网格、材质和骨骼动画。
- `CreatureAnimationSetup` 使用模型自带的 Idle / Walk 动画；四肢、尾巴、头部随步态一起摆动。
- 生态类型扩展为 `strider`、`hopper`、`crab`、`beetle`、`manta`、`blob`，由 `data::biome_animal_kind` 确定性映射到四个动画模型。
- **替换模型时必须保留的 `Creature` 组件**（碰撞/逻辑不依赖模型外观）：

```rust
pub struct Creature {
    pub hp: f32, pub radius: f32, pub height: f32,
    pub shoot_t: f32, pub ai_t: f32, pub dir: Vec3, pub vel: Vec3,
    pub grounded: bool, pub home: Vec3, pub jump_t: f32, pub kind: &'static str,
}
```

- 生物按生态确定性生成：`creature_spawn_system`（每 1.5s 检查，玩家周围 25 格，数量上限 `animal.4`）。

### 1.4 人形 NPC / 村民 —— `src/char.rs`（147 行，体素人形）

- `spawn_humanoid(commands, meshes, mats, appearance, pos, yaw, extra)`：9 组立方体拼装（腿/靴/躯干/饰条/手臂/手/头/目镜/头盔或发型 6 种变体），纯色材质，外观随存档。
- 返回 `HumanoidParts { root, head, torso, arm_l, arm_r, leg_l, leg_r }`（部分字段为 `Entity::PLACEHOLDER`，目前只有 head/root 被实际使用）。
- 用于：空间站站员、村民 NPC、主菜单角色预览。

### 1.5 飞船 —— `src/space.rs` `spawn_ship`（约 376–467 行）

- 程序化拼装 15 个立方体部件：机身(1.4×0.9×3.6)、座舱玻璃(半透明)、机翼(±1.9)、引擎(±0.55)、起落架×3、引擎光斑×2（自发光）、尾焰×2（半透明蓝）。
- 签名：`spawn_ship(commands, meshes, mats, pos, yaw, cls: &ShipClass) -> (Entity, Vec<Entity>)` —— 返回 (根实体, 尾焰实体列表)。
- **C/B/A/S 四等级只换 `accent` 涂装色**（`cls.color`），几何完全相同。
- 尾焰由 `flames` 实体在飞行时显隐/缩放，替换模型时建议保留尾焰实体方案。

### 1.6 贴图 —— `src/textures.rs`（1081 行）

- 62 张 16×16 方块贴图 + 全部物品图标（32×32），全部程序绘制（mulberry32 种子 + 调色板）。**本报告不涉及替换贴图**（程序化贴图是原版 1:1 移植的一部分，有确定性回归测试，改动风险大）。

### 1.7 太空场景与空间站

- 星球：程序化 128×256 噪声贴图球体（`planet_texture`）；恒星/太阳：球体 + 自发光；小行星：不规则缩放球体；星星：立方体。
- 空间站（`station.rs`）：程序化碰撞盒（`station_cols()`）+ 板状结构，无外观模型；站内地面/边界也是程序化。

---

## 2. 技术可行性（已对 Bevy 0.19.1 源码核实 ✅）

> 核实方式：直接读取本地 cargo 缓存中 `bevy-0.19.1` / `bevy_internal-0.19.1` / `bevy_gltf-0.19.1` / `bevy_scene-0.19.1` / `bevy_animation-0.19.1` / `bevy_world_serialization-0.19.1` 源码。

### 2.1 Cargo feature 结论（`starforge-bevy/Cargo.toml` 无需改动）

| 能力 | 状态 | 依据 |
|---|---|---|
| GLB/glTF 加载 | ✅ 默认启用 | bevy `3d` 默认特性 → `3d_bevy_render` → `bevy_gltf` + `gltf_animation` |
| 骨骼动画（GLB） | ✅ 默认启用 | `gltf_animation` 在默认链内；GLB 场景 spawn 时自动给根动画实体挂 `AnimationPlayer` |
| OGG(Vorbis) 音频 | ✅ 默认启用 | bevy `audio` 默认特性 = `bevy_audio` + `vorbis` |
| WAV 音频 | ✅ 已启用 | 项目 `Cargo.toml` 显式 `features = ["wav"]` |
| MP3 / FLAC / AAC | ❌ 未启用 | 如需用，改 `bevy = { features = ["wav", "mp3", ...] }` |
| 场景组件 `SceneRoot` | ❌ 已移除 | Bevy 0.19 场景系统重写（BSN），改用 `WorldAssetRoot`（见 2.2） |

### 2.2 Bevy 0.19 加载/生成 GLB 的正确姿势（官方 doc 示例，已验证）

```rust
use bevy::prelude::*;
use bevy_gltf::prelude::*;                 // GltfAssetLabel
use bevy_world_serialization::prelude::*;  // WorldAssetRoot

fn spawn_gltf(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn((
        // 等价于 "models/ship.glb#Scene0"；#Scene0 标签必须显式指定，
        // 否则 bevy 不知道加载文件中的哪一幕
        WorldAssetRoot(asset_server.load(
            GltfAssetLabel::Scene(0).from_asset("models/ship.glb"),
        )),
        Transform::from_xyz(0.0, 0.0, 0.0),
        crate::InGame,   // 项目自带的游戏态标记组件（按需）
    ));
}
```

- 若要按名称取场景（多场景 GLB）：先 `asset_server.load::<Gltf>("...")`，等 `Assets<Gltf>` 就绪后取 `gltf.named_scenes["SceneName"]` 或 `gltf.scenes[0]`，再 `WorldAssetRoot(handle)`。
- **动画播放**（0.19 新 API，比 0.15–0.18 复杂）：GLB spawn 后自动带 `AnimationPlayer`；驱动需要 `AnimationGraph` / `AnimationNodeIndex` / `AnimationTransitions`。**下一步 agent 请直接参考 Bevy 0.19 官方示例**（bevy 仓库 `examples/animation/` 下的 mesh 动画示例）与 `docs.rs/bevy_animation/0.19.1`，不要照抄 0.18 及以前的 `Animator` 写法（0.19 中 `Animator` 组件已不存在，已核实源码）。
- **循环音/音乐**：现有 `PlaybackSettings` 改为 `mode: PlaybackMode::Loop` 即可（Bevy 0.19 原生）。

### 2.3 GLB 兼容性红线（选素材时过滤）

- ❌ **`KHR_draco_mesh_compression` 不支持**：下载 GLB 后如加载报错/空白，先用 Blender 重新导出（不勾选 Draco）再试。Poly Pizza 部分模型默认提供 Draco 压缩版，注意选非 Draco 下载项。
- ❌ `KHR_materials_variants`、`KHR_texture_basisu` 等不支持（见 bevy_gltf lib.rs 扩展表）。
- ✅ 常规 PBR、自发光（emissive）、透明度、`KHR_materials_unlit`（无光照材质，适合像素风！）均支持。
- **FBX → GLB**：用 Blender 一键导出（File → Export → glTF 2.0，格式选 .glb），骨骼动画一般可保留。
- **面数**：推荐包均为低模/体素（数百~数千面），无压力；同屏实体多，注意合并材质批次。

---

## 3. 🎵 音效素材清单（许可已核实）

> 通用原则：优先 CC0 / Public Domain / MIT；CC-BY 需署名（项目内建署名清单）；**排除 NC / SA / GPL**。

### 3.1 最推荐（零署名组合）

| # | 素材包 | 来源 | 许可证 | 格式 | 适配说明 |
|---|---|---|---|---|---|
| A1 | **Kenney 音频全家桶**（UI Audio / Impact Sounds / Interface Sounds / Sci-Fi Sounds / RPG Audio） | https://kenney.nl/assets/ui-audio （同站其他包同理） | **CC0，免署名** | OGG/WAV | 游戏通用音效最省心来源：UI 点击、拾取、爆炸、脚步全覆盖；像素风契合度最高。每包 50–150+ 条 |
| A2 | **Sonniss #GameAudioGDC 免费大包**（2015–2026 历年） | https://sonniss.com/gameaudiogdc/ ；许可证 https://sonniss.com/gdc-bundle-license/ | **自定义宽松**：可商用、免署名；**禁把原始素材作为独立音效库转售/再分发** | WAV(24-bit) | 专业录音棚级素材底库：引擎、爆炸、环境垫、生物、机器人全都有。每年 GDC 更新（2024 版 27.5GB+） |
| A3 | **Signature Sounds CC0 系列**（Space & Spaceship / Footsteps on the Moon / Cave Atmospheres / 雨 / 风 / BRAAAMS 等 10+ 包） | https://signaturesounds.squarespace.com/store/p/space-spaceship-sound-effects-free-download-cc0-wav-sample-pack- 等 | **CC0，免署名** | WAV | 科幻/太空/环境针对性最强，直接对口本作太空阶段与月面星球 |
| A4 | **OGA 512 Sound Effects (8-bit style)** + **qubodup Audio** | https://creazilla.com/media/audio/15530273/512-sound-effects-8-bit-style ；https://opengameart.org/content/qubodup-audio-cc0 | **CC0，免署名** | WAV/OGG | 8-bit 复古音效，适合 UI 与合成音点缀 |
| A5 | **Creazilla 站内音频**（站内全部 CC0）：60 CC0 Sci-Fi SFX、63 Digital Sound Effects（激光/相位枪/太空）、50 CC0 Retro/Synth SFX 等 | https://creazilla.com/media/audio/15534159/60-cc0-sci-fi-sfx ；https://creazilla.com/media/audio/15535620/63-digital-sound-effects-lasers-phasers-space-etc. | **CC0，免署名** | WAV | 激光/武器/扫描/警报/曲速类齐备 |

### 3.2 环境与生物音效

- 环境：洞穴回声（Signature Sounds Cave Atmospheres）、雨（Light Rain / Nighttime Rain / Rain Hitting Window）、外星风（Lunar Wind Vol.1）、海浪（Beach Ambience / Waterfall）、熔岩（Sonniss fire/lava 分类）。
- 生物：Creazilla Sci-Fi Aliens and Cows Pack（CC0）、16 Monster Growls（CC0）、Pixabay 动物叫声（鹿等，Pixabay 内容许可：免费商用免署名，禁原样转售）。
- 无人机/机器人：Sonniss robot/drone 分类 + Signature Sounds 空间包。

### 3.3 BGM

- **零署名**：Pixabay Music（https://pixabay.com/ ，免费商用免署名，禁独立再分发）、FreePD.com（Public Domain）。
- **需署名**：Incompetech（Kevin MacLeod，CC-BY 4.0，科幻/氛围曲目多）——用则需在 Credits 写 "Kevin MacLeod (incompetech.com)" + CC-BY 4.0 链接。

### 3.4 ⚠️ 音效避坑

- OpenGameArt / Freesound：**混合许可**，逐条看页面右上角许可证；曾见 "Warp sound 1"、"Space Walk" 为 GPL-3.0，一律排除。
- itch.io：免费 ≠ 可商用，逐个看 License 栏。
- freesoundsite.com / ZapSplat / Purple Planet / FesliyanStudios：免费档许可不透明或要求署名/付费，**不建议**。

---

## 4. 🐾 生物模型素材清单（许可已核实）

> 风格匹配度排序：体素方块风 > 低多边形扁平着色 > 其他。项目生物格子约 0.25~1 格。

### 4.1 最推荐（全部 CC0 免署名）

| # | 素材包 | 来源 | 许可证 | 动画 | 格式 | 适配说明 |
|---|---|---|---|---|---|---|
| B1 | **Quaternius Ultimate Animated Animal Pack** | https://quaternius.com/packs/ultimateanimatedanimals.html | **CC0** | ✅ 骨骼动画（Idle/Walk/Gallop/Jump/Death 等） | glTF/FBX/Blend/OBJ | 12 种动物，当前已下载 Alpaca/Deer/Fox/Wolf 四个 glTF 并接入 Bevy |
| B2 | **Kenney Animal Pack Remastered** | https://kenney.nl/assets/animal-pack-remastered | **CC0** | ❌ 静态 | 多格式含 glTF | **方块风格**（鹿/牛/羊/熊等 10+），与 16×16 体素风最契合；无动画可用现有呼吸/浮动假动画 |
| B3 | **KayKit Character Pack: Adventurers** | https://github.com/KayKit-Game-Assets/KayKit-Character-Pack-Adventures-1.0 | **CC0** | ✅ 基础动画 | **GLB** + FBX | 4 个低多边形人形角色 → 村民/NPC 位 |
| B4 | **KayKit Character Pack: Skeletons** | https://github.com/KayKit-Game-Assets/KayKit-Character-Pack-Skeletons-1.0 | **CC0** | ✅ | GLB + FBX | 4 个骷髅 → 敌对怪物位 |
| B5 | **KayKit Character Animations**（动画库） | https://kaylousberg.itch.io/kaykit-character-animations | **CC0** | ✅ | FBX/glTF | 配合 KayKit 角色用 |
| B6 | **Quaternius Animated LowPoly Dinosaurs** | https://quaternius.itch.io/animated-lowpoly-dinosaurs | **CC0** | ✅ 骨骼动画 | FBX/glTF | 10+ 恐龙 → 外星/巨型生物 |
| B7 | **Quaternius Ultimate Monsters Pack** | https://sketchfab.com/3d-models/ultimate-monsters-pack-fd72e114d119488da71fe3a16f216c4f | **CC0** | 部分 | GLB/FBX | 30+ 怪物（含史莱姆/软体怪）→ 史莱姆位 |
| B8 | **Quaternius Ultimate Space Kit** | https://sketchfab.com/3d-models/ultimate-space-kit-84c108ff2bcf4d4cbf2adff74a942822 | **CC0** | 多为静态 | GLB | 机器人/无人机/舱体 → 遗迹守卫无人机位 |
| B9 | **Quaternius 3D Animated Robots** | https://gdevelop.io/asset-store/free/3d-animated-robots-3d-animated-robots | **CC0** | ✅ | 多格式 | 机器人 → 无人机/机械敌人 |
| B10 | **Poly Pizza**（体素/低模生物搜索：voxel / animal / crab / slime） | https://poly.pizza | 逐模型 **CC0 / CC-BY** | 多静态 | **GLB** | ⚠️ CC-BY 条目需署名；**注意选非 Draco 压缩下载项** |

### 4.2 替换约束（重要）

- 替换模型时**保留 `Creature` 组件的 radius/height 碰撞参数**（逻辑不依赖外观）。
- 动画驱动参考 §2.2 的 0.19 动画指引（`AnimationPlayer` + `AnimationGraph`）。
- 静态模型（Kenney 系）可用现有假动画（呼吸缩放/浮动/旋转）弥补，或只在远处作为装饰。

---

## 5. 🚀 飞船 / 太空素材清单（许可已核实）

| # | 素材包 | 来源 | 许可证 | 格式 | 适配说明 |
|---|---|---|---|---|---|
| C1 | **Kenney Modular Space Kit** | https://kenney.nl/assets/modular-space-kit | **CC0** | OBJ/GLTF | **模块化飞船零件**——最贴合替换现有程序化拼装：保持"零件组装"架构，只换零件模型 |
| C2 | **Quaternius Ultimate Spaceships Pack** | https://godotengine.org/asset-library/asset/1674 | **CC0** | GLTF | 多艘低模飞船（小型→大型），天然覆盖 C/B/A/S 等级差异化 |
| C3 | **Kenney Space Station Kit** | https://kenney.nl/assets/space-station-kit | **CC0** | 多格式 | 空间站模块 → 站体外观（可选） |
| C4 | **KayKit Space Base Bits** | https://kaylousberg.itch.io/space-base-bits | **CC0** | 多格式 | 太空基地模块（可选） |
| C5 | **itch.io Free CC0 3D Sci-fi Props**（14 艘飞船+组件） | https://itch.io/games-like/2434944/free-cc0-scifi-props | CC0（以商品页为准） | 多格式 | 等级差异化补充 |
| C6 | **GDevelop 3D Spaceships / 3D Space Station**（Quaternius CC0 再打包） | https://gdevelop.io/ru-ru/asset-store/free/3d-spaceships-3d-spaceships | **CC0** | 多格式 | 同上位素材备选 |
| C7 | **Kenney Voxel Pack** | https://www.kenney.nl/assets/voxel-pack | **CC0** | 多格式 | 体素道具/装饰 |

### 5.1 ⚠️ 飞船避坑

- **Kenney "Space Shooter Remastered / Extension" 是 2D 精灵不是 3D 模型**，勿误用（已核实）。
- 上述飞船包多为静态模型：**尾焰/推进光效保留现有实现**（`spawn_ship` 返回的 `flames` 实体 + 自发光材质），成本最低。
- Sketchfab 下载注意许可证筛选（Quaternius 账号为 CC0，其他作者逐模型核对）。

---

## 6. 落地实施路线图（建议按 Phase 顺序执行）

### Phase 1 —— 音效替换（收益最大，改动最小，1~2 天）

1. 下载 A1 Kenney 音效包，挑选与现有 12 个 SFX 对应的文件（dig/place/break/jump/hurt/pickup/click/craft/jet/laser/error/alarm），转 OGG（体积小）放入 `assets/audio/`。
2. 改 `src/audio.rs`：`Sfx::build` 改为接收 `&AssetServer`，用 `asset_server.load::<AudioSource>("audio/dig.ogg")` 加载；**保留程序合成为回退**（或删掉合成代码，二选一，建议保留以零依赖兜底）。
3. 新增音效：飞船引擎（循环）、爆炸、曲速跃迁、生物叫声、环境垫（可选），在对应调用点用现有 `audio::play()` 触发。
4. 可选：主菜单/游戏内 BGM（`PlaybackMode::Loop`）。
5. **验收**：`cargo run --release -- --smoke` 通过；进游戏逐个触发音效无缺失、无爆音。

### Phase 2 —— 飞船模型替换（1~2 天）

1. 下载 C2 Quaternius Ultimate Spaceships（或 C1 Kenney 模块化），转 GLB 放 `assets/models/ships/`。
2. 改 `src/space.rs::spawn_ship`：把 15 个 `Cuboid` 分支替换为 `WorldAssetRoot(asset_server.load(GltfAssetLabel::Scene(0).from_asset(path)))`；C/B/A/S 四等级映射到 4 个不同模型（或同一模型不同材质）。
3. **保留**：返回 `(Entity, Vec<Entity>)` 签名、尾焰实体（用现有发光 Cuboid 或粒子）、`ShipClass` 逻辑、碰撞（飞船按半径球体 `SHIP_R` 处理，见 `station.rs::resolve_station_collision`）。
4. 注意模型缩放/朝向：程序化飞船以 -z 为前、长 3.6 格；GLB 若朝向/尺寸不一致，用 `Transform` 修正（或改 `assets` 时在 Blender 里对齐）。
5. **验收**：登船→起飞→大气飞行→太空→泊入空间站全程正常；`cargo test` 通过（换系往返测试与模型无关，应不受影响）。

### Phase 3 —— 生物模型替换（2~3 天，含动画）

1. 下载 B1 Quaternius Animated Animals（或 B2 Kenney Animal Pack 静态版），转 GLB 放 `assets/models/creatures/`。
2. 改 `src/creatures.rs::spawn_creature`：`kind` → 模型路径映射（crab/blob/strider 或按生态选模型）；`Creature` 组件字段（radius/height/hp）保持不变。
3. 动画：按 §2.2 指引接入 `AnimationPlayer`；最简单的第一版可只播放 Idle，Walk 后续再加。
4. 掉落物、村民（char.rs）、无人机可后续按同模式替换。
5. **验收**：`cargo run --release -- --smoke` 通过；生物生成/游荡/死亡/掉落正常；`cargo test` 全绿。

### Phase 4 —— 空间站 / 太空视觉（可选，后期）

- C3 Kenney Space Station Kit 或 C4 KayKit Space Base Bits 替换站体外观（碰撞盒 `station_cols()` 保持不动）。
- 星球贴图、恒星、小行星保持程序化（有确定性测试，不建议动）。

---

## 7. 许可合规规范（必做）

### 7.1 建 `starforge-bevy/CREDITS.md`（或 `licenses/` 目录）

模板：

```markdown
# STARFORGE Bevy 移植版 · 第三方素材 Credits

> 下载日期以各条目为准；许可证以资源页实际标注为准，本表为登记簿。

## 音效
| 素材 | 来源 | 许可证 | 署名要求 | 入库路径 |
|---|---|---|---|---|
| Kenney UI Audio | https://kenney.nl/assets/ui-audio | CC0 1.0 | 无 | assets/audio/ui_*.ogg |

## 模型
| 素材 | 来源 | 许可证 | 署名要求 | 入库路径 |
|---|---|---|---|---|
| Quaternius LowPoly Animated Animals | https://quaternius.itch.io/lowpoly-animated-animals | CC0 | 无 | assets/models/creatures/ |

## 需署名条目（CC-BY）
（如有，逐条列：素材名 / 作者 / 署名文本 / 许可链接）
```

### 7.2 下载当日复核清单（每批素材执行一次）

1. 打开资源页面，核对许可证原文是否为 CC0 / CC-BY / MIT / Apache-2.0 / Public Domain / 官方自定义宽松许可。
2. 出现以下任一关键词 → **弃用**：`NonCommercial`、`ShareAlike`、`GPL`、`LGPL`、`CC-BY-NC`、`CC-BY-SA`、`CC-BY-ND`。
3. CC-BY 条目记录作者署名文本与许可链接。
4. GLB 文件确认非 Draco 压缩（或已在 Blender 重新导出）。
5. 把许可证原文截图/存档进 `licenses/`（Sonniss 包尤其要留档当年许可页）。

### 7.3 发行物要求

- 若使用任何 CC-BY 素材：随游戏发行 `CREDITS.md` 或 `THIRD_PARTY_NOTICES.md`，包含署名 + 许可链接。
- CC0 素材无需署名，但建议仍登记来源（方便追溯与替换）。

---

## 8. 避坑清单（总）

1. **Bevy 0.19 场景 API 已重写**：`SceneRoot` / `Animator` 组件不存在；用 `WorldAssetRoot`（`bevy_world_serialization::prelude`）+ `GltfAssetLabel`；动画参考 0.19 官方示例，勿抄旧版代码。
2. **Draco 压缩 GLB 不支持**：加载空白/报错先查这个（Poly Pizza 常见）。
3. **混合许可平台**（OpenGameArt / Freesound / Poly Pizza / Sketchfab / itch.io）：逐条核对，NC/SA/GPL 一律跳过。
4. **Kenney Space Shooter Remastered 是 2D 精灵**，不是 3D 模型。
5. **freesoundsite / ZapSplat / Purple Planet / FesliyanStudios** 许可不透明或需付费免署名，不用。
6. **Sonniss 包**：可商用免署名，但禁止把原始素材作为独立音效库再分发（游戏内使用完全没问题）。
7. **Pixabay / Mixkit**：免费商用免署名，但禁止原样转售/独立再分发。
8. **免费 ≠ 可商用**：itch.io 每个包看 License 栏。
9. 音效入库转 OGG（WAV 体积大）；Bevy 用 MP3/FLAC 需在 `Cargo.toml` 加 feature。
10. 替换模型时**不要动**：`Creature` 组件字段、`spawn_ship` 返回签名、`station_cols()` 碰撞盒、程序化贴图（有确定性回归测试）。

---

## 9. 素材来源总表（速查复制用）

### 音效
- Kenney 音频全家桶：https://kenney.nl/assets/ui-audio （CC0）
- Sonniss GDC 大包：https://sonniss.com/gameaudiogdc/ + https://sonniss.com/gdc-bundle-license/ （宽松，免署名）
- Signature Sounds CC0 系列：https://signaturesounds.squarespace.com/store/p/space-spaceship-sound-effects-free-download-cc0-wav-sample-pack- 等（CC0）
- 512 8-bit 音效：https://creazilla.com/media/audio/15530273/512-sound-effects-8-bit-style （CC0）
- Creazilla Sci-Fi/激光/合成：https://creazilla.com/media/audio/15534159/60-cc0-sci-fi-sfx 等（CC0）
- OGA qubodup Audio：https://opengameart.org/content/qubodup-audio-cc0 （CC0）
- Pixabay Music / 动物叫声：https://pixabay.com/ （免署名）
- Incompetech BGM：https://incompetech.com/music/royalty-free/faq.html （CC-BY 4.0，需署名）

### 生物模型
- Quaternius Animated Animals：https://quaternius.itch.io/lowpoly-animated-animals （CC0）
- Kenney Animal Pack Remastered：https://kenney.nl/assets/animal-pack-remastered （CC0）
- KayKit Adventurers：https://github.com/KayKit-Game-Assets/KayKit-Character-Pack-Adventures-1.0 （CC0）
- KayKit Skeletons：https://github.com/KayKit-Game-Assets/KayKit-Character-Pack-Skeletons-1.0 （CC0）
- KayKit Animations：https://kaylousberg.itch.io/kaykit-character-animations （CC0）
- Quaternius Dinosaurs：https://quaternius.itch.io/animated-lowpoly-dinosaurs （CC0）
- Quaternius Monsters：https://sketchfab.com/3d-models/ultimate-monsters-pack-fd72e114d119488da71fe3a16f216c4f （CC0）
- Quaternius Space Kit：https://sketchfab.com/3d-models/ultimate-space-kit-84c108ff2bcf4d4cbf2adff74a942822 （CC0）
- Quaternius Robots：https://gdevelop.io/asset-store/free/3d-animated-robots-3d-animated-robots （CC0）
- Poly Pizza：https://poly.pizza （逐模型 CC0/CC-BY）

### 飞船 / 太空
- Kenney Modular Space Kit：https://kenney.nl/assets/modular-space-kit （CC0）
- Quaternius Ultimate Spaceships：https://godotengine.org/asset-library/asset/1674 （CC0）
- Kenney Space Station Kit：https://kenney.nl/assets/space-station-kit （CC0）
- KayKit Space Base Bits：https://kaylousberg.itch.io/space-base-bits （CC0）
- itch Free CC0 Sci-fi Props：https://itch.io/games-like/2434944/free-cc0-scifi-props （CC0，以页面为准）
- GDevelop 3D Spaceships：https://gdevelop.io/ru-ru/asset-store/free/3d-spaceships-3d-spaceships （CC0）

### 参考链接
- Bevy 0.19 Cargo features：https://raw.githubusercontent.com/bevyengine/bevy/v0.19.0/docs/cargo_features.md
- Bevy 0.19 动画示例：bevy 仓库 `examples/animation/`（本地缓存可查 `bevy_animation-0.19.1` 源码）
- CC0 资源大全：https://github.com/madjin/awesome-cc0
