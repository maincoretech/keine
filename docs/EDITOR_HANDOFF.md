# Kēne Editor 开发交接

> 面向下一位负责 editor 的 Codex。Editor Phase 0/1 已完成；当前实现状态以
> `docs/PROJECT_STATE.md`、`docs/editor-phase0.md` 和 `docs/editor-phase1.md` 为准。
> 本文集中记录跨阶段边界，不授权绕过 `AGENTS.md` 或直接修改主分支。

## 1. 开始前先读

按以下顺序阅读，避免被旧文档或目录名误导：

1. `AGENTS.md`：最高优先级的协作、架构、验证和所有权规则。
2. `docs/PROJECT_STATE.md`：当前能力、已知限制和任务队列。
3. `docs/editor-phase0.md` 和 `docs/editor-phase1.md`：已经实现和验证的 editor 合同。
4. 本文：跨阶段上下文和决策边界。
5. 本次唯一分配的 `docs/tasks/TXX-*.md`：实际任务范围和文件所有权。
6. 仅按任务需要查看：
   - `docs/PROJECT.md`
   - `docs/architecture/05-bevy-architecture.md`
   - `docs/architecture/07-content-loader.md`
   - `docs/architecture/08-letsgal-studio.md`

不要在没有任务文件的情况下先搭 editor 框架。任务文件应明确 MVP、仓库位置、
UI 技术、引擎接入方式、可写文件和验收方式。

## 2. 项目是什么

Kēne 是 Rust 2024 编写的原生视觉小说引擎，可运行 Kēne 原生项目、WebGAL
脚本、LetsGal Studio 项目和 Hakutaku v1 包。当前桌面运行时基于 Bevy 0.19，
目标平台是 macOS、Windows x64 和 Linux。

工程分成三个依赖层：

```text
keine-core  <-  keine-loader  <-  keine
纯模型/执行     内容和格式适配      Bevy 运行时、渲染、UI、媒体、存储、发布
```

- `crates/core/`：`Action`、`Program`、`State`、表达式和确定性执行。
- `crates/loader/`：项目识别、格式适配、受限内容挂载、编译结果、诊断。
- `src/runtime/`：启动、宿主边界、输入、生命周期、脚本驱动。
- `src/scene/`：背景、立绘、音频、视频、粒子和场景效果。
- `src/render/`：渲染管线和 WGSL。
- `src/ui/`：游戏运行时的固定 MainCore UI，不是 editor GUI。
- `src/storage/`：存档、设置、历史、画廊和用户数据目录。

依赖方向不可反转；`keine-core` 和 `keine-loader` 必须保持 Bevy-free。

## 3. 当前已经有的、可被 editor 利用的能力

### 3.1 类型化且确定的执行层

运行时只消费 `Program`/`State` 和类型化 `Action`。同一程序和状态应得到可重现的
执行结果。这是预览、定位和诊断的基础；editor 自己的文档 JSON、GUI 状态或控件
模型不得进入 core。

复杂多线蓝图的 UI/语义尚未跟进，但 core 已拥有流程、条件、选择、赋值和场景调用
等基础能力。不要为了画布先发明第二套执行语义。

### 3.2 Loader 与诊断

`keine-loader` 已公开：

- `LoaderRegistry`、`ProjectAdapter` 和 `StructuredSceneLoader`；
- `ContentProject`、`ContentMount`、`ContentFile` 和有序覆盖规则；
- `LoadedScene`、`SourceSpan`、`Diagnostic`、资源引用和子场景引用；
- 项目加载、场景加载、编译包编解码接口；
- LetsGal 项目的选中场景/步骤、初始变量和热重载所需窄接口。

这些能力适合做“打开项目、只读解析、错误定位、预览输入”。它们不等于可写的
editor 文档模型，也没有承诺往来源格式安全回写。

### 3.3 可嵌入运行时

根 crate 当前公开：

- `build_app_with_loader(project_path, loader) -> Result<bevy::prelude::App>`；
- `run`、`run_cli`、`run_with_loader`；
- `HostCapabilityRegistry`、`HostCommandMessage`。

`build_app_with_loader` 可供受控宿主在运行前增加插件或能力，但它仍构建完整 Bevy
应用。不要据此假定已有稳定的 editor SDK、渲染控件或跨进程协议。

### 3.4 已有的 LetsGal Studio 联调路径

`cargo dev <project> --sync` 会只读监听 LetsGal Studio 项目，重新编译并按选中步骤
重建确定性状态。它证明了“外部编辑器改动 -> loader -> 运行时预览”的路径。

这条路径是外部格式兼容功能，不是 Kēne editor 架构：

- 不注入 Studio；
- 不读写 DOM/ASAR；
- 不依赖 HTTP/TCP 控制协议；
- 不反向操控 Studio；
- 不提供 Kēne editor 的文档保存模型。

## 4. 最容易误解的目录

`crates/loader/src/adapter/editor/` 中的 `editor` 指“完整的第三方编辑器项目格式”。
目前这里的 LetsGal adapter 负责读取多份 JSON、解析 ID、生成 adapter-neutral 场景、
诊断、配置和调试光标。

它不是 Kēne editor 的 GUI 层，也不应该承担：

- 窗口、控件、布局或主题；
- editor 文档的撤销/重做和脏状态；
- 文件写回、自动保存或迁移策略；
- 启动/管理预览进程；
- Bevy ECS、渲染或输入；
- 某个 editor 的通用插件系统。

若新 editor 需要文档模型，应由 editor 自己拥有，再经明确的 compile/preview 边界
转换为 loader/core 能理解的类型。不要复用 `ProjectAdapter` 充当双向编辑 API。

## 5. Editor 与引擎的推荐边界

下面是边界原则，不是对具体 UI 框架或仓库位置的决定：

```text
Editor UI
  -> editor-owned document/session state
  -> explicit compile/preview boundary
  -> keine-loader (formats, mounts, diagnostics)
  -> keine-core (Program, State, deterministic semantics)
  -> keine runtime (rendering and interactive preview)
```

每层只向下一层交付稳定、必要的数据：

- editor 负责标签页、选择、撤销、脏状态和保存交互；
- loader 负责来源格式到中立模型的读取、编译与诊断；
- core 负责语义，不了解窗口和磁盘项目布局；
- runtime 负责运行游戏，不解释 editor 私有 JSON。

如果预览必须跨进程，先写清最小消息集、生命周期、崩溃恢复和平台差异，再实现。
不要把 shell 命令、Electron/Node 思路或平台路径扩散到 core/loader。若能通过 Rust API
直接嵌入且满足窗口需求，优先保持 Rust 原生和类型化；但具体方案仍需任务批准。

## 6. 已确认的产品方向

以下是产品偏好，可以作为 editor 任务的约束：

- 默认是一个主窗口，避免大量独立窗口导致观感和管理混乱。
- 不同脚本文件可考虑使用标签页。
- 预览不必始终开启，应允许按需显示；可分离预览是可接受方向。
- UI 应保持清晰、实用，不为小场景引入大型框架或复杂抽象。
- 实现应遵循 Rust 原生思路，不照搬 LetsGal/Electron 的进程、shell 或 JS 架构。

“标签页”和“可分离预览”是方向，不是已经批准的完整信息架构。下一任务仍需给出可验收
的界面范围。

## 7. 下一阶段仍未决定，不能擅自固化

开始产品代码前必须由用户或任务文件明确：

1. 首个可写文档范围，以及格式版本、原子保存和恢复草稿策略。
2. IME、撤销/重做、冲突检测和外部文件变化的具体合同。
3. Preview 的宿主形态、进程生命周期、崩溃恢复和热重载边界。
4. Editor 如何发现、选择和启动匹配版本的 Kēne Engine。
5. 下一阶段验收的平台、真实工程和完成证据。
6. 是否需要扩展机制；没有首个真实用例时继续明确不做。
7. Git、录制、协作、资产管理和构建 UI 是否进入后续独立阶段。

不要用“先搭通用框架”代替这些决策。仓库明确禁止没有当前用例的主题系统、插件框架、
动态后端抽象、新兼容层和新依赖。

## 8. 当前明确不做或受限的内容

- WebGAL 是冻结兼容层；除安全、崩溃、数据丢失和 Kēne 引起的明显回归外，不新增语义。
- Spine/Live2D 当前明确不支持；editor 不应展示为可用生产能力。
- 复杂多线蓝图暂缓；可以保留未来入口，但不要先实现不稳定语义。
- LetsGal Studio 2.0 的安装包、扩展宿主和收费后续不是同步目标。
- 不用 shell 拼接平台能力；平台差异应收束在明确的 runtime/host 边界。
- 不得把只读导入格式悄悄变成可写格式。写回需要单独设计、备份和失败恢复。
- 不得让开发兼容格式悄悄扩大 shipping build。

## 9. 引擎侧必须保持的约束

- 设计空间固定为 1920x1080；viewport/letterbox 转换只有一个所有者。
- scene、normal UI、dialog 三类相机职责和合成顺序不可被 editor preview 绕过。
- 内容通过有序、只读、受根目录限制的 mount 进入；后挂载覆盖先挂载。
- Save v10 只在 Program fingerprint 匹配时恢复；profile、历史、画廊和设置不随槽回滚。
- shipping 持久化使用稳定 `project.id` 对应的平台用户数据目录，不写入只读 bundle。
- publisher 身份和密钥不得记录、提交、缓存或传给无关子进程。
- WebP 和 Ogg Opus 是生产图像/音频规范；Hakutaku v1 是唯一打包项目格式。

如果 editor 的设计要求打破其中任何一条，先停止编码，提交接口变更提案。

## 10. 建议的下一个 editor 任务

Phase 1 已完成 GPUI 单应用多项目窗口、Dock 工作台、只读文档、Inspector/Output、
app-data 持久化和 secondary-launch 转发。下一任务应只选择一个可验收闭环：

1. 可写文本链路：IME、撤销/重做、原子保存、外部修改和恢复草稿；或
2. Preview 链路：Engine 发现/启动、最小控制协议、帧传输和失败恢复。

不要把两条链路、流程图、插件、Git、协作、安装器和通用扩展市场塞进同一任务。只有一个
真实闭环验收后，才扩大范围。

## 11. 文件所有权和改动路线

先在任务文件声明所有权。一般判断如下：

- 新 editor UI/session/document：应位于明确的新 editor 所有区域。
- 新的纯语义：`crates/core/`，必须与 editor UI 无关。
- 来源格式读取/编译/诊断：`crates/loader/`。
- 运行和预览宿主：`src/runtime/`；避免把通用 runtime 变成 editor service locator。
- 游戏 UI：`src/ui/`；不要把 editor UI 混入。
- 资源/画面/音频预览能力：按现有 `src/scene/`、`src/render/` 所有权处理。

`Cargo.toml`、`Cargo.lock`、`.cargo/`、`src/lib.rs`、`src/runtime.rs`、
`src/runtime/bootstrap.rs`、`README*`、`docs/PROJECT_STATE.md`、`docs/tasks/`、
`docs/performance-baseline.md` 和测试项目默认由集成任务拥有。需要改动时，任务文件必须
显式授权；worker 不得顺手修改。

## 12. 开发流程

1. 检查 `git status` 和相关 diff，保留用户已有修改。
2. 阅读 `AGENTS.md`、`docs/PROJECT_STATE.md` 和恰好一个分配的任务文件。
3. 使用 `codex/TXX-short-name` 隔离 worktree/branch。
4. 只修改任务声明的所有权；跨界需求先报告接口变更，不绕路编辑。
5. 不凭 UI 截图或编译成功声称功能完成；实际运行并验证改变的交互。
6. 不为采集证据在产品中添加截图 hook、ready 标志、环境变量协议或临时 GUI 自动化。
7. 未经用户明确要求，不提交、不合并、不推送主分支。

新增依赖、序列化行为、unsafe/FFI、安全或性能结论必须先查官方/一手资料，并把最终
不变量落入代码、测试或提交说明。性能改动必须提供前后测量。

## 13. 验证门槛

所有集成改动必须通过：

```text
cargo fmt --all --check
cargo check --workspace
cargo clippy --workspace --all-targets
cargo test --workspace
```

改动项目、loader、adapter、compiler 或 publisher 时另跑：

```text
cargo validate projects/test-project
```

还要执行任务文件声明的 feature checks 和 benchmarks。Editor UI 的验收至少应覆盖：

- 实际尺寸下打开项目、切换文档和关闭窗口；
- 诊断是否定位到正确文件/位置；
- 预览启动、停止、重载和失败恢复；
- 未保存状态和退出确认；
- macOS 之外的平台代码是否仍可编译，平台逻辑是否被隔离。

视觉验收应直接操作运行中的应用；构建通过不是视觉验收。

## 14. 完成交接时应报告

- 任务和 commit/branch（若用户要求提交）；
- 修改文件及其所有权；
- editor/engine 接口是否变化；
- 执行过的命令及原始结果摘要；
- 真实 UI 验收环境和观察结果；
- 尚存风险、平台缺口和明确未实现项；
- 是否需要集成任务修改共享文件。

## 15. 可直接给下一位 Codex 的启动提示

```text
你要继续开发 Kēne editor。先阅读 AGENTS.md、docs/PROJECT_STATE.md、
docs/editor-phase0.md、docs/editor-phase1.md 和 docs/EDITOR_HANDOFF.md，然后只阅读本次
分配的一个 docs/tasks/TXX-*.md。
先检查 worktree 和相关 diff，不覆盖未提交修改。不要把
crates/loader/src/adapter/editor 当成 Kēne editor GUI；它是第三方编辑器项目格式的
只读适配层。保持 keine-core <- keine-loader <- keine，core/loader 必须 Bevy-free，
editor 私有文档和 UI 状态不得进入 runtime/core。`crates/editor` 已有 GPUI Phase 1 只读
工作台，不要重建窗口、Dock、项目身份或 app-data 生命周期。任务文件必须明确选择可写文档
或 Preview 中的一个闭环，不要搭通用框架。只改任务所有权范围，完成后跑规定验证并提供
真实 UI 证据；未经明确要求不要提交、合并或推送。
```
