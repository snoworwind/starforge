# GitHub 自动发布（Windows x64）

`.github/workflows/release.yml` 在推送 `v*` 标签时自动生成 Windows x64 的
GitHub Release，也可在 GitHub 的 **Actions → Release → Run workflow** 中输入
已存在的标签手动运行。使用仓库自动提供的 `GITHUB_TOKEN`，无需配置额外 Secret。

## 发布新版本

1. 修改 `starforge-bevy/Cargo.toml` 的 `[package].version`，例如 `0.1.1`。
2. 在 `starforge-bevy/` 中运行 `cargo check` 更新 `Cargo.lock`，提交版本变更与发布配置。
3. 将提交推送到 `main`，再创建并推送与 Cargo 版本完全一致的标签：

   ```powershell
   git push origin main
   git tag -a v0.1.1 -m "STARFORGE v0.1.1"
   git push origin v0.1.1
   ```

当前 Cargo 版本为 `0.1.0` 时，也可以在包含发布配置的提交上创建 `v0.1.0`。
预发布版本应同时使用 Cargo 版本 `0.2.0-rc.1` 和标签 `v0.2.0-rc.1`；流程会自动
标记为 **Pre-release**，不会将它设为 Latest。

## 流程与产物

- 校验标签格式、Cargo 版本，并解析标签指向的准确提交；手动运行也构建这个提交。
- 复用 `test.yml`，执行打包测试、格式检查、`cargo check`、Clippy 和 Rust 测试。
- 使用 `windows-2022` 与 `x86_64-pc-windows-msvc` 执行锁定依赖的 release 构建，静态链接 MSVC 运行库。
- 生成 ZIP 与 SHA-256 校验文件，上传构建产物（保留 7 天）。
- 验证校验值，创建 Release 草稿，上传 ZIP 和 `SHA256SUMS.txt`，上传成功后公开发布。

ZIP 名称为 `starforge-v<版本>-x86_64-pc-windows-msvc.zip`，解压结构为：

```text
starforge-v0.1.1-x86_64-pc-windows-msvc/
├── starforge-bevy.exe
├── assets/
├── README.md
├── CREDITS.md
├── LICENSE
└── RELEASING.md
```

完整解压后运行 `starforge-bevy.exe`；必须保留旁边的 `assets/`。
仅打包 Git 跟踪的素材，不包含存档、调试文件、构建缓存及本地下载的大型外部模型。
外部飞船、空间站和地球模型按 `README.md` / `CREDITS.md` 的说明另行下载。

可用 PowerShell 的 `Get-FileHash <ZIP路径> -Algorithm SHA256` 核对下载文件与
`SHA256SUMS.txt` 中的哈希。

## 失败与重跑

检查或构建失败时不会创建 Release。附件上传或公开发布失败时，草稿会保留；在
Actions 中重跑失败任务，或使用 **Run workflow** 输入同一标签，流程会继续上传并发布草稿。
已公开的 Release 不会被覆盖；修改代码后请递增版本并推送新标签。

若仓库或组织策略禁止工作流写入 Releases，需要允许该流程使用
`contents: write` 权限。写权限仅授予最终发布任务，构建与检查任务只读取仓库内容。

本地检查打包脚本（在仓库根目录运行）：

```powershell
python -B -m unittest discover -s .github/scripts -p 'test_*.py' -v
```
