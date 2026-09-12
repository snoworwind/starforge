# STARFORGE 星穹熔炉

[中文](README.md) · [English](README_EN.md)

STARFORGE 已完成从浏览器 Three.js 版本到原生 Bevy 版本的主线迁移。当前主工程位于 [`starforge-bevy/`](starforge-bevy/)，旧版 Web 游戏、Node 联机服务器和浏览器回归测试统一归档在 [`legacy-web/`](legacy-web/)。

## Bevy 主版本

```powershell
cd starforge-bevy
cargo run --release
cargo run -- --smoke
```

功能、操作、素材许可和测试说明见 [`starforge-bevy/README.md`](starforge-bevy/README.md)。

Bevy 版全面美术提升的工程规划见 [`美术与视觉质量升级方案`](starforge-bevy/docs/art-overhaul/README.md)：包含源码现状审计、贴图与材质、光影与体积云、地形与自动建筑、工厂与角色、全界面 UI，以及约新增 5 万行 Rust 的分阶段工作包和验收要求。该文档为待实施方案。

新增 **沉浸感与表现升级**：程序化实时音乐（每颗星球/洞穴/太空/空间站/跃迁独立音景，战斗与昼夜自适应）、实时环境氛围声、雷暴/极光/沙暴/流星雨天气事件、20+ 粒子预设、镜头震动与第一人称手感、体素环境光遮蔽、外骨骼主动技能、星际图鉴（K）、40+ 成就（J）、小地图（N）、摄影模式（F2）与情境引导。

新增 **边疆公会扩展**：30 种循环委托、6 条远征（24 阶段）、16 种生态调查、12 个里程碑和可连续接取的村庄委托。游戏中按 **L** 进入，地面按 **C** 采集调查记录；兼容已有存档。详细玩法和检查记录见 [`FRONTIER_EXPANSION.md`](starforge-bevy/FRONTIER_EXPANSION.md)。

发布时请将 `assets/` 放在可执行文件旁边；Bevy 版会固定从可执行文件所在目录读取
模型、纹理、动画和着色器，不依赖启动时的当前工作目录。

> 注意：外部飞船和空间站模型体积较大，不包含在 Git 仓库中。首次运行前请按
> [`starforge-bevy/CREDITS.md`](starforge-bevy/CREDITS.md) 中的来源链接下载，并按
> [`assets/licenses/models-directory-audit.md`](starforge-bevy/assets/licenses/models-directory-audit.md)
> 的目录结构解压到 `starforge-bevy/assets/models/external/`。

## 旧版 Web 归档

旧版源码及其说明见 [`legacy-web/`](legacy-web/)；该目录不再维护或进入 CI，仅作历史归档和迁移核对，不是 Bevy 版的依赖或兼容目标。

## 迁移资料

- [`starforge-bevy/MIGRATION_REPORT.md`](starforge-bevy/MIGRATION_REPORT.md)：迁移核对报告
- [`STARFORGE_BEVY_PORT_SPEC.md`](STARFORGE_BEVY_PORT_SPEC.md)：总体移植规格
- [`SPEC_data.md`](SPEC_data.md)、[`SPEC_player.md`](SPEC_player.md)、[`SPEC_world.md`](SPEC_world.md)：核心系统规格
- [`TEXTURES_SPEC.md`](TEXTURES_SPEC.md)：程序化纹理规格

## 目录

```text
starforge/
├── starforge-bevy/   # 当前 Bevy/Rust 主工程
├── legacy-web/       # 旧 Three.js/Node.js 版本与浏览器测试
├── .github/          # Bevy 主版本的持续集成
└── *_SPEC.md         # 移植规格与核对资料
```

代码按 [MIT License](LICENSE) 发布；第三方素材许可见 Bevy 工程中的 [`CREDITS.md`](starforge-bevy/CREDITS.md)。
