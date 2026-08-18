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
