# 不支持项与兼容边界

## 明确不支持的 3 个官方命令

这三个关键词在 loader 中作为 WebGAL 保留字单独识别。它们会产生带行列信息的 warning，然后被跳过；不会进入 core，也不会被未知命令回退策略误显示成普通对话。

| 命令 | WebGAL 行为 | keine 当前行为 | 缺失能力 | 引入前置条件 |
|---|---|---|---|---|
| `showVars` | 在对话框输出本地/全局变量 | warning + skip | 调试展示 Action、稳定排序/格式、敏感值策略 | 定义仅开发模式还是发布版可用；编写确定性格式测试 |
| `applyStyle` | 把 WebGAL UI 模板样式名运行时映射到新样式 | warning + skip | React/CSS class/template 等价层 | keine 使用 Bevy UI，需先设计 typed theme token 映射；不能直接执行项目 CSS |
| `callSteam` | 通过 Electron/Steam 桥接解锁 achievementId | warning + skip | AppID 配置、Steam SDK/桥接、成就结果与平台错误处理 | feature-gated 后端、再分发许可、非 Steam 构建的确定性行为、真实客户端验收 |

自动保护测试：`reports_reserved_unsupported_commands_without_dialogue_fallback`。

## 高风险部分支持项

下列命令不是“完全不可用”，但当前差距会显著改变剧情或画面，迁移现有 WebGAL 项目时必须优先检查。

### P0：会改变流程或最终状态

| 缺口 | 影响 | 当前安全处理 | 完成标准 |
|---|---|---|---|
| 公共 `-continue` 未建模 | 演出结束后不能按 WebGAL 时机自动推进 | 参数不污染对话文本，但没有伪造语义 | 在 core 中区分 click wait、presentation completion 与 auto-continue；补 wait/intro/transform 连续链测试 |
| `setTempAnimation` 不解析 keyframe JSON | 多段动画变成单一 fallback 效果 | 有界 native fallback，不执行脚本代码 | 编译 JSON 为 typed keyframe；逐段 duration/ease/继承；补中断、keep 与存档测试 |
| `setAnimation` 不读取 animation table/file | 项目自定义动画失真 | 已知 native 名可用，未知名 fallback | loader 解析动画资源并发布诊断；core 使用不可变 keyframe payload |
| `setComplexAnimation` 官方曲线未实现 | `universalSoftIn/Off` 视觉错误 | generic fallback | 实现两个固定曲线并对照上游 0/50/100% 状态 |
| `getUserInput` 缺默认值与校验 | 空输入和非法输入会走不同分支 | 本地 UI 只允许非空提交 | 实现 defaultValue、regex rule/flag/error 文案；补 IME、空输入、读档测试 |
| `setVar` 裸字符串回退不同 | `setVar:name=Bob` 可能不赋值 | evaluator 报错，不执行任意代码 | 与 4.6.2 固定样例对照；未知值安全回退字符串且不扩大表达式攻击面 |
| scene 条件表达式只是安全子集 | `-when`/Choice 的复杂表达式结果可能不同 | 解析错误时记录错误并按 false | 明确支持语法表；为拒绝项提供发布前诊断和迁移说明 |

### P1：会改变呈现、资源或平台行为

| 命令/域 | 当前子集 | 仍缺少 |
|---|---|---|
| `changeBg` | 图片、none、基础 transform 与 native transition | 默认 1500 ms、分离 enter/exit duration、完整 ease/filter、unlockname/series |
| `changeFigure` | 静态图、id/位置/z/blend、基础 transform | Live2D/Spine/GIF、差分嘴型/眨眼、motion/expression、完整参数/默认值 |
| `intro` | 黑底白字、多页、hold | font/background、backgroundImage、五种 animation、delayTime、userForward |
| `pixiPerform` | 单一固定容量 native 粒子 | 多效果叠加、四个官方 preset 精确外观与强度 |
| `bgm` | 循环、volume、fade、空路径/`bgm:none` stop | 同路径无缝调参、unlockname/series、正式听测记录 |
| `playEffect` | one-shot、id loop、volume、stop | 同帧多 cue 顺序保证、正式听测/录屏证据 |
| `unlockCg/Bgm` | path→name 持久化与 EXTRA | series/order 元数据与完整鉴赏视觉验收 |
| `filmMode`/`setTextbox`/`miniAvatar`/`pixiInit` | 主状态与 Bevy UI/scene 投影存在；filmMode/miniAvatar 已有单平台组合帧抽样 | 自动 golden、完整生命周期、resize/DPI、叠层和输入隔离证据 |
| `setTransition` | native enter/exit rule | WebGAL 自定义 animation table、完整 target/default、视觉关键帧 |
| `setTransform` | 32-byte 稀疏 patch、默认 500 ms、显式 0、7 个基础字段、4 easing | 其余 10 easing、writeDefault/keep、stage-main、完整 filter 与连续 patch golden |

## 已关闭的结构性缺口

以下项目已有实现与定向测试，不应继续出现在“尚未实现”清单中：

- `setTransform` 已改为 presence-mask `TransformPatch`，未出现字段继承当前状态，显式 0 可清除旧值。
- Choice 条件提取会跟踪括号深度、引号和转义，`[clues[2]]` 与带嵌套数组/括号的条件不会在内层 `]` 提前截断。
- `global_vars` 已从单槽 save 与 Backlog rollback 分离，使用独立 `saves/profile.bin`；读档保留当前 profile。
- CLEAR ALL 删除整个 `saves/` 数据域并同步清空 settings/profile/read/gallery 与 writer cache，磁盘、内存和重复调用已有自动回归。
- `playVideo` 已使用 typed Action/State、按播放时钟有界解码、动态 GPU 纹理、音频 duck、
  阻塞和双击跳过；缺少 `video-ffmpeg` 的最小构建会明确报错，不能冒充播放成功。剩余工作是
  各桌面平台 FFmpeg 分发和组合人工验收，不再属于 parser/runtime 缺失。

这些修复没有把对应命令自动升级为“已实现”：`setTransform` 仍缺完整参数/滤镜/视觉证据，Choice/`-when` 仍是安全表达式子集；profile 剩余缺口主要是 GUI 端到端与 import/export 完整流程，而不是基础磁盘 round-trip 或 CLEAR ALL 生命周期。

## 全局语法边界

### `-next`

多数无阻塞命令在 core 单步中自然继续，演出命令会用 `Flow.next` 取消等待。仍需专门对照：WebGAL 明确称 `wait -next` 不兼容，而 keine 的通用 flow 包装可能使它成为非阻塞等待。

### `-when`

keine 使用确定性的表达式 evaluator，支持常见算术、比较、布尔、数组和变量，但不执行任意 JavaScript。安全优先于表面上的“全 JS 兼容”；超出子集的脚本必须在发布前得到明确诊断。

### `-continue`

当前 parser 能识别它是参数，因此不会泄漏进对话文字；core 尚无对应字段和演出完成推进状态。任何依赖连续自动演出的脚本都应视为部分兼容。

### 参数覆盖顺序

WebGAL parser 的参数查找通常取首次出现值；keine 当前存入 `HashMap`，后出现的同名参数覆盖先出现值。正常脚本不应重复参数，但兼容测试需要固定此差异。

## 文档外的上游行为

WebGAL 4.6.2 源码存在但当前脚本参考未公开的参数/路径包括：

- `wait -nobreak`
- animation `-parallel`
- animation `ignoreDefault`
- CG/BGM 的 `order` 等元数据
- 实时预览专用 `choose -defaultChoose`
- 已注册但实现为空的 `setFilter`

这些项目不计入“31 个文档化命令”覆盖率。若目标升级为源码级兼容，应为每项先建立上游行为 fixture，再决定实现或明确拒绝。

## keine 扩展

以下行为可服务 keine 自身项目，但不能反向宣称为 WebGAL 官方语义：

- `setFilter`：keine 提供 blur/brightness/contrast/saturation 的原生子集；WebGAL 4.6.2 同名实现目前是 no-op。
- `stopBgm`：历史兼容别名；官方写法是 `bgm:none` 或 `bgm:`。
- `comment:`：历史显式注释扩展；官方注释语法是从第一个未转义 `;` 开始。
- Choice 的 `callScene(...)` 目标：keine 扩展；官方文档只列 scene 文件和当前场景 label。

扩展必须保持命名明确、资源有界、无隐式网络/脚本执行，并且不能改变官方写法的解析优先级。

## 不应伪装支持的媒体类型

静态占位图不能冒充视频、Live2D 或 Spine；generic blur 不能冒充官方复杂/临时动画；日志中打印 achievementId 也不能冒充 Steam 解锁。对这些能力，明确 warning 比“看起来执行了”更安全，因为后者会让剧情制作方漏掉真实发布错误。

## 升级状态的统一门槛

从“不支持”升级到“部分支持”至少需要：typed Action、core 状态、缺失资源诊断和一条端到端测试。从“部分支持”升级到“已实现”还需要：

1. 官方 4.6.2 文档参数与默认值的 fixture；
2. parser、state/runtime、renderer/UI/audio 的消费测试；
3. 会改变画面的命令拥有固定关键帧 golden 或带环境记录的人工截图；
4. 阻塞、`-next`、`-continue`、Skip/Auto、读档和返回标题的组合回归；
5. 不支持的资源类型或平台路径继续给出明确、可定位的诊断。
