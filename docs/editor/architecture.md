# Keine Editor 最终架构与分阶段实施交接

- 文档日期：20260920
- 文档版本：2.1 Final
- 适用仓库：`maincoretech/keine`
- 交付对象：Codex / 后续开发者
- 状态：**产品与架构约束。不是已完成代码的说明。**
- 本文完整替代此前所有 `KEINE_EDITOR_CODEX_HANDOFF_20260919.md` / `KEINE_EDITOR_CODEX_HANDOFF_20260920.md` 版本。

> **一句话目标：**Keine Engine 与 Keine Editor 保持在**同一个 Git/Cargo monorepo** 中，但编译为**两个独立原生程序**。Editor 使用 GPUI，按 VS Code/Zed 风格管理一个项目窗口内的可停靠 View；不同项目可以多开窗口，同一物理项目只能由一个窗口持有。Editor 与 Engine 在本地通过版本化 IPC 协作完成真实 VN 预览。普通作者/协作者使用预编译的 Editor 与 Engine 编辑、预览或交付**工程包**时，**不需要本地 Rust、Cargo、源码树或编译依赖**。构建**正式发行包**必须在构建环境中安装固定 Rust toolchain 与平台依赖，从固定 Keine revision 重新编译匹配的 Engine；该环境通常是项目 CI。

---

## 0. Codex 执行规则

### 0.1 默认只执行指定 Phase

- 用户明确指定 Phase：只执行该 Phase 以及确实缺失的前置门槛。
- 用户没有指定 Phase：**只执行 Phase 0**。
- 每个 Phase 达到验收门槛后必须停止并交付；不要自动进入下一 Phase。
- 不允许为了“顺便完善架构”提前铺设后续页面、插件体系、云功能、Git UI 或新项目格式。

### 0.2 开工前必须读取

```text
AGENTS.md（如存在）
Cargo.toml
Cargo.lock
.cargo/config.toml
docs/PROJECT_STATE.md
docs/project-and-assets-spec.md
与当前 Phase 直接相关的架构文档/任务文档
git status --short
```

本交接稿中的仓库事实是设计基线，**本地 HEAD 和未提交工作永远优先**。不得覆盖用户尚未提交的修改。

### 0.3 架构原则优先于方便实现

若 GPUI、Dock 或现有 runtime 接口暂时不方便，不得静默改变以下产品边界；先输出最小复现、阻断点和最小改动建议。

---

# 1. 已确定的硬约束

| ID | 决策 | 必须落实的结果 |
|---|---|---|
| D01 | 单仓库 | Engine、Editor、共享 crate、CI 均留在 `maincoretech/keine`。不拆 Editor repo。 |
| D02 | Editor 放在 `crates/` | 新增 `crates/editor/`，package/binary 名为 `keine-editor`。 |
| D03 | 默认 build 仍是 Engine | 现有 `default-members = ["."]` 保持；根目录 `cargo build` 不构建 Editor。 |
| D04 | Editor 显式开发构建 | Keine 开发者使用 Cargo package 选择构建 Editor；这不是最终用户接口。 |
| D05 | 不修改现有业务命令 | `dev`、`bundle`、`validate`、`assets`、`perf` 等保持现有语义，不增加 `--editor`。 |
| D06 | Engine 与 Editor 是独立程序 | 两个进程、两个可执行产物、独立安装/更新；Editor 不把 Engine 打进自身安装包。 |
| D07 | 本地通过 IPC 协作 | 真实预览由独立 Keine Engine 进程执行；Editor 不在 GPUI 内复制一套 VN runtime。 |
| D08 | 工程创作与工程包不需要 Rust | 正常编辑、预览和工程包交付不得调用 `cargo` / `rustc` / `rustup`，不得要求 Keine 源码树或 FFmpeg 开发包；此约束不适用于正式发行包的构建环境。 |
| D09 | 一个项目一个窗口 | 不同项目可以多开窗口；同一物理项目只能有一个可写 Workspace/Window。重复打开聚焦已有窗口。 |
| D10 | 无项目选择窗体 | 不新增 Welcome/Project Picker/首次 workspace 向导。无项目时显示空 Workbench。 |
| D11 | VS Code/Zed 式 Dock | 主要 View/文档标签可排序、跨组移动、横纵分屏、合并、resize、zoom；同一项目内自由拼贴。 |
| D12 | 布局只存 app data | Dock、窗口位置、标签、视图状态不写入游戏项目，不污染 Git diff。 |
| D13 | Preview 默认关闭 | 默认布局不显示 Preview，也不启动 Engine。手动开启；可由本机设置显式允许项目打开后自动启动。 |
| D14 | Preview 属于项目会话 | 移动/重排 Preview 不重启 Engine；关闭 Preview 才停止该项目预览。 |
| D15 | 不做 Editor 插件系统 | 不新增 EditorRegistry、Extension SDK、插件生命周期、工坊。普通模块即可。 |
| D16 | 现有 adapter 不当 Editor 插件 | loader adapter 继续负责运行时内容/工程/存储适配；作者文档写回属于 Editor 文档层。 |
| D17 | 正式发行构建必须有 toolchain | 项目可放 private repo；构建环境必须安装固定 Rust toolchain 与平台依赖，checkout 固定 Keine revision，从源码编译匹配的 Engine 并执行现有正式 `cargo bundle` 链。通常由 CI 承担。 |
| D18 | 不重写当前 publisher | 当前 per-project engine build / Hakutaku key embedding 可继续存在于 CI；不为无需本地 Rust 的工程创作体验提前重构成 runtime pack。 |
| D19 | Engine 不由 Editor 管理 | Editor 可以发现/选择 Engine executable 并检查兼容性，但不下载、安装、更新、切 commit 或编译 Engine。 |
| D20 | 不扩大引擎路线图 | 不因 Editor 顺带追齐 WebGAL/LetsGal 全功能、Live2D/Spine、移动端、协作或社区。 |

---

# 2. 最终源码与产物结构

## 2.1 Monorepo

```text
keine/
├─ Cargo.toml                         # 根 package = keine；workspace root
├─ Cargo.lock
├─ .cargo/config.toml                 # 现有 alias，保持原义
│
├─ src/                               # Keine Engine / runtime
│  ├─ main.rs                         # binary: keine
│  ├─ lib.rs
│  ├─ runtime/
│  ├─ render/
│  ├─ scene/
│  ├─ ui/                             # 玩家游戏 UI，不是 Editor UI
│  └─ storage/
│
├─ crates/
│  ├─ core/                           # keine-core
│  ├─ loader/                         # keine-loader；adapter 在这里
│  ├─ media/                          # keine-media
│  └─ editor/                         # 新增 workspace package
│     ├─ Cargo.toml                   # package = keine-editor
│     ├─ src/
│     │  ├─ main.rs
│     │  ├─ app.rs                    # Application、多项目 Window、打开请求路由
│     │  ├─ actions.rs                # GPUI Actions / key context
│     │  ├─ workspace/
│     │  │  ├─ session.rs             # 一个项目窗口的 WorkspaceSession
│     │  │  ├─ ownership.rs           # ProjectKey / 同项目唯一占用
│     │  │  ├─ layout.rs              # Dock/Layout
│     │  │  ├─ persistence.rs         # app-data 布局/视图状态
│     │  │  └─ views/                 # 内置 View
│     │  ├─ document/
│     │  │  ├─ manager.rs
│     │  │  ├─ history.rs
│     │  │  ├─ save.rs
│     │  │  └─ native/                # 第一条可写作者格式，Phase 2 再落地
│     │  ├─ preview/
│     │  │  ├─ client.rs              # IPC client
│     │  │  ├─ session.rs
│     │  │  ├─ frame.rs
│     │  │  └─ input.rs
│     │  ├─ components/
│     │  └─ services/
│     │     ├─ engine_locator.rs
│     │     ├─ tasks.rs
│     │     ├─ settings.rs
│     │     └─ search.rs
│     └─ tests/
│
├─ projects/                          # fixture / acceptance
├─ docs/
└─ dev/
```

这些是**职责边界**，不是要求 Phase 1 就创建全部空文件。无真实第二个使用点时不要为了目录漂亮抽象接口。

## 2.2 Cargo 边界

现有 workspace 继续：

```toml
[workspace]
resolver = "2"
members = ["crates/*"]
default-members = ["."]
```

因此：

```bash
# Keine 开发者：默认仍只构建 Engine 根 package
cargo build
cargo build --release

# Keine 开发者：显式构建 Editor
cargo build -p keine-editor
cargo run -p keine-editor

# 明确检查整个 monorepo
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

`-p/--package` 是**源码开发者内部的 Cargo 选择方式**。普通 VN 作者永远不应接触这些命令。

### 禁止事项

- 不为了隐藏 `-p` 把 Editor 移出 `crates/`。
- 不给 `cargo dev` / `cargo bundle` 加 `--editor`。
- 不让根 Engine package 反向依赖 `keine-editor`。
- 不让 `keine-editor` 链接整个根 `keine` runtime 以绕过 IPC；完整游戏执行仍由独立 Engine 进程负责。

---

# 3. 开发者世界与作者世界必须分离

## 3.1 Keine 开发者

需要：

```text
Rust / Cargo
平台 SDK
必要的 native media dev dependencies
Keine 源码
```

负责：

```text
开发 Engine
开发 Editor
开发 IPC
CI / release
正式 game bundle 工具链
```

## 3.2 游戏作者 / 协作者

只需要编译好的：

```text
keine-editor
keine                  # 需要真实预览时
游戏项目目录
```

不需要：

```text
Rust
Cargo
rustup
C/C++ compiler
FFmpeg development headers
Keine source tree
```

### 产品原则

> **引擎的编译复杂度由 Keine 开发者和 CI 支付，不由每一个游戏作者重复支付。**

Editor 在正常产品模式下不得“检测不到 Engine 就自动安装 Rust 再 cargo build”。

---

# 4. Engine 与 Editor 的关系

## 4.1 独立程序，不是独立仓库

```text
同一个 keine commit/tag
          │
   ┌──────┴──────┐
   ▼             ▼
keine         keine-editor
Engine         GPUI frontend
   ▲             │
   └──── IPC ────┘
```

二者：

- 可以分别下载和安装；
- 可以分别启动/退出；
- 不共享 GUI event loop；
- 不把 Engine 作为 Editor 安装包内部隐藏 runtime；
- 源码仍共同演进、共同测试。

## 4.2 Engine discovery

Editor 可以保存一个**本机设置**：

```text
engine.executable = /absolute/path/to/keine[.exe]
```

建议发现顺序：

1. 用户显式选择的 Engine；
2. 上次成功使用的路径；
3. 平台标准安装位置 / PATH 中可识别的 `keine`；
4. 找不到时仅在用户开始 Preview 时提示选择 Engine。

### 不做

- Engine Store；
- 自动下载；
- 自动更新；
- 自动切换 Git commit；
- 自动编译源码；
- 把 Engine path 写进游戏项目。

## 4.3 兼容性靠 handshake，不靠猜版本

Editor 连接 Engine 后必须先完成 handshake。最小信息：

```text
protocol_version
engine_version
engine_build_id / commit（可用时）
project_format_versions
capabilities[]
```

判断顺序：

```text
IPC protocol 是否兼容
→ 项目格式是否可接受
→ 需要的 runtime capability 是否存在
→ 才进入预览
```

不要用 `engine_version >= x` 直接推断所有能力。

---

# 5. Workspace / Window 模型

## 5.1 一个项目一个 Window

```text
Keine Editor Application
├─ ProjectWindow(A)
│  └─ WorkspaceSession(A)
├─ ProjectWindow(B)
│  └─ WorkspaceSession(B)
└─ EmptyWindow（可存在，用于无项目启动）
```

- A 与 B 可以同时存在。
- A 不允许被第二个 Window 以可写方式再次打开。
- 同一项目再次打开时聚焦 A。
- 关闭 A 不影响 B。
- 每个 ProjectWindow 最多一个 PreviewSession。

## 5.2 建议：一个 Editor app process，多项目 Window

优先实现：

```text
一个 keine-editor 进程
→ 多个 GPUI Window
→ 每个 Window 一个项目 WorkspaceSession
```

如果用户再次启动 `keine-editor <path>`：

```text
secondary launch
→ 本机 app-open IPC
→ 主 Editor 进程收到 OpenProject(path)
→ 已打开则 focus；否则新建 ProjectWindow
→ secondary process 退出
```

不需要长期后台 daemon，也不建设通用多进程平台。

## 5.3 ProjectKey：同项目去重必须可靠

ProjectKey 不是用户输入字符串。至少：

1. 转绝对路径；
2. 能 canonicalize 时解析 symlink；
3. 按平台处理大小写/路径语义；
4. 能使用文件系统稳定 identity 时可作为附加证据；
5. canonicalize 失败时用规范化绝对路径 fallback，并明确风险。

同名但物理不同的项目副本可以分别打开。

## 5.4 不做 Project Picker

无参数启动：

```text
App Window
└─ Empty Workbench
   ├─ 菜单/命令可以 Open Folder / Open Recent
   └─ 不弹额外 Welcome / Project Picker Window
```

`Open Folder`、系统文件对话框属于命令结果，不属于新增“项目选择窗体”。

---

# 6. View / Dock 最终模型

## 6.1 固定外壳 + DockHost

```text
ProjectWindow
├─ AppChrome                     # 固定，不可 Dock
│  ├─ menu / navigation
│  ├─ search / commands
│  └─ project title
├─ DockHost                      # 主要工作区
│  ├─ Split(horizontal/vertical)
│  └─ TabGroup
│     └─ DockItem
├─ StatusBar                     # 固定
└─ OverlayHost                   # menu/popover/modal/toast/drag feedback
```

## 6.2 DockItem 类型

当前设计目标包括，但不要求一次实现：

```text
Explorer
ScriptDocumentView(document_id)
Inspector
Problems
Output
Tasks
Preview
AssetBrowser
CharacterView
SceneView
MediaView
PerformanceTimeline
Variables
CallStack
Settings
History
```

### P1 只要求

```text
Explorer
至少两个真实文档标签
Inspector shell
Problems/Output/Tasks 中至少一个工具组
```

Preview **不属于 P1 默认布局**。

## 6.3 Dock 交互合同

| 操作 | 必须行为 |
|---|---|
| 同组拖标签 | reorder，不重建 Document |
| 拖到另一 TabGroup | move，不复制 Document |
| 拖到目标边缘 | 创建 Split |
| 拖 splitter | resize 并持久化 |
| 非法/取消 drop | 原布局保持 |
| 拖出项目 Window | 不创建新的 View OS window；取消/回原位 |
| 拖到另一项目 Window | 禁止跨项目迁移 |
| Close Document View | 关闭 View，不删除文件 |
| Close Tool View | 隐藏工具，可重新打开 |
| Reset Layout | 不丢草稿、不修改游戏文件、不启动 Preview |
| Zoom View | 临时占满/恢复，不改变 Document |

## 6.4 状态分层

```text
DocumentId       内容、revision、undo、dirty
ViewInstanceId   cursor、scroll、selection、zoom
DockItemKey      layout persistence identity
WorkspaceId      app-data workspace state
ProjectKey       物理项目唯一身份
WindowId         本次运行期窗口
```

GPUI EntityId/内存地址不得成为跨启动 persistence key。

---

# 7. App-data 持久化

建议：

```text
<keine-editor app-data>/
├─ settings.json
│  ├─ theme / keymap
│  ├─ engine.executable
│  └─ preview.auto_start_on_project_open = false
├─ recent-projects.json
├─ app-instance/                  # secondary launch 路由/互斥所需最小状态
└─ projects/
   └─ <workspace-id>/
      ├─ identity.json            # ProjectKey / last known path
      ├─ layout.json
      ├─ view-state.json
      └─ recovery/                # 未保存草稿恢复；不是 layout
```

### 硬约束

布局/窗口状态不得写入：

```text
config.yaml
project.json
.studio/
.keine/ 项目内部缓存
项目自创 layout 文件
Git tracked source
```

### 保存规则

- 拖动完成后 debounce 写，不在 pointer move 每帧写磁盘。
- 布局 snapshot 带 schema version。
- 临时文件 + 安全替换；损坏时回退默认布局。
- layout save failure 不允许偷偷写进项目目录。
- `Save` / `Save All` 只保存作者文档，不负责 layout。

---

# 8. Editor 文档层与 adapter 边界

## 8.1 运行时 Program 不是作者文档

禁止：

```text
source project
→ compile Action/Program
→ 在 Editor 修改 Action
→ 反序列化回源项目
```

原因：编译后信息可能丢失、一个作者对象可能变成多个 Action，且无法可靠保留未知字段/原始结构。

## 8.2 loader adapter 保持当前职责

`ProjectAdapter` / `StructuredSceneLoader` 继续：

```text
detect/open/compile runtime content
watch/runtime reload
runtime debug cursor / initial state
```

它们**不升级为 GPUI plugin API**，也不直接管理 Editor Dock/View。

## 8.3 作者写回属于 `keine-editor::document`

Editor 文档层负责：

```text
source-preserving document model
revision
undo/redo
safe save
external-change detection
recovery draft
selection/source mapping
```

### Phase 2 第一可写目标

默认优先 **Kēne Native Project**：

```text
config.yaml
scripts/*.txt (script: webgal)
assets/
```

理由：这是当前规范定义的主要原生开发形态；不需要先发明新项目格式。

实现要求：

- 开始可以先提供安全文本编辑；
- 结构化 Card View 必须建立在可保留未知/原始信息的源文档模型上；
- 未能安全 round-trip 的命令显示只读/诊断，不得静默重写；
- 不修改内容时不得 rewrite 文件。

### LetsGal

当前 LetsGal adapter 继续作为**运行兼容输入**。没有专门实现 writer 前：

- 不宣称 LetsGal 项目可完整结构化编辑；
- 可提供只读/诊断/迁移入口；
- 不因为“JSON 好写”就直接做无保护的覆盖保存。

---

# 9. Editor ↔ Engine IPC

## 9.1 IPC 是正式 authoring 边界

Editor 与 Engine 虽同仓，但产品上独立安装，因此 IPC 要版本化、可诊断，但**不是第三方插件 ABI**。

建议逻辑协议：

```text
Handshake
├─ Hello
├─ protocol_version
├─ engine_version/build_id
└─ capabilities

Project
├─ OpenProject
├─ CloseProject
├─ Validate
└─ ProjectDiagnostics

Document / Preview
├─ ApplySnapshot(revision)
├─ ApplyPatch(revision)
├─ SetExecutionCursor
├─ SetPreviewContext
└─ SourceLocationChanged

Runtime
├─ Start
├─ Pause
├─ Resume
├─ Stop
└─ Input

Diagnostics
├─ warning/error
├─ runtime variables
├─ call stack
└─ log

Lifecycle
├─ Ping/Pong
├─ GracefulShutdown
└─ Disconnect/Crash
```

不要在 Phase 3 一次把所有消息实现完。先实现 handshake + open/close + basic diagnostics + lifecycle。

## 9.2 控制通道与帧通道分开

```text
Editor ── control IPC ─────────────▶ Engine
Editor ◀─ diagnostics/state ──────── Engine
Editor ◀═ frame transport ═════════ Engine
```

控制消息不传完整视频帧。

### 可选实现候选

控制：

```text
Unix domain socket / Windows named pipe / loopback local socket
```

帧：

```text
第一版：有界 shared memory / 异步 raw frame buffer
后续：平台 GPU sharing（有真实收益再做）
```

准确方案由 Phase 0 spike 决定。

## 9.3 Engine authoring host

Engine 需要一个**内部 authoring host 模式**，用于 Editor 启动/连接：

- 不属于 `cargo dev` 用户命令；
- 不属于 `cargo bundle`；
- 不需要在普通 help 中作为作者命令宣传；
- 可以是隐藏内部 subcommand/启动参数或等价的内部入口；
- exact syntax 在 Phase 3 落地，Phase 0 只确定可行边界。

Standalone Keine Engine 的发布构建应包含此 authoring host 能力；正式游戏 bundle 仍是现有 hardened/runtime 构建，不要求暴露 authoring IPC。

## 9.4 Session identity

每次预览至少带：

```text
ProjectKey
session_generation
request_id
document_revision
```

旧 session / 旧 revision 返回的 frame、diagnostic、cursor 更新必须丢弃。

---

# 10. Preview 生命周期

## 10.1 两种状态分开

```text
PreviewPresentation
= Hidden | Visible | Obscured

PreviewSession
= Off | Starting | Ready | Playing | Paused | Stopping | Failed
```

View 存在不等于 Engine 在运行。

## 10.2 默认行为

| 场景 | View | Engine |
|---|---|---|
| 新项目窗口 | 不显示 Preview | Off |
| 恢复 layout，auto-start=false | 不自动显示 | Off |
| Show Preview | 显示 stopped UI | Off |
| Start Preview | 打开/显示 | 启动/连接 Engine |
| auto-start=true | 项目 Window 正常恢复后显示 | 启动一次 |
| 切到其他标签 | 仍存在 | Session 保留，可因不可见自动暂停呈现 |
| 移动/分屏 Preview | 移动 | 不重启 |
| Stop Preview | 可保留 View | 释放 session |
| Close Preview | 移除 View | Stop + 释放 |
| Close Project Window | 销毁 | Stop 该项目 session |

`preview.auto_start_on_project_open`：

- app-data 本机设置；
- 默认 false；
- 项目文件不能替用户开启。

## 10.3 每项目独立

```text
Window A → PreviewSession A → Engine process/session A
Window B → PreviewSession B → Engine process/session B
```

第一版建议一个 ProjectWindow 对应一个 Engine authoring process，隔离最清楚。

A 崩溃/Stop 不影响 B。

## 10.4 输入

两种模式必须明确：

```text
Edit Mode
pointer → Editor selection/manipulation

Play Mode
pointer/key → Engine runtime input
```

拖 Dock、拖 splitter、使用快捷键时不得把事件泄漏到游戏推进。

## 10.5 渲染正确性

Preview 必须来自真实 Keine 合成结果，包括该场景应有的：

```text
background
sprites
text/game UI
transitions
post-processing
video
```

不能只截第一台 camera 后宣称完成。

---

# 11. 正式发行：toolchain 在构建环境，通常是 Project CI

## 11.1 游戏项目与 Keine 源码分开

典型：

```text
GitHub
├─ maincoretech/keine              # Engine + Editor source monorepo
└─ studio/my-vn                    # 游戏 project repo；可 private
   ├─ config.yaml
   ├─ scripts/
   ├─ assets/
   └─ .github/workflows/release.yml
```

## 11.2 本地 authoring

```text
Writer / Artist / Director
→ keine-editor
↕ IPC
→ installed keine Engine
→ game project directory
```

这里的产物是仍可继续编辑、预览和交接的**工程目录/工程包**，不是交付给玩家的正式发行包。只要安装了预编译 Editor 与 Engine，这条路径不需要本地 Rust toolchain。

## 11.3 正式 shipping

正式发行包包含为当前项目从源码构建的匹配 Engine 和 Hakutaku 资源，因此构建机**必须**具备固定 Rust toolchain 与目标平台依赖。作者电脑可以不安装这些依赖，是因为构建职责移到了 CI，而不是因为发行构建不再需要 toolchain。

```text
push/tag game project
→ CI checkout game repo
→ CI checkout/pin Keine revision
→ install Rust/platform deps in runner
→ cargo bundle <project>
→ artifact / release
```

**当前 `cargo bundle` 内部会从固定 Keine 源码为项目构建匹配的 hardened engine，并处理 Hakutaku publisher key material。这个成本留在 CI 是可接受且当前最小风险的方案。**不要把预编译 Editor/Engine 重新打包成正式发行物，也不要为了 Editor 先重写 publisher。

## 11.4 固定 Keine revision

第一版可以直接由 game repo 的 workflow 固定：

```text
maincoretech/keine @ tag / commit
```

暂时**不新增 `keine.lock` 或 config 字段**。等有第二个真实消费者需要“项目内声明 engine revision”时再设计。

## 11.5 Publisher identity

生产 publisher identity 应存于 CI secret / secure runner：

```text
HAKUTAKU_IDENTITY_BASE64
```

普通写手/美术电脑不需要持有生产 publisher secret。

## 11.6 后续 reusable workflow

当至少一个真实游戏 repo 跑通后，再在 Keine repo 提供 reusable workflow，例如概念上：

```text
uses: maincoretech/keine/.github/workflows/game-build.yml@<pinned-ref>
```

不要在 Phase 0 先写一个猜测性的 300 行万能 workflow。

---

# 12. 使用者生命周期

## 12.1 普通协作者

```text
安装 Keine Editor
（需要预览的人另外安装 Keine Engine）
        ↓
启动 Editor
        ↓
空 Workbench / 恢复上次项目 Window
        ↓
Open Folder / Open Recent / OS open project
        ↓
ProjectKey 去重
├─ 已打开 → focus existing Window
└─ 未打开 → create ProjectWindow
        ↓
恢复该项目 Dock / 文档 / view state
        ↓
写作 / 素材 / Inspector / Undo / Save
        ↓
需要预览？
├─ 否 → 完全不启动 Engine
└─ 是 → locate compatible Engine → IPC handshake → PreviewSession
        ↓
继续编辑；Preview patch/revision 同步
        ↓
关闭 Preview / Window / App
        ↓
保存作者文档 + app-data；结束对应 Engine session
```

## 12.2 正式发布者

```text
作者提交 project repo
→ CI
→ pinned Keine source
→ existing cargo bundle
→ platform artifacts
```

## 12.3 Keine 开发者

```text
git clone maincoretech/keine
→ cargo build / test
→ cargo build -p keine-editor
→ 调试 Engine + Editor + IPC
```

---

# 13. 分 Phase 实施

> **这部分是 Codex 的主执行计划。任何 Phase 都必须在门槛完成后停。**

## Phase 0 — 架构冻结与最小技术 Spike

### 目标

在写大量 UI 前证明关键边界可行，避免后续返工。

### 必须完成

1. 核对本地 workspace / package / aliases / feature 图。
2. 确认新增 `crates/editor/` 后：
   - `cargo build` 仍只选择根 Engine；
   - `cargo build -p keine-editor` 可独立选择 Editor。
3. 锁定 GPUI / gpui-platform / Dock 依赖版本；不得使用 `*` 或漂移 main。
4. 跑通 GPUI：
   - Application；
   - 两个 Window；
   - focus；
   - typed drag/drop；
   - Dock split/tab reorder；
   - basic GPUI test。
5. 验证 app-data 目录选择。
6. 设计并 spike ProjectKey / app single-instance open-request 路由。
7. 验证 Engine authoring IPC 的最小控制通道可行性：
   - launch/connect；
   - Hello/Ping；
   - clean shutdown。
8. 验证 Keine offscreen 完整合成的技术路径，至少定位：
   - window coupling；
   - camera target；
   - frame extraction；
   - audio/video lifecycle。
9. 从候选 frame transport 中只选**一个第一版路径**，记录理由和限制。

### 明确不做

- 完整编辑器页面；
- 作者文档 writer；
- 真实 VN Preview UI；
- CI game release workflow；
- GPU zero-copy 优化；
- 插件系统。

### 交付

```text
P0 技术记录
锁定依赖版本
最小多窗口/Dock demo 或 test
IPC hello spike
offscreen/frame transport spike 结论
需要的最小 Engine 侧接口清单
```

### 验收门槛

- `cargo build` 不编译 Editor/GPUI；
- `cargo build -p keine-editor` 可到最小 GPUI app；
- GPUI 一个进程可持有两个 Window；
- 同一 dummy ProjectKey 的第二次 open 会路由到既有 window；
- Dock 移动/分屏有可复现证据；
- Engine IPC hello/shutdown 有真实进程证据；
- offscreen 渲染不是“理论上可以”，而是有代码路径/实验结果。

**完成后停止。**

---

## Phase 1 — Editor Shell / Project Window / Dock / AppData

### 目标

建立稳定的编辑器骨架，尚不接真实 VN runtime。

### 必须完成

```text
crates/editor package
App / multi-window routing
Empty Workbench
Open Folder / Open Recent
ProjectKey 去重
secondary launch → existing app request routing
ProjectWindow / WorkspaceSession
DockHost
layout persistence
basic actions/key contexts
status bar / overlay host
```

最小 View：

```text
Explorer
两个真实文本 Document tab
Inspector shell
Output/Problems/Tasks 至少一个工具组
```

### 必须保证

- A、B 项目可同时开两个 Window；
- A 第二次打开只 focus A；
- 同一项目不同路径表达尽量正确去重；
- View 可 reorder/split/move；
- layout 按项目写 app-data；
- Reset Layout 不改游戏文件；
- Preview 不存在、不启动 Engine。

### 不做

- 结构化 VN Card editor；
- Engine preview；
- asset manager；
- production build UI。

### 验收门槛

真实打开两个 fixture/project folder，完成窗口、Dock、持久化、重复打开和关闭恢复测试。

**完成后停止。**

---

## Phase 2 — 作者文档与安全写回

### 目标

让 Editor 成为真正可写的 VN 编辑工具，而不靠 runtime Action 反序列化。

### 第一目标

Kēne Native Project：

```text
config.yaml
scripts/*.txt
```

### 必须完成

1. DocumentManager。
2. Native script source-preserving model。
3. revision / dirty / save state。
4. text input + IME。
5. undo/redo scope。
6. external modification detection。
7. safe/atomic save。
8. recovery draft。
9. source diagnostics mapping。
10. Inspector 与 source selection 基础联动。

### 结构化 Card View

只在能够 round-trip 后加入：

```text
SourceDocument authority
├─ TextView
└─ CardView
```

CardView 不持有第二份正文。

### 不做

- 从 `Action/Program` 倒推源项目；
- 完整 LetsGal writer；
- 新的“万能 native JSON script format”；
- Preview。

### 验收门槛

- fixture 打开不修改 → 文件哈希不变；
- 修改再保存再打开 → 语义一致；
- 未知/暂不支持指令不丢；
- 中文/日文 IME 通过实机；
- layout move 不影响 document undo；
- external modification 不被静默覆盖。

**完成后停止。**

---

## Phase 3 — Engine Authoring IPC Host

### 目标

建立稳定的 Editor ↔ Engine 进程边界，但暂不要求完整画面接入。

### Engine 侧

实现内部 authoring host：

```text
handshake
open/close project
validate
basic runtime lifecycle
source diagnostics
ping/shutdown
capabilities
```

### Editor 侧

```text
EngineLocator
EngineProcess/Connection
compatibility UI
PreviewSession state machine（暂可无画面）
crash/disconnect handling
per-project ownership
```

### 协作者要求

使用**预编译 Engine** 完成整个测试；Editor 不得 fallback 到 Cargo build。

### 不做

- 每帧画面；
- zero-copy；
- Engine download/update manager；
- public third-party IPC SDK。

### 验收门槛

- 无 Rust toolchain 的测试环境能用预编译 Editor + Engine 完成 handshake/validate；
- 不兼容 protocol/capability 有明确错误；
- A、B Window 的 Engine session 隔离；
- A Engine crash 不关闭 B Window；
- Stop/Close 不留下 orphan Engine process。

**完成后停止。**

---

## Phase 4 — 真实内嵌 Preview

### 目标

完成“像 LetsGal，但使用真实 Keine runtime”的核心闭环。

### 必须完成

```text
Preview View（默认关闭）
manual Start/Stop
optional local auto-start
真实 offscreen composition
frame transport
resize
letterbox/input mapping
Edit vs Play input mode
source cursor ↔ runtime position
revision/stale-result rejection
visibility pause
media cleanup
```

### 关键规则

- Show Preview ≠ Start Engine；
- restore layout ≠ restart previous running state；
- move Preview ≠ restart Engine；
- Close Preview = Stop session；
- project A/B Preview 独立；
- 不开第二个游戏 OS window。

### 验收场景

真实 Keine fixture 至少覆盖：

```text
dialogue
background
sprite
basic transition/post-process
game UI
代表性 audio
video（平台支持时）
```

### 性能门槛

不先写虚构 FPS 数字。必须记录：

```text
text input latency observation
frame queue depth
resize rebuild count
idle CPU
start/stop resource trend
```

发现瓶颈后再决定 GPU sharing。

**完成后停止。**

---

## Phase 5 — 核心 VN Authoring UX

### 目标

在已有 Document + Preview 基础上形成真正优于纯文本的 VN 工作流。

### 优先级顺序

1. Script Card / Plain Text 双视图。
2. Selection → Inspector → Preview 闭环。
3. 连续对白模式。
4. Insert Palette。
5. 资源选择/引用索引。
6. Asset Browser。
7. Character 基础管理。
8. Scene 基础管理。
9. Problems / runtime diagnostics。
10. Performance Timeline 的最小可用版本。

### 不是本 Phase 自动包含

```text
Visual UI Designer 全量
复杂关键帧编辑器
Localization/Voice 全流程
Narrative Blueprint 全量
Live2D/Spine
```

每增加一个 View 必须复用既有 Document/Selection/Undo/Dock/Task 规则，不能另造 page-local 状态体系。

**完成选定子任务后停止，不以“Phase 5”作为无限功能桶。**

---

## Phase 6 — Project CI 正式发行链

### 目标

让作者在本机没有 Rust 的情况下编辑、预览和交付工程包；正式发行时，由具备固定 Rust toolchain 与平台依赖的 CI 从固定 Keine revision 开始，重新编译匹配的 Engine 并产出发行包。

### 产物与工具链边界

| 产物 | 内容与用途 | toolchain 要求 |
|---|---|---|
| 工程目录/工程包 | 可继续编辑的配置、脚本、资产和工程元数据；供 Editor 打开、Engine 预览或交给协作者 | 作者电脑无需 Rust；使用预编译 Editor 与 Engine |
| 正式发行包 | 面向玩家的平台产物；包含从固定 Keine 源码为该项目构建的 hardened Engine 与 Hakutaku 资源 | 构建环境必须安装固定 Rust toolchain 与平台依赖，并从头构建 |

“作者无需本地 toolchain”只表示把发行构建职责移到 CI，绝不表示正式发行包可以绕过 Engine 源码构建。

### 第一条真实项目先手写验证

```text
game repo
→ checkout pinned Keine ref
→ setup toolchain/platform deps
→ restore publisher identity secret
→ cargo bundle <project>
→ collect artifact
```

第一条真实流水线必须从 clean runner 开始显式安装/选择固定 toolchain；不得依赖 runner 上碰巧存在的 Rust、复用作者电脑的 Engine binary，或把工程包直接改名为发行包。

### 验证后再抽 reusable workflow

必须覆盖：

```text
pinned engine revision
publisher secret handling
artifact naming
failure logs
cache correctness
platform matrix（仅已实际支持的平台）
```

### 不做

- Editor 内置 Git UI；
- Editor 上传源码到 CI；
- 云构建平台；
- 自动把本地未提交文件上传。

Editor 可以以后提供“打开 CI 文档/复制模板”的辅助，但不成为 CI owner。

### 验收门槛

一个真实 private test game repo 在至少一个平台从 clean runner 成功产出可运行 artifact；记录所用 Keine ref 和 secret contract。

**完成后停止。**

---

## Phase 7 — 跨平台与发布硬化

### 目标

把已经成立的架构做成可分发产品，而不是继续扩功能。

### 内容

```text
Editor standalone release packaging
Engine standalone authoring release packaging
Windows/macOS/Linux installation/discovery
DPI/input/IME validation
multi-monitor/window restore
crash recovery
protocol compatibility policy
performance regression baseline
full acceptance project
```

### 重要边界

Editor 和 Engine **分别发布**；不要在此 Phase 又改回“Editor 内嵌 Engine”。
签名与公证暂不纳入当前 P7 验收，待有实际发行要求时单独决定。

---

# 14. Phase 依赖关系

```text
P0
├─→ P1 Workspace/Dock
│    └─→ P2 Document
│          └─→ P5 Authoring UX
│
└─→ P3 IPC Host
     └─→ P4 Preview
          └─→ P5 Authoring UX

P6 CI shipping 可以在 P2/P3 后并行准备，
但不要阻塞 P1–P4 的本地作者体验。

P7 等核心链路稳定后进行。
```

核心产品闭环：

```text
P1 + P2 + P3 + P4
```

达到这里已经具备：

```text
多项目工作台
安全编辑
独立 Engine IPC
真实内嵌预览
无需本地 Rust 的工程创作与交付体验
```

P5 是体验扩展，P6 是正式生产链，P7 是发布硬化。

---

# 15. 核心验收矩阵

| ID | 行为 | Phase |
|---|---|---|
| B01 | 根 `cargo build` 不构建 `keine-editor` | P0/P1 |
| B02 | `cargo build -p keine-editor` 可独立构建 | P0/P1 |
| B03 | Engine 不依赖 Editor | P1 |
| W01 | 无参数启动不弹 Project Picker | P1 |
| W02 | A/B 项目分别开 Window | P1 |
| W03 | A 重复打开只 focus 已有 Window | P1 |
| W04 | symlink/相对路径重复打开尽量正确去重 | P1 |
| W05 | A 关闭不影响 B | P1 |
| L01 | tab reorder / cross-group / split / resize | P1 |
| L02 | layout 保存 app-data，不改项目文件 | P1 |
| L03 | Reset Layout 不丢文档 | P1 |
| D01 | 无修改打开/关闭不 rewrite 源文件 | P2 |
| D02 | unknown source content 不静默丢失 | P2 |
| D03 | IME/Undo/外部修改安全 | P2 |
| I01 | 无 Rust 环境可连接预编译 Engine | P3 |
| I02 | handshake/capability incompatibility 可诊断 | P3 |
| I03 | A/B Engine session 隔离 | P3 |
| I04 | crash/shutdown 无 orphan | P3 |
| P01 | Preview 默认不显示、不启动 | P4 |
| P02 | Show Preview 不自动启动 Engine | P4 |
| P03 | Start Preview 才建立 session | P4 |
| P04 | move/split Preview 不重启 session | P4 |
| P05 | Close Preview 完整释放媒体/runtime | P4 |
| P06 | layout/input gesture 不泄漏到 VN runtime | P4 |
| P07 | frame/diagnostic stale revision 被丢弃 | P4 |
| P08 | 真实合成包括 UI/transition/post-process | P4 |
| P09 | A Preview Stop 不影响 B | P4 |
| C01 | private game repo 的 clean CI 可安装固定 toolchain，并从固定 Keine 源码 `cargo bundle` | P6 |
| C02 | CI 使用固定 Keine ref | P6 |
| C03 | publisher secret 不进入作者项目/日志 | P6 |
| C04 | 工程包无需本地 toolchain；正式发行包构建环境必须有 toolchain 并从头构建 Engine | P6 |
| R01 | Editor/Engine 分别发布、可分别安装 | P7 |
| R02 | 普通作者的编辑、预览和工程包交付文档不要求本地 Rust/Cargo；发行文档明确 CI toolchain 前置条件 | P3–P7 |

---

# 16. 明确暂缓/不做

当前不建设：

```text
Editor 插件/扩展框架
Workshop / community
Cloud collaboration
Story relay
内置 Git UI
Engine download/update manager
Engine Store
Editor 内嵌 Engine 安装包
多引擎版本管理器
项目本地 Rust build
新的通用脚本格式
完整 LetsGal writer
Live2D / Spine 路线图扩张
移动端 Editor
跨项目 Dock 拖动
同项目多 Preview
远程 Preview
GPU zero-copy 作为首版硬要求
```

这些未来有真实需求再单独立项。

---

# 17. Codex 防范围漂移纪律

1. 每次只完成当前 Phase。
2. 一个阻断先给最小复现，再决定最小改动。
3. 不为了“以后可能需要”预建 SDK/registry/provider abstraction。
4. 不把 mock frame 当真实 Preview 完成。
5. 不把“能 parse”写成“能安全 edit/write”。
6. 不把“GPUI 可拖拽”写成“Dock 生命周期已正确”。
7. 不为了 Editor 修改 `cargo dev` / `cargo bundle` 用户语义。
8. 不让 Editor 正常运行路径调用 Cargo。
9. 不把 app-data 写入项目。
10. 不静默升级 Bevy、存档格式、Hakutaku 或媒体栈。
11. 不复制 Zed 大量代码；优先参考模式和公开 API，检查许可。
12. 每个 Phase 交付必须列出实际运行命令、平台、Pass/Fail/Not run。

### 每阶段固定交付格式

```text
Phase：
完成的用户行为：
修改文件及原因：
架构边界是否变化：
实际依赖版本：
实际运行的命令：
Pass：
Fail：
Not run：
已知问题与最小复现：
下一 Phase 前置条件：
```

---

# 18. 已核对的当前仓库事实

以下事实是本文设计的重要前提；实现时仍需以本地 HEAD 复核。

## 18.1 Cargo workspace

当前根 `Cargo.toml`：

```text
workspace members = crates/*
default-members = ["."]
root package = keine
```

因此 `crates/editor` 可以成为 workspace member，同时根 `cargo build` 继续默认只选择 root package。

## 18.2 现有 Cargo aliases

`.cargo/config.toml` 已有：

```text
dev      → run ... -- dev
validate → run -- check
assets   → publisher runner -- assets
bundle   → publisher runner -- bundle
```

这些保持现有语义，不承接 Editor。

## 18.3 现有 project formats

当前规范：

```text
LetsGal project     → project.json
Kēne Native Project → config.yaml
Packaged Project    → game.haku + data/
```

Kēne Native Project 当前以 `scripts/*.txt` + `script: webgal` 为主要脚本形态。

## 18.4 现有 adapter 边界

`ProjectAdapter` 是 read-only format translator；LetsGal editor adapter 当前用于把 Studio 工程编译成 runtime-neutral program，不是 UI 插件，也没有安全写回职责。

## 18.5 当前 `bundle`

现有 publisher：

```text
prepare project
→ validate/compile
→ derive runtime key material
→ cargo build matching hardened engine
→ Hakutaku pack
→ assemble release
```

这意味着正式 shipping 必须使用 Rust toolchain 从固定源码重新构建匹配 Engine；本文只把这个成本放在 CI，而不是消除它或重写进 Editor。工程包仍可由预编译 Editor/Engine 创建、打开和交付。

---

# 19. GPUI / Zed 参考入口

实现前读取锁定版本对应源码，而不是凭聊天记忆写 API。

建议参考：

```text
Zed GPUI README
crates/gpui/examples/hello_world.rs
crates/gpui/examples/input.rs
crates/gpui/examples/drag_drop.rs
crates/gpui/examples/uniform_list.rs
crates/gpui/examples/testing.rs
crates/workspace/src/persistence.rs
```

借鉴重点：

```text
Application / Window / Entity / Render
FocusHandle / key context / Action
IME / clipboard / selection
Typed drag/drop
async tasks
workspace/window persistence layering
GPUI tests
```

不要照搬 Zed 的：

```text
remote workspace
collaboration
database stack
plugin ecosystem
language-server architecture
entire pane/workspace implementation
```

Visual novel 编辑器交互参考用户提供的《LetsGal Studio 2.0 UI 结构与交互审计》，重点只取：

```text
Document/View 分离
Selection → Inspector → Preview
context-aware undo
DragDrop 目标语义
Preview ↔ source mapping
Background Task 状态
```

不复制其中的 Workshop、Cloud、Realtime Collaboration 或独立 Preview Window。

---

# 20. 最终架构图

```text
                         maincoretech/keine
                       （一个 Git/Cargo monorepo）
                                  │
              ┌───────────────────┼────────────────────┐
              │                   │                    │
              ▼                   ▼                    ▼
         Keine Engine        Keine Editor         shared crates
           `keine`          `keine-editor`      core/loader/media
              │                   │
              │   versioned IPC   │
              └───────────────────┘
                       authoring local
                              │
                              ▼
                         game project
                              │
                              │ git push/tag
                              ▼
                         Project CI
                              │
                  pinned Keine tag/commit
                              │
                              ▼
                    existing `cargo bundle`
                              │
                              ▼
                        shipping artifact
```

最终边界：

> **同仓开发，双程序运行，IPC 协作；项目独立，CI 发行；Rust 成本只由 Keine 开发者和 CI 承担。**


---

# 21. 实现核对入口

Codex 开始每个 Phase 时优先读取**当前本地版本**的这些文件；下面只提供定位，不表示远端内容比本地 HEAD 更新。

## Keine

```text
Cargo.toml
.cargo/config.toml
src/runtime/cli.rs
src/runtime/bootstrap.rs
src/publisher.rs
crates/loader/src/adapter.rs
crates/loader/src/adapter/editor.rs
docs/PROJECT_STATE.md
docs/project-and-assets-spec.md
dev/docs/architecture/05-bevy-architecture.md
dev/docs/architecture/08-letsgal-studio.md
```

重点核对：

```text
default-members
现有 dev/bundle alias 与 CLI parser
SingleInstanceGuard / runtime lifecycle
publisher::build_engine
ProjectAdapter read-only contract
StructuredSceneLoader debug/reload contract
native project layout
Bevy multi-camera/render composition
```

## Zed / GPUI

本设计阶段参考过的 Zed 代码基线：

```text
commit 916fc2b8cb3a815cbef4a3b40e13081be72036b6
```

实现时应重新核对最终锁定依赖对应版本：

```text
crates/gpui/README.md
crates/gpui/examples/README.md
crates/gpui/examples/hello_world.rs
crates/gpui/examples/input.rs
crates/gpui/examples/drag_drop.rs
crates/gpui/examples/testing.rs
crates/workspace/src/persistence.rs
```

GPUI 仍处于快速演进期；**不要从这份文档复制假定的函数签名**，所有 API 必须以锁定版本源码/文档和实际编译结果为准。

## 产品交互参考

```text
LetsGal_Studio_2.0_UI_Interaction_Audit_CN(1).md
```

只把其成熟交互模式当参考，不把 LetsGal 的产品范围当 Keine Editor 的需求列表。
