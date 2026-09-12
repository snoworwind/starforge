# 00 · 现状审计与视觉工程架构

## 0.1 事实、风险与建议必须分开

以下行号来自编制时基线，文件路径相对 `starforge-bevy/`。`已确认` 指本次读到的代码事实；`待验证` 指潜在问题；不根据 README 宣称功能缺失或已完成。

| 子系统 | 已确认的入口与实现 | 约束与改进方向 |
| --- | --- | --- |
| 依赖 | `Cargo.toml` / `Cargo.lock`：Bevy 0.19.1、bevy_egui 0.41；edition 2024 | 固定锁文件验证，不能把最新版网页示例直接拼入工程 |
| 相机 | `main.rs:498` 附近：Hdr、DepthPrepass、ContactShadows、SSAO、Bloom、AcesFitted、EV100 13、Msaa::Off | 已有高级效果；需测正常分辨率边缘稳定性、曝光、组合顺序及预算 |
| 大气 | `daynight.rs`：Atmosphere、环境图补光、昼夜/F3 调参；`PlanetScaleProfile` 统一半径和高度 | 保留已有修复；避免恢复曾失效的曝光范围和过强阳光补偿 |
| 阴影 | `main.rs` 4096 阴影贴图；`daynight.rs` 级联；接触阴影长度 2.5 | 单一固定配置不适合所有硬件；屏幕接触阴影不能代替真正遮挡 |
| 近地表 | `materials.rs:69`：256×256 sRGB 图集，Nearest；固体与植物共享 Mask(0.4)、双面、无背面剔除的 StandardMaterial | 材质物理类别、opaque/cutout/transparent 分桶有提升空间；不能直接线性过滤整个旧图集 |
| 水 | `materials.rs:113`：Blend、双面、粗糙度 0.18、metallic 0.12；材质和顶点均有颜色/alpha 路径 | 需核对重复着色/alpha 乘积、物理材质、透明排序；不能仅据此断言运行画面错误 |
| 纹理 | `textures.rs` 的 TS=16、Painter、Atlas::build；注册顺序与 tile index 绑定；UI 程序化图标 | 要为数据 ID 和视觉 ID 解耦、mip、防串色、材质通道做迁移 |
| 网格 | `world.rs:1477` build_chunk_meshes、逐面 shading、corner_ao；近景绝对坐标 | 有顶点 AO，不是零 AO；加入法线贴图前核对 tangent 和 face UV；体素逻辑坐标不能随视觉曲率改变 |
| 远景 | `lod.rs` 32³ 节点、2–8 层、每帧构建预算 4；`world.rs` legacy far mesh；曲率主 pass/prepass 两份 WGSL | 已有 LOD；必须收敛 fallback、多重遮挡、裂缝、修改传播与实际消耗 |
| 云 | `weather.rs` 自定义 CloudShellMaterial、192×32×192 密度纹理、默认24步；`cloud_shell.wgsl` 最多64视线步、4光线步、HG相函数、深度截断、预乘alpha | 已有体积云和基础自阴影；需改善形态、时间稳定、真实分辨率控制、云影和跨尺度一致性 |
| 云设置 | CloudTuning 保存 render_resolution，源码读取主要用于旧设置兼容；当前球壳材质直接在相机目标绘制 | 不把旧分辨率字段当作已生效的独立低分辨率云通道；注释也有演进残留 |
| 太空云 | `weather.rs:808` space_cloud_system：两层透明球壳旋转 | 与近地密度是不同表现路径，须统一宏观天气图和相位后再谈无缝交接 |
| 高度 | `planet_scale.rs` 半径16384，云420–780，大气顶2800，再入1800、离开2400 | 这是游戏尺度；不要用真实地球参数硬套，也不要另造第五套高度阈值 |
| 建筑 | `world.rs:718` 结构格哈希；640×320格，候选概率0.35；村庄围中心撒小屋；`stamp_hut:983` 固定5×5、固定门向、平屋顶；遗迹3种 | 必须提升选址、街道、地基、形态、内部、可达性、加载确定性，而非只改墙纹理 |
| 植被 | `world.rs` tree_at、decor_column、生成树/菌；`wildlife.rs` 兽群反馈 | 逻辑植被与纯装饰要分层；装饰新增不得改变资源分布和采矿射线 |
| 工厂 | `factory.rs` 25个具体 MachineKind 加 Other；`machine_fx.rs` 粒子/灯池/运输物品已有 | 重做表现层保留电力、库存、生产权威状态，不把动画当生产计时器 |
| 角色 | `char.rs:119` spawn_humanoid 参数 `_appearance` 未使用，按位置选NPC模型 | 此函数尚未应用完整捏人数据；需明确玩家/NPC外观映射，不声称整个项目没有外观系统 |
| 生物 | `creatures.rs` GLB/程序化体块、动画库、确定性兽群及存档 | 扩展动作/材质/落地，不改变永久死亡、出生掩码和资源掉落规则 |
| 太空 | `space.rs` 大文件含坐标互逆、飞行、外部船、程序化船、轨道星球纹理；station.rs 模型包围盒/碰撞与停泊 | 渲染拆出，避免为细船壳破坏已修复的高速碰撞和空间站通道 |
| UI | `ui.rs` HUD、背包、科技、机器、贸易、星图、字体、手动egui pass；多模块补充面板 | 设计系统、输入路由、业务命令、视图模型分离；不能在迁移中重复执行交互 |
| 粒子/镜头 | particles、screen_fx、camera_fx、photo 均已有，粒子资产缓存和上限已有 | 基于现有框架改善遮挡、材质、状态优先级、运动舒适度，避免再造第二套 |
| 存档/联机 | `save.rs` SAVE_VERSION=5；network 原生权威UDP；schedule.rs GameSet 全链顺序 | 美术资产版本与世界生成版本分离；纯画质配置不改变世界指纹 |
| CI | `.github/workflows/` fmt/check/clippy/test，全targets/features，Linux runner | 无法据此证明 Windows GPU 效果通过；必须新增真实渲染验证 |
| 本机探针 | `examples/shadow_probe.rs` 存在但根.gitignore忽略整个examples；当前代码没有创建方向光 | 不能把它当正式阴影验收；正式测试目录需受版本控制，临时探针不计交付 |

README 仍提到旧 TerrainExtension、250 半径、旧 shader 文件名及 vendored 云库；当前清单与依赖没有对应 vendor 云接入。只将其记为文档漂移，实施时确认资产许可历史后修订，不能贸然删除现有署名。已有审计报告中的“通过”属于报告当时的版本，不是本次重测结果。

## 0.2 非功能边界

1. Bevy 主工程内改造，legacy-web 仅供历史参考。不要为了兼容旧 JS 纹理字节而限制新艺术风格，但数据迁移要有证据。
2. 不重写战斗、物流、电力和世界坐标主逻辑来实现装饰。新逻辑性建筑、门、楼梯确有碰撞需求时作为 G 的显式兼容变更。
3. 不默认引入光追、Nanite式几何系统、全动态GI、流体模拟或第三方复杂插件。先验证单项收益、支持平台、许可证和长期维护成本。
4. 源码、本地素材、引擎版本都可能在后续变更；每个工作包开始要重读相关函数和最近提交，不能以规划文字覆盖实际代码。
5. 不把制作图像、购买资产、发布游戏混为一项授权；后续素材采购按团队实际流程，免费可用素材也必须完成许可登记。

## 0.3 拟建目录及拆分步骤

```text
src/
  visual/                 # 配置、视觉帧快照、质量解析、统计、生命周期
  art/                    # 材质注册、资产描述/校验、图标与导入适配
  rendering/              # 相机、曝光、光照、阴影、天空、合成、反射
  weather/                # weather.rs迁移后：天气状态、云场、云渲染、降水
  terrain/                # meshing、采样、LOD、流式工作队列、修改传播
  environment/            # 植被、碎石、水表现、积雪湿润、生态配置
  structures/             # 场地、布局、语法、地基、道路、内部、验证、版本
  factory_visuals/        # 机器注册、动画、接头、灯光、物流物件
  actor_visuals/          # 角色外观、生物、船、站、动画LOD
  ui/                     # ui.rs迁移后：theme、widgets、view_models、screens
  visual_qa/              # 可版本控制的场景/采集运行器
assets/art/               # 稳定资产路径及manifest；源工程另定目录
assets/shaders/visual/    # 共享曲率、材质、云、合成函数
tests/                    # 算法/兼容/生命周期/截图元数据校验
```

这些目录是职责图，不要求一次性创建空目录或空插件。Rust 同名 `ui.rs` 与 `ui/mod.rs`、`weather.rs` 与 `weather/mod.rs` 不并存为同一模块入口。操作顺序：先抽私有子模块并保留薄适配入口→跑旧行为回归→更新引用→单独提交迁移→再做视觉变更。不要一次重排全部 schedule；逐个消除实际依赖，允许无依赖的视觉读取并行，权威系统顺序保持。

## 0.4 公共数据契约（拟建类型，不是现有 API）

| 契约 | 必含数据 | 生产者 → 消费者 | 不变量 |
| --- | --- | --- | --- |
| `VisualFrame` | frame_id、world_epoch、camera_id、前后相机矩阵、原点偏移、局部星球框架、尺寸、dt、暂停标志 | A统一采样 → D/E/F/I/J | 同一帧的云/阴影/雾使用同一坐标框架；不用各自取一半旧状态 |
| `CelestialLighting` | 恒星方向/色/能量、昼夜因子、环境补光、曝光策略 | D → E/F/H/I | 向光方向与光线传播方向命名分开，统一单位 |
| `WeatherSample` | weather_id、覆盖/降水/风/湿度/温度、过渡相位、宏观图版本 | E天气状态 → 云/树/雨/湿润/UI/音频 | 逻辑天气与纯视觉扰动分离；确定性种子不由帧率推进 |
| `SurfaceMaterialId` | 稳定ID、材质族、纹理层、UV策略、PBR参数、脚步/破碎标签 | B → C/F/H/I | 不复用方块u8当纹理数组层；登记ID永不隐式重排 |
| `ChunkVisualRevision` | 世界epoch、区块坐标、修改revision、邻居revision、生成/风格版本 | C → mesh/LOD/装饰/地图 | 异步返回四项不匹配则丢弃；不能旧任务覆盖新矿洞 |
| `StructurePlan` | stable_id、边界AABB、逻辑体素/碰撞、模块、道路/门/导航、实体锚点、种子和版本 | G → C/NPC/任务/存档 | 与chunk加载顺序和画质无关；不保存临时Entity |
| `MachineVisualState` | kind、朝向、端口、负载、供电、进度、堵塞/缺料、稳定机器ID | factory适配 → H/J | 只读生产状态；表现不扣物品、不生成电力 |
| `QualityProfile` / `ResolvedQuality` | 用户请求、设备支持、预算、最终有效设置、降级原因 | A → 各视觉流/J | UI显示生效值；不支持不是silent no-op |
| `VisualEvent` | 源stable_id、事件id、位置、法线、强度、表面标签、时刻 | 玩法适配 → VFX/镜头/声 | 联机重复事件去重；截图可固定随机种子；连续状态不每帧冒充事件 |

定义步骤：为每字段写单位和坐标空间→默认/范围/非有限值处理→序列化边界→最小使用者→测试→冻结v1。只有确实需要跨边界时才增字段。可先采用现有serde JSON，不能为了“架构感”引入新配置语言和反射框架。

## 0.5 更新与渲染顺序

应用侧建议：权威模拟完成→应用网络/建造结果→更新视觉原点/相机最终姿态→抽取只读视觉快照→推进纯表现状态→上传渲染数据。`camera_fx` 加到最终相机后，云重投影使用该最终矩阵；UI定位也从同一相机取得。不能让每个插件各写一次玩家/飞船相机。

渲染侧建立明确依赖图：必要的深度/法线/运动向量→阴影和opaque主场景→天空大气→体积场积分→水/玻璃与体积的有序合成→透明粒子→抗锯齿/重建→曝光相关Bloom与调色→产品UI。**这是数据依赖示意，不是已验证的Bevy节点顺序**。D01/E01必须依据锁定版本实测选择插入点；如果云集成发生在tone map前，其颜色必须为场景线性HDR；屏幕后处理不得二次色调映射UI。

透明物体不能简单统一放云前或云后：近水、近玻璃、云前粒子与云内飞船各不同。E05/D06应约定“到透明表面深度的体积透射”接口或分层合成；低档保留明确的近景优先降级，测试云沿岸、玻璃罩、引擎烟，而非假定opaque depth能解决全部透明遮挡。

## 0.6 生命周期、异步与资源管理

1. 每次新世界、星球切换、读档和退菜单递增 `world_epoch`。所有生成任务、GPU history、装饰索引和截图请求带epoch。
2. 建立资源拥有者表：应用常驻（字体/图标/共用噪声）→世界（地形材质/天气）→星球（地貌纹理/植被）→区块（mesh/局部装饰）→相机（深度/历史/反射）→实体（动画实例）。销毁顺序从叶至根。
3. 返回菜单先取消任务/断开消息源，再移除实体，再清空引用句柄与缓存。`InGame` 标记沿用，但不假定仅despawn会释放仍被强句柄持有的纹理。
4. 工作线程接收不可变局部采样快照，返回CPU数据；主线程验证revision后创建/替换Bevy资产。不要把ECS查询、可变World或未确认可跨线程的GPU句柄搬入任务。
5. 设置队列容量、每帧CPU时间预算和GPU上传字节预算；有限任务数不等于有限耗时。大云纹理、星球图和建筑生成不能在重建场景时一次阻塞主线程。
6. 共享材质不每实体clone；需要外观变化用有界variant缓存或实例参数。LUT/噪声由内容哈希缓存并计数，热重载失败保留上一有效版本。
7. 设备丢失、resize到0、最小化、切全屏、窗口DPI变化：暂停创建零尺寸纹理，恢复时重建相机资源，清除history并重新验证格式。
8. 每个模块暴露实体数、材质数、纹理估算、任务队列、拒绝旧结果数量、历史重置原因。指标进入统一诊断，不散落在HUD每帧打印。

## 0.7 配置与世界兼容

分开 `settings_schema`、`visual_style_version`、`material_catalog_version`、`generator_version`、`save_schema`、`network_protocol`。切低画质只更换视觉，不能换建筑、道路、树木可采资源和宝箱。世界生成器变化必须进G06：旧档继续用旧生成器或受保护的按区域新版本，不能把缺省字段直接当最新版。

现有u8方块ID和RLE编码不可随意增长到>255。新增外观优先在方块ID外挂视觉变体；若确需更宽ID，另设迁移和协议工作包并重估预算。新屋顶/门窗装饰的可碰撞性必须一致：有玩法碰撞就同步和保存，无玩法意义就不挡人、不挡射线。

## 0.8 官方参考及使用边界

本次已阅读下列官方来源，访问日期2026-09-12。在线示例可能随主线更新，**仅用于原理和接口核对；最终编译以本地锁定0.19.1源码和可运行最小样例为准**。docs.rs 的0.19.1页面本次未能读取，不能据此更改项目版本或声称该版本不存在。

- 材质扩展可沿用标准PBR输入/光照，并注意主通道和prepass差异；见 [Bevy Extended Material](https://bevy.org/examples/shaders/extended-material/)。本项目自有曲率材质就是优先扩展入口。
- 自定义后处理需要正确读取/写回视图目标并适配HDR格式；见 [Bevy Custom Post Processing](https://bevy.org/examples/shaders/custom-post-processing/)。实际调度API要在锁定版本上验证。
- 大气的场景尺度、相机和光照配置可参照 [Bevy Atmosphere](https://bevy.org/examples/3d-rendering/atmosphere/)，不能照搬真实尺度到本项目的小星球。
- 模型材质、纹理通道、坐标与alpha语义按 [Khronos glTF 2.0 Specification](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html)核对，B负责将格式规范落实为项目导入校验。
- UI对比度以 [W3C WCAG 2.2 Contrast Minimum](https://www.w3.org/TR/WCAG22/#contrast-minimum)作为设计参考：普通文字4.5:1，大字3:1；此处是游戏UI内部目标，不宣称整个游戏符合Web无障碍认证。
