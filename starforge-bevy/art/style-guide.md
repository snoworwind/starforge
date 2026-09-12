# STARFORGE 美术风格与尺度规范（B01）

> 版本：1；对应 `ART_STYLE_VERSION = 1`（`src/art/style.rs`）。
> 适用范围：B/C/D/E/F/G/H/I/J 所有新增与替换资产。本文是执行规范，不描述尚未制作的资产。
> 资产清单以 `--art-audit` 生成的 `target/art-audit/art-manifest.json` 和 `src/art/manifest.rs` 为准；
> README/CREDITS 只作为来源登记，不作为“代码实际引用了什么”的依据。

## 1. 形态与构图

1. **世界尺寸基准是一格方块（1 m）**。所有尺寸取自 `SCALE_REFERENCES`（`src/art/style.rs`），不凭截图猜测：

   | 锚点 | 数值 | 来源符号 |
   |---|---:|---|
   | 方块 | 1.0 m | `data.rs` 网格 |
   | 玩家眼高 | 1.62 m | `player.rs EYE` |
   | 人形目标身高 | 1.9 m | `char.rs npc_scale` |
   | 门净空 | 2.0 m | `world.rs stamp_hut` 门洞两格 |
   | 小屋墙高 | 3.0 m | `world.rs stamp_hut` 墙体 |
   | 停泊船体 Y 包络 | 1.25 m | `space.rs SHIP_BOX[1]` |
   | 飞船碰撞半径 | 3.0 m | `space.rs SHIP_R` |

2. **三档信息频率**：远景可辨轮廓（山脊、村庄塔、机器烟囱）→ 中景结构（屋檐、设备舱、管道、植被簇）→ 近景表面（磨损、粗糙度、接缝）。任何关键物体不能只有贴图噪声而缺中尺度构造。
3. 建筑用正交体素主体 + 有限斜顶/圆角装饰；机器用少量倒角和有功能解释的部件。倒角只改视觉网格，不改变体素射线命中。
4. 模块共同基准：统一枢轴、正面（frontal）、底面、连接点与网格步长。建筑门向用枚举，机器端口用逻辑方向，禁止作者任意朝向。
5. **允许**：大块色面、材质分层、有限高光、受控色板差异。**不允许**：所有表面同等噪声、全物体泛光、随处高饱和霓虹、风格相冲的原样外购模型。

## 2. 色彩与亮度

1. 全局基础色板 `BASE_PALETTE`（`src/art/style.rs`）：

   | key | sRGB | 用途 |
   |---|---|---|
   | `metal_deep_blue_gray` | `#3a4758` | 机器外壳、结构金属、数据面板 |
   | `rock_warm_gray` | `#8a8178` | 岩石/玄武岩/混凝土/石材 |
   | `wood_warm` | `#a87c4f` | 木板、结构木、货箱 |
   | `interact_cyan` | `#35e0e8` | 交互高亮、扫描字形、友好 UI |
   | `energy_amber` | `#ffb347` | 能量源、引导目标、暖色室内光 |
   | `danger_red` | `#ff6a5e` | 仅危险、伤害与破坏警告 |

2. 每生态色板（天光/雾/地表/岩石/植被/水/发光点/人工建筑 8 组）由 F01 建立；先看灰度明暗，再看饱和度，矿石不能与背景同亮度。全球基础色不变。
3. 色值以 sRGB 源值登记，进入 shader 后在线性空间计算；顶点色已线性化时不得二次 sRGB 转换。
4. 材质审稿固定曝光（`Exposure { ev100: 13.0 }`），五光况轮换：正午、阴天、暖夕阳、夜灯、室内。不得改环境补光掩盖坏 albedo。
5. 发光分三级 `EMISSION_TIERS`：`marker`（0.6，不参与 Bloom，不点灯）、`lamp`（1.6，参与 Bloom，不点灯）、`source`（4.0，参与 Bloom 且登记真实灯，受 D04 灯池预算约束）。普通白墙不得泛光。

## 3. 材质家族（审稿起点）

按 `docs/art-overhaul/01-ART-DIRECTION-AND-ASSETS.md` §1.4 表执行：土/砂/盐、岩石、草/苔藓、木、涂漆设备、裸金属、玻璃、水、冰、雪、晶体、熔岩/能量、叶/菌/膜。每族制作标准球、方块、斜坡、薄片、室内物件五件套。base color/emission 是颜色语义，normal/AO/roughness/metallic 是线性数据；glTF 的 metallic-roughness 按 G=roughness、B=metallic、occlusion R 核对。

### 3.1 稳定材质 ID 与冻结映射（B02）

1. `src/art/catalog.rs` 是唯一材质目录：62 个 `SurfaceMaterial`，`SurfaceMaterialId` 为显式常量 1..62，与旧 `textures.rs` painter 注册顺序一一对应但**不再由注册顺序推导**。`data.rs` 的 u8 方块 ID 仍是区块/存档 ABI，禁止再当纹理层索引使用。
2. 每个方块面的解析走 `catalog::face_material(block_id, face)` / `catalog::cross_material(block_id)`；face role 为 Side/Top/Bottom/Front（+X,-X,+Z 侧，+Y 顶，-Y 底）。方块网格、图标与远景平均色都消费同一组 ID。
3. 变体规则 `variant_slot(material, variants, wx, y, wz, face)` 只用稳定整数世界格 + 层高 + 面 + 材质 ID 掷骰，禁止用 Entity、加载顺序或帧号；每族目标变体数 3–6（`MaterialFamily::variant_budget`），当前 `variants=1`，B03/B04 生产后只改数字。
4. `block key + face role → material key` 与 `id → key` 各有冻结指纹（`face_fingerprint`/`id_fingerprint`）；`--art-audit` 输出 `ART_AUDIT_CATALOG` 并把完整目录写到 `target/art-audit/material-catalog.json`（B03 的输入）。未知 tile、未知 biome 地表 key、有 painter 无材质或有材质无 painter 都是硬失败。
5. `unused_materials` 是当前无方块面/biome 引用的冻结层（`belt_turn`/`furnace_on`/`gravel`/`wind_pole`），保留用于图集注册顺序兼容；接线或退役必须显式决策，不能悄悄删 painter。
6. 换贴图顺序/数组布局只允许改变 UV 与像素，不允许改变 `SurfaceMaterialId`、方块 key、存档或配方；K05 不得把不同 `catalog_face_fingerprint` 的前后帧当作配对证据。

### 3.2 纹理 mip/图集/数组与字节预算（B03）

1. `src/art/texture_pipeline.rs` 是逐材质纹理生产线（纯 CPU，`--texture-audit`）：每个 `SurfaceMaterialId` 一条 mip 链到 1×1。通道类 `Albedo`（sRGB 颜色 + 线性 alpha）/`LinearData`（ORM、roughness）/`Normal`（xy 编码、还原 z、平均后归一）分别使用正确缩小；后续新增 normal/ORM 图不再改缩小代码。
2. Albedo 缩小在线性光下做**预乘 alpha** 平均，透明 texel 不拉黑边缘颜色（R019）；alpha 通道用按目标覆盖率缩放的 4×4 Bayer 有序抖动逐级重映射，保持 `Mask(0.4)` 的阈值覆盖率（主 pass 与阴影共用同一 alpha，R016）。小于 4×4 的层级只报告不再纠正。
3. 图集 fallback：每级独立 padding，gutter 从源分辨率 4 起精确减半（4/2/1），最深有效 mip 为 `log2(gutter)`；cell 尺寸同比例缩小，因此内层 UV 在所有打包层级完全一致。`verify_bleed` 检查每级每个 padding 像素都等于其内层边缘像素，双线性足迹永不读取邻 tile（R013）。
4. 数组路径按 `materials × mip 链` 计字节；路由 `TextureArray → PaddedAtlas → ClassicAtlas` 由 `RenderCapabilities::texture_route()` 依据实际 `max_texture_array_layers`/`max_texture_dimension_2d` 与格式 filterable 决定，附拒绝原因（R117）。参考机（RTX 4060）：`max_array=2048, route=TextureArray, compression=bc7`；`--simulate-low-device`（array=1）回退 `PaddedAtlas`。
5. `--texture-audit` 写 `target/art-audit/texture-pipeline.json`（逐材质尺寸/字节/覆盖误差、padding、路由与原因）和 `texture-contact-sheet.bmp`（L0/L1/L2 最近邻放大堆叠）。硬失败：padding 违例、表层覆盖率阶跃 >0.15、UV 不恒定、法线归一化失败。
6. 经典 16×16 Nearest 图集仍是当前发布路径；数组/精致图集的实际绑定归 B04/C02，本工具只产出工件、预算与路由证据。

### 3.3 PBR 家族与材质庭院（B04）

1. `src/art/pbr.rs` 把 01 §1.4 的家族表变成数据：`PbrFamilyProfile` 13 条（roughness 基准/变化、metallic、法线强度、必做细节），每条自动校验落在 `MaterialFamily::roughness_range()` 内；`SurfacePbr` 给出 62 个材质的最终 roughness/metallic/描述与 `Opaque/Cutout/Blend` 分类。非金属家族 metallic 必须为 0，只有 `BareMetal` 为 0.85；发光只允许 catalog 标记 `emission` 的材质（普通表面不泛光）。
2. 程序化地图与 albedo 共用同一 16×16 网格与 UV：
   - `normal`：由 albedo 线性亮度求高度 → Sobel → 单位切空间向量，强度由家族参数烘焙（Bevy 0.19 无法线强度标量）；使用法线贴图的网格必须 `generate_tangents()`（地形网格归 C02）。
   - `ORM`：R 恒为 255，**不烘焙 AO**（顶点 AO 是唯一 AO 源，防止 R040 叠黑）；G=roughness（家族基准 ± 局部细节，clamp 在家族范围内）；B=metallic。
   - `emission`：只对 emission 材质生成亮度 mask；HDR 颜色与档位倍率放在 `StandardMaterial.emissive`（`marker 0.6 / lamp 1.6 / source 4.0`），因此普通方块不会错误发亮。
3. 发布路径：地形 `TerrainMaterials.solid` 绑定 ORM 图集（`metallic=1.0`、`perceptual_roughness=1.0` 作为乘数因子，值全部来自纹理）。`--texture-audit` 同时输出 `target/art-audit/pbr-families.json` 与 `TEXTURE_AUDIT_PBR` 行。
4. `--visual-qa --visual-qa-scene B04` 生成 PBR 家族庭院：13 家族 × 球/方块/斜坡/薄片/室内物件 5 形态 + 一间带灯房间，9 个机位覆盖正午/阴天（`LightingProbeMode::Overcast`）/暖夕阳/夜灯/室内与金属/岩石/木材/发光近景；每个 pose 的 `day_time` 写入 manifest。判读：同机位下 roughness 高低应改变反射大小、金属应有环境反射而非黑块、发光只出现在 Energy/Crystal/Foliage 发光材质。
5. 当前仍是 16×16 旧图集上的参数化；B04 后续/B05 制作 64/128 源时只替换源像素，家族参数、地图生成与庭院判读方式不变。

## 4. 资产 manifest 与审计

1. 每个被代码引用的资产在 `src/art/manifest.rs` 有一条目：稳定 key、类别、运行时路径、来源、许可证、许可证文件、角色（`live`/`reserve`/`optional_download`）、风格版本、用途备注。
   - `live`：当前代码路径实际加载；
   - `reserve`：随仓库分发但当前不实例化（例如 Kenney 船，等待最小素材 fallback 接线或删除决策）；
   - `optional_download`：README 要求下载的大模型，缺失不算失败。
2. 目录与文件命名：只允许小写 ASCII、`_` 与 `-`；路径区分大小写（打包/Linux）。审计对每个已存在资产做**逐段大小写检查**，Windows 下大小写错误也会被记为失败。
3. 运行：

   ```powershell
   cargo run --locked -- --art-audit
   # ART_AUDIT_OK / ART_AUDIT_FAIL；JSON: target/art-audit/art-manifest.json
   ```

   报告包含：缺失的必发资产、路径大小写不匹配、找不到的许可证文件、`models/`+`shaders/` 下未被 manifest 引用的 `.glb/.gltf/.wgsl`。缺必发资产或大小写错误返回退出码 2。
4. 新增资产必须先登记 manifest 再接线代码；删除或降级资产必须把对应条目改为 `reserve` 或在同一次变更中删除，不能让 dead file 默默留下。
5. 外部 CC-BY 下载物：缺包时按最小素材路径运行，不得伪造“视觉验收通过”；许可与署名由 K04/B06 与 `CREDITS.md` 合并核对。

## 5. 校准架与证据

`--visual-qa --visual-qa-scene B01` 生成同灯、同曝光、同分辨率的 B01 场景（`target/visual-qa/<commit>/B01/<run>/raw-frames/`）：

- `terrain`：草/土/沙/石/玄武岩/雪/冰；
- `machines`：熔炉/矿机/装配机/反应堆/太阳能/电池方块及其机器实体；
- `characters`：KayKit 骑士、Kenney 宇航员与外星人固定队列，脚底对齐 1.9 m；
- `building`：7×7 原木角柱 + 木板墙/屋顶 + 2 格门 + 玻璃窗 + 室内灯；
- `overview`：全部家族同屏 + 1..5 格高尺度梯。

验收判读：四类样板必须在同一 `day_time`、同一相机曝光下可比；地面/机器/角色/建筑的接触阴影、材质粗糙度与发光强度直接对照，不用调色或机位差异掩盖问题。HUD 与实时兽群仍在画面中（A01 已知限制），正式 K05 看图时以同一 run 的原始帧为准。

## 6. 常见修复顺序

| 现象 | 首查 | 修复顺序 |
| --- | --- | --- |
| 每格边缘异色线 | atlas padding、mip、UV 导数 | 可视化 UV/mip → 独立 tile mips → 极小 mip 验证 → 必要时切数组 |
| 石头像塑料 | roughness/色彩空间/法线强度 | 固定曝光 → 显示 roughness → 还原线性 → 降低微法线和高光 |
| 金属变纯黑 | 环境反射、metallic 通道 | 灰球对照 → 恢复环境图 → 查 B 通道与贴图线性 |
| 模型近看正常远处乱闪 | 细面、mip、高光、LOD | 去共面 → normal 方差/roughness → LOD 轮廓 → 时间序列复查 |
| 新资源只在开发机显示 | 大小写/路径/忽略文件 | `--art-audit` → 干净目录安装 → 可见 fallback → 补包 |
| 换色影响所有实例 | 共享材质被直接修改 | 区分共享材质/实例参数 → 有界 variant → 多人外观回归 |
