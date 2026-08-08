# Phi Launcher

一款 **Phigros 模拟器启动器**，用 Rust + [egui/eframe](https://github.com/emilk/egui) 实现。
界面沿用 Phigros 的天空蓝 / 青色主题（图标、背景、字体、音效均取自 `assets/`），
可以按分类浏览各 Phi 模拟器仓库的 GitHub Releases，一键**下载**并**启动**。

## 功能

- **16:9 窗口**（1280×720，可缩放），使用 `assets/icon.png` 作为窗口图标。
- **Phigros 视觉风格**：`assets/background.jpg` 作为背景，`assets/phigros.ttf` 作为字体，青色主题配色。
- **音效**：按钮点击 / 主操作 / 启动完成分别播放 `button.ogg` / `button_large.ogg` / `ending.ogg`（可在右上角开关）。
- **版本分类**（左侧栏）：
  | 分类 | 仓库 |
  | --- | --- |
  | **Phira** | [TeamFlos/phira](https://github.com/TeamFlos/phira/releases) |
  | **Phira-Firefly** | [tiancra/Phira-Firefly](https://github.com/tiancra/Phira-Firefly/releases) |
  | **phire** | [2278535805/phire](https://github.com/2278535805/phire/releases) |
  | **PhirLie** | [LuteRenxaer/PhirLte](https://github.com/LuteRenxaer/PhirLte/releases) |
- **在线获取版本列表**：调用 GitHub Releases API 拉取每个分类的所有发行版（含预览版开关）。
- **下载 + 进度条**：多线程后台下载资源，实时显示进度；`.zip` 自动解压。
- **启动 / 删除**：下载完成后自动定位可执行文件（`.exe`）并可一键启动；也支持删除已安装版本。
- **打开发布页 / 刷新**：右上角可直接打开该仓库的 GitHub Releases 页面或刷新列表。

## 运行

```bash
cargo run           # 开发模式（带控制台）
cargo run --release # 发布模式（无控制台窗口）
```

> 首次运行需要联网访问 GitHub API。若所在网络访问 GitHub 受限，请自行配置代理；
> 触发 API 频率限制时界面会给出提示，稍后重试即可。

## 目录结构

```
Phi launcher/
├─ Cargo.toml
├─ assets/                # 贴图、字体、音效、图标（已随项目提供）
├─ versions/              # 下载安装的各版本（运行时自动生成，位于 assets 同级目录）
│  └─ <repo>/<tag>/...
└─ src/
   ├─ main.rs             # 入口：16:9 窗口 + 图标
   ├─ app.rs              # egui 界面与状态管理
   ├─ github.rs           # GitHub Releases API 客户端 + 4 个分类定义
   ├─ download.rs         # 下载 / 解压 / 定位可执行文件 / 启动
   ├─ audio.rs            # 音效播放（rodio）
   ├─ assets.rs           # 资源定位与加载（图标 / 背景 / 字体）
   └─ theme.rs            # 青色 Phigros 主题
```

## 说明

- 下载的版本安装在启动器同级的 `versions/<仓库>/<版本号>/` 目录下。
- Windows 平台会优先启动名字包含 `phi` 的 `.exe`，并自动跳过卸载 / 安装程序。
- 若某个资源不是 `.exe`（例如 `.apk`），启动时会为你打开安装目录。
