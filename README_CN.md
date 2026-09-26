# Kēne

**简体中文** · [English](README.md)

<p align="center">
  <img src="assets/branding/keine-portrait.png" width="220" alt="Kēne 角色立绘">
</p>

Kēne 是基于 Rust/Bevy 的桌面视觉小说引擎，配有独立的原生编辑器。
Eiyashou（`.shou`）是原生创作格式。WebGAL 脚本和 LetsGal Studio 工程通过兼容
加载器支持；WebGAL 兼容行为会维护，但当前不继续追求完整功能对齐。

## 打开编辑器

安装 Rust 1.97.1 后，在仓库根目录运行：

```bash
cargo editor projects/test-project
```

`cargo editor` 打开空工作台；后面可接一个或多个工程路径，分别打开工程窗口。
编辑器可浏览并修改原生 Eiyashou 工程：`config.yaml`、配置指定的清单以及
`scripts/**/*.shou` 可写；兼容工程和其他文档在编辑器中只读。

Preview 需要预先构建且协议匹配的 Engine。源码检出后，首次在 Preview 中点击启动前
先构建一次：

```bash
cargo build -p keine --bin keine
cargo editor projects/test-project
```

这两个开发版可执行文件会位于 `target/debug/`。若 Engine 不在编辑器旁，可设置
`KEINE_ENGINE=/path/to/keine`。Editor 与 Engine 的 authoring 协议必须匹配，
不匹配时握手会失败。独立 macOS 应用的打包步骤见
[T24](docs/tasks/T24-project-release-hardening.md)。

**`cargo dev editor` 不是编辑器命令。** `cargo dev <工程>` 启动游戏工程；
创作界面请用 `cargo editor <工程>`。

## 运行与发行游戏

```bash
cargo validate projects/test-project
cargo dev projects/test-project
cargo bundle projects/test-project
```

`cargo dev` 启用热重载及原生、FFmpeg 视频功能，因此需要相应的开发库。
没有 FFmpeg 开发库时，可不启用这些视频功能运行：

```bash
cargo run --features hot-reload -- dev projects/test-project
```

示例工程的[验收清单](projects/test-project/ACCEPTANCE.md)用于手动检查编辑器和运行时。
`cargo bundle` 默认在 `target/bundle/` 生成完整的桌面发行目录：

```text
target/bundle/
├── keine[.exe]
├── game.haku
└── data/
    └── <content-id>.taku
```

分发时需保留整个目录。不传工程参数启动发行版时，可执行文件会查找同级的
`game.haku`，无需从发行目录执行。正式发行要求图片使用 WebP、独立音频使用
Ogg Opus；开发时可读取的其他格式不会自动进入发行构建。首次需要发布身份时会生成
`.keine/publisher.hakutaku-key`；请备份，且绝不要随游戏分发或提交到仓库。
正式发行流程见 [Project CI](docs/project-ci-release.md)。

## 命令

下列 `cargo` 命令由 `.cargo/config.toml` 定义，只在本仓库中使用：

| 命令 | 用途 |
|---|---|
| `cargo editor [工程 ...]` | 打开编辑器；Preview 使用独立 Engine |
| `cargo validate <工程>` | 不打开窗口校验工程 |
| `cargo dev <工程> [--sync]` | 热重载运行；`--sync` 跟随已打开的 LetsGal 工程 |
| `cargo bundle <工程> [--output <目录>]` | 构建完整游戏发行版 |
| `cargo pack <工程> [--output <目录>]` | 只构建 Hakutaku 资源包，不包含 Engine |
| `cargo migrate <源工程> <目标工程>` | 将可保真的兼容内容转换到新 Eiyashou 工程 |
| `cargo remap <工程> <旧=新>... [-y]` | 在单独转换资源后更新引用 |
| `cargo perf <工程> [选项]` | 测量运行帧或启动耗时（`--startup`） |

`cargo migrate` 不改写源工程；不能保留原语义时会拒绝转换。
`cargo remap` 只更新引用，不转换媒体文件。
`cargo bundle <工程> --benchmark` 生成独立的性能测试包。
各命令的选项可用 `cargo <命令> --help` 查看。

安装后的 `keine` 只提供构建时编入的命令；正常游戏发行版不包含 publisher 和
hot-reload 命令。编辑器是独立程序，并非 `keine` 子命令。

## 工程格式

| 输入 | 根目录入口 | 编辑器中的状态 |
|---|---|---|
| 原生 Eiyashou | `config.yaml` 和 `.shou` 脚本 | 可编辑 |
| WebGAL 目录 | `config.yaml` | 兼容输入；只读 |
| LetsGal Studio | `project.json` | 兼容输入；只读 |
| Hakutaku 包 | `game.haku` 与 `data/` | 打包后的运行时输入 |

游戏存档、设置和资料位于由稳定工程 ID 决定的平台用户数据目录，而不是只读的发行目录旁。
工程格式由内容和配置识别；不再读取旧的机器级 `engine.conf` 适配器／视频开关。

macOS 可运行 `dev/scripts/bundle-macos.sh <工程> <应用名> <bundle-id>` 生成游戏
`.app`。原生工程必须在第三个参数传反向域名格式的 bundle ID；LetsGal 工程可从
`project.json.id` 推导。这与独立的 Editor/Engine 应用包是两回事。

## 游戏快捷键

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

以上是游戏快捷键，编辑器另有自己的编辑快捷键。

## 开发 Kēne

```bash
cargo fmt --all --check
cargo check --workspace
cargo clippy --workspace --all-targets
cargo test --workspace
cargo validate projects/test-project
```

`crates/core/` 负责不依赖 Bevy 的模型与执行；`crates/loader/` 负责工程和格式适配；
`crates/authoring/` 负责 Editor–Engine 协议；`crates/editor/` 负责工作台。
根目录 `src/` 负责 Bevy 运行时、场景、UI、媒体及存储。项目进度和验收边界见
[docs/PROJECT_STATE.md](docs/PROJECT_STATE.md)。

延伸阅读：[工程结构](docs/PROJECT.md)、[资源限制](docs/resource-limits.md)、
[内容加载](docs/architecture/07-content-loader.md)、
[存档](docs/architecture/04-rollback-and-save.md)、
[Hakutaku 打包](docs/architecture/06-hakutaku-packaging.md)、
[WebGAL 兼容性](docs/webgal/README.md)。
