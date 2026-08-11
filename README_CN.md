# Kēne

**简体中文** · [English](README.md)

<p align="center">
  <img src="assets/branding/keine-portrait.png" width="220" alt="Kēne 角色立绘">
</p>

Kēne（/keːne/）是一个使用 Rust 编写的原生视觉小说引擎。它基于 Bevy 与 wgpu
渲染，支持直接读取工程目录进行开发，也能生成可独立分发的桌面游戏。

## 功能

- 原生渲染、音频、视频、UI、存档、回看、自动、跳过与回滚。
- 背景、立绘、图层、转场、滤镜、粒子与区域模糊。
- 与帧率无关的对话、动画和时间轴播放。
- 通过独立适配器支持 WebGAL 脚本与 LetsGal 工程。
- 使用 Hakutaku 生成加密、可增量更新的发行包。

## 快速开始

Kēne 需要 Rust 1.97.1。

```bash
git clone https://github.com/maincoretech/keine.git
cd keine
cargo validate projects/test-project
cargo dev projects/test-project
```

没有安装 FFmpeg 开发库时，可使用 `cargo run -- dev projects/test-project`。
视觉验收步骤见
[`projects/test-project/ACCEPTANCE.md`](projects/test-project/ACCEPTANCE.md)。

## 常用命令

| 命令 | 用途 |
|---|---|
| `cargo validate <工程>` | 不打开窗口校验工程 |
| `cargo dev <工程>` | 使用开发工具和热重载运行工程 |
| `cargo dev <工程> --sync` | 跟随已打开的 LetsGal 工程 |
| `cargo bundle <工程>` | 构建可分发游戏 |
| `cargo adapters` | 配置内置适配器 |
| `cargo perf <工程>` | 采集运行时性能样本 |

## 游戏工程

| 工程类型 | 根目录入口 |
|---|---|
| 原生 / WebGAL 目录 | `config.yaml` |
| LetsGal 工程 | `project.json` |
| 已打包游戏 | `game.haku` 与同级 `data/` |

开发时，Kēne 直接读取可编辑的工程目录。通常按以下流程工作：

1. 编辑原生/WebGAL 目录或 LetsGal 工程。
2. 修改脚本或配置后运行 `cargo validate <工程>`。
3. 使用 `cargo dev <工程>` 迭代；跟随已打开的 LetsGal 工程时加 `--sync`。
4. 使用 `cargo bundle <工程>` 构建发行版。
5. 运行生成的发行版验收，再分发完整输出目录。

默认发行目录是 `target/release-package/`：

```text
target/release-package/
├── keine[.exe]
├── game.haku
├── data/
│   └── <content-id>.taku
└── run.sh | run.bat
```

`cargo bundle` 会自动调用 Hakutaku，游戏开发者不需要单独运行打包器。第一次打包会
创建 `.keine/publisher.hakutaku-key`；请备份它，并且不要随游戏分发。保留上一版
输出时，后续打包会复用未变化的内容 segment。

生成 macOS 应用包：

```bash
dev/scripts/bundle-macos.sh path/to/project
```

## 快捷键

| 快捷键 | 操作 |
|---|---|
| `Ctrl+A` / `Ctrl+K` | 自动 / 跳过 |
| `Ctrl+B` / `Ctrl+R` | 回看 / 重播语音 |
| `Ctrl+H` | 隐藏或恢复文本框 |
| `Ctrl+Q` / `Ctrl+L` | 快速存档 / 快速读档 |
| `Ctrl+S` / `Ctrl+O` | 存档 / 读档 |
| `Ctrl+,` / `Ctrl+T` | 设置 / 标题页 |
| 按住 `Ctrl` | 快进 |
| `Esc` | 关闭或返回 |

## 仓库结构

| 路径 | 内容 |
|---|---|
| `src/` | 运行时、渲染、UI、媒体与存储 |
| `crates/core/` | 类型化动作模型与执行状态 |
| `crates/loader/` | 工程、脚本、资源与存档适配器 |
| `projects/test-project/` | 端到端验收工程 |
| `dev/` | 架构文档与平台脚本 |

运行时依赖 `keine-loader` 与 `keine-core`；core crate 不依赖 Bevy。

## 构建与测试

```bash
cargo build --release
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo validate projects/test-project
```

视频构建需要对应平台的原生媒体库。发行音频推荐使用 Ogg Opus。

## 文档

- [工程结构](dev/docs/PROJECT.md)
- [内容加载](dev/docs/architecture/07-content-loader.md)
- [渲染](dev/docs/architecture/03-render-pipeline.md)
- [存档与回滚](dev/docs/architecture/04-rollback-and-save.md)
- [Hakutaku 打包](dev/docs/architecture/06-hakutaku-packaging.md)
- [LetsGal 集成](dev/docs/architecture/08-letsgal-studio.md)
- [WebGAL 兼容性](dev/docs/webgal-compatibility/README.md)
- [当前工作](dev/docs/TODO.md)
