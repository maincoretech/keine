# WebGAL_K 对照审计

> 审计基线：本地 WebGAL_K `4.6.1`（commit `b5a6fa08`）与 keine `0.2.0`
>（commit `a044b5a`）。本文件记录代码层面的实际能力，不以界面上已有按钮或 parser
> 能接受某个关键词作为“已完成”。

## 结论

keine 已经完成 Phase 1 的 WebGAL 核心运行链：表达式/数组/全局变量、公共 flow 参数、
条件 Choice、跨场景调用、source span 诊断、本地资源扫描预取、vocal 和标题结束流程均已
端到端接通。它仍是 **WebGAL 的明确子集**，下一阶段的主要缺口不是基础 parser，而是：

1. Backlog、已读记录、回滚和完整存档模型；
2. BGM、SE、Replay 与分总线音量混音；
3. 演出生命周期、阻塞语义和高级合成；
4. 真正的富文本/ruby 排版与输入类命令；
5. 移动端输入、安全区和发布打包。

## 结构对照

| WebGAL_K 结构 | 职责 | keine 当前对应 | 缺口 |
|---|---|---|---|
| `packages/parser` | 命令、参数、资源和子场景扫描，保留行信息 | `keine-loader::ParseReport` | 已有 source span、明确诊断、资源/子场景引用；仍缺发布前独立 check CLI 与不可达分析 |
| `Core/Modules/scene` | 当前场景、场景栈和恢复点 | `State.scenes/current_scene/cursor/scene_stack` | `changeScene/callScene/end`、自然返回、舞台清理与标题重开已接通；缺 rollback 恢复点 |
| `scriptExecutor` | `-when`、变量插值、`-next`、`-notend`、执行上限 | `core::step` + `expression` | 公共 flow、表达式/数组/全局变量和 1024 action 上限已实现；高级命令仍待扩展 |
| `stageStateManager` | 计算态、展示态、commit 与恢复 | 单个 `State` + ECS 同步 | 没有演出提交边界、阻塞演出、状态快照和回滚 |
| `performController` | 演出挂载、阻塞、卸载 | transition progress | 只有少量背景/立绘 transition，没有统一 perform 生命周期 |
| `BacklogManager` / `ReadHistoryManager` | 历史快照、已读位图、回想跳转 | 无 | 当前 `State` 没有 Backlog 或已读记录 |
| `controller/storage` | 多槽存档、快存、预览、时间、用户数据 | bincode 单槽快存 + 持久化舞台快照预览 | 没有槽位元数据、设置/全局数据、迁移和回滚存档 |
| `Stage/AudioContainer` | BGM、vocal、SE、UI SE | Bevy vocal + `Bgm` action stub | 单句 vocal 已播放；BGM/SE/Replay 和分总线混音未实现 |
| React `UI` | Choice、Backlog、Title、Save/Load、Options、Extra、Flowchart | Textbox、ControlBar、Dialog、桌面 Choice、Title | Choice/快存读/标题已可用；Backlog、正式存档、Options、Extra、Flowchart 未实现 |
| Pixi stage | filter、复杂动画、粒子、GIF、Spine/Live2D | Bevy Sprite + 全屏 Dialog blur | 大部分高级演出未实现 |

## 脚本能力对照

### 已有基础，但仅部分兼容

| 命令 | 当前能力 | 仍缺少 |
|---|---|---|
| `say` | speaker、打字机、vocal/volume、notend/concat、公共 flow、插值、富文本可读降级 | 真正的样式 span/ruby 排版、`-name` 完整细节 |
| `changeBg` | 设置/none、transition、transform/filter、blur 与公共 flow | 更复杂 filter 和自定义 transition |
| `changeFigure` | left/center/right、显示/移除、自定义 id、zIndex、transform、blur | add/multiply/screen 实际合成、口型/眨眼 |
| `choose` | 桌面 Choice、条件显示/禁用、label/changeScene/callScene 目标 | 移动端布局与触控验收 |
| `label/jumpLabel` | 当前场景标签、`-when` 和插值跳转 | 跨场景流程由 scene manager 承担 |
| `changeScene/callScene/end` | 两种格式、嵌套调用、自然返回、舞台清理、标题重开、公共 flow | rollback 恢复点 |
| `setVar` | WebGAL、表达式、数组元素、全局/局部变量和插值 | 对 WebGAL 全部 JS 动态表达式的兼容不作为目标 |
| `setTransform` | JSON/键值、duration/easing、立绘/背景 target 和 blur | 统一阻塞 perform 生命周期、复杂 filter |
| `miniAvatar` | 显示/隐藏和淡入 | WebGAL 参数与完整布局行为 |
| `bgm` | parser/action 存在 | Bevy 没有播放；volume/enter/fade/stop 语义未接通 |

### 完全或实质未实现的 WebGAL_K 命令

- 条件与输入：`if`、`getUserInput`
- 音频：`playEffect`、实际 `bgm`、Replay 与音量总线
- 演出：`setAnimation`、`setTempAnimation`、`setComplexAnimation`、
  `setTransition`、`setFilter`、`pixiPerform`、`pixiInit`
- 叙事/UI：`intro`、`filmMode`、`setTextbox`、`applyStyle`、`playVideo`
- 鉴赏/平台：`unlockCg`、`unlockBgm`、`callSteam`
- 调试：`showVars`

注释只完成了空行、`;` 全行注释与 `//` 行内注释；显式 `comment:` 目前会被误解析为
普通对话，也应纳入 parser 修复。

## UI、存储与运行时缺口

### UI

- Choice 的条件显示/禁用、跨场景目标与移动端布局；
- Backlog 列表、滚动、语音重播和回想跳转；
- 多槽 Save/Load、槽位截图、角色名/文本/时间预览；
- Options：主音量、BGM/vocal/SE/UI SE、文字速度、自动速度、字号、透明度、
  字体、语言、全屏和“只跳已读”；
- Title、启动页、Extra CG/BGM、Flowchart、全局错误界面；
- UI SE 与完整本地化。主题、运行时换肤和任意自定义 UI 是专用引擎明确放弃的范围。

### 状态与存储

- 场景调用栈和返回位置；
- 每句 Backlog 快照以及容量上限；
- 按场景/句号持久化的已读记录，供 Skip All/Read 使用；
- 存档格式版本、迁移、损坏恢复和游戏 key 隔离；
- 快存与正式存档分离，多槽分页、覆盖/删除确认；
- 用户设置、全局变量、鉴赏解锁与存档状态分离；
- Backlog 回滚需要恢复舞台、变量、场景栈和 cursor，而不只是文本。

### 资源与舞台

- 缺失资源的发布前硬错误与跨场景资源图可视化（运行时已有扫描、前看预取和加载状态）；
- BGM/vocal/SE 生命周期、淡入淡出与音量总线；
- GIF、视频、Live2D、Spine 和自由立绘；
- filter、复杂 transform、粒子、转场和阻塞演出；
- WebGAL 的计算态/展示态分离以及 perform commit/rollback。

## 建议实现顺序

1. ~~统一 WebGAL flow/表达式/source span/资源扫描。~~ Phase 1 已完成。
2. ~~Choice label/changeScene/callScene 与条件显示/启用。~~ Phase 1 已完成。
3. **Backlog + read history + rollback snapshot**：先建立正确状态模型，再做 UI 和 Skip 语义。
4. **版本化多槽存档**：基于上一步的快照补齐 preview、metadata 和迁移。
5. **音频总线**：BGM、vocal、SE、Replay 和设置页一起接通。
6. **演出控制器**：统一 animation/transition/filter/wait/blend 的生命周期和阻塞语义。
7. **高级兼容**：富文本、视频、Live2D/Spine、鉴赏、Flowchart、Steam、编辑器同步。

不建议直接照搬 WebGAL_K 的 Redux/React/Pixi 组织。应保留 keine 的
`core -> loader -> bevy` 边界，只借鉴它的职责划分和行为语义。
