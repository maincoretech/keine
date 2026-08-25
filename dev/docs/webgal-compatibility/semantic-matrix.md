# WebGAL 4.6.2 语义矩阵

## 判定规则

本矩阵逐项覆盖 WebGAL 4.6.2 官方注册表中的 31 个文档化命令。状态采用保守口径：

- **已实现**：文档化主语义已被当前 IR 表达，parser 与 core 行为有自动测试，且不存在已知的命令本地语义差距。
- **部分支持**：已有可用链路，但参数、默认值、资源类型、阻塞/继续、呈现行为或验证证据至少有一项不完整。
- **不支持**：没有等价 Action/runtime；loader 明确给出 warning 并跳过，不能降级成 `say`。

矩阵状态统计：**已实现 5 / 部分支持 23 / 不支持 3 / 合计 31**。

证据缩写：

- **P**：[`crates/loader/src/adapter/script/webgal.rs`](../../../crates/loader/src/adapter/script/webgal.rs)
- **C**：[`crates/core/src/model/action.rs`](../../../crates/core/src/model/action.rs)、[`state.rs`](../../../crates/core/src/model/state.rs)、[`runtime/step.rs`](../../../crates/core/src/runtime/step.rs)
- **R**：[`src/scene/`](../../../src/scene)、[`src/ui/`](../../../src/ui) 或 [`src/runtime/`](../../../src/runtime) 的消费链路
- **T**：上述模块内的 Rust 自动测试
- **M**：[`projects/test-project`](../../../projects/test-project) 与 [`dev/docs/acceptance`](../acceptance) 中的可玩/人工验收场景；项目或验收步骤存在本身不是视觉通过证明

## 31 命令逐项结论

| # | 命令 | WebGAL 4.6.2 文档合同 | keine 当前行为与证据 | 状态 | 主要差距 |
|---:|---|---|---|---|---|
| 1 | `say` | 对话/旁白、继承或清空 speaker、`vocal`、`notend`、`concat`、多行、`fontSize`、立绘嘴型驱动 | P+C+R+T：两种 speaker 写法、语音/音量、继承/clear、concat/notend、转义、多行、插值、富文本/ruby 子集、Backlog 与打字机已接通 | **部分支持** | 不支持持久 `fontSize` 与 `left/right/center/figureId` 嘴型同步；`-continue` 不等价；表达式/富文本并非 WebGAL 全集 |
| 2 | `changeBg` | 入场/替换/退场；transform、enter/exit、分段时长、ease、CG 自动收录 | P+C+R+T+M：图片/none、若干 transition、基础 transform/filter、阻塞与材质转场可用 | **部分支持** | 默认 1500 ms 淡入淡出、`enterDuration/exitDuration`、完整 14 easing、完整 filter、`unlockname/series` 未等价；当前无参数时是 Instant |
| 3 | `changeFigure` | 图片/Live2D/Spine；位置/id/zIndex/blend；入退场；完整 transform；嘴型/眨眼差分 | P+C+R+T+M：静态图片、空内容/`none`/`-clear`/`-none` 退场、left/center/right、自定义 id、zIndex、`blendMode`（兼容旧 `blend`）、基础 transform 和若干转场可用 | **部分支持** | 无 Live2D/Spine/GIF、嘴型/眨眼、motion/expression；默认时长、完整 easing/filter 仍不等价；left+right 优先级不同 |
| 4 | `bgm` | 播放/切换/停止/同文件调参；volume、enter；`unlockname/series` | P+C+R+T+M：循环播放、空路径/`none` 停止、音量钳制、淡入/淡出和分总线已接通 | **部分支持** | 同路径调参会通过 revision 重新同步而非保证无缝；未处理 `unlockname/series`；缺少 GUI 音频人工记录 |
| 5 | `playVideo` | 全屏视频、临时静音 BGM/voice、完成后继续、`skipOff` | P+C+R+T：typed VideoSpec/State、按时钟有界流式解码、动态 GPU 纹理、音频 ducking、阻塞与双击跳过已接通 | **部分支持** | FFmpeg 后端为按项目启用的桌面 feature；尚缺 macOS/Windows/Linux 分发打包和跨平台音画/读档人工验收 |
| 6 | `pixiPerform` | 播放 `cherryBlossoms/rain/snow/heavySnow`，可叠加不同特效 | P+C+R+T+M：映射为固定容量、原生 Bevy 粒子，支持按名称选择若干视觉风格 | **部分支持** | 状态只能保存一个效果，不能叠加；`cherryBlossoms` 与 `heavySnow` 不完全等价；不是 WebGAL Pixi 生命周期 |
| 7 | `pixiInit` | 初始化/清空全部舞台特效 | P+C+R+T+M：清空当前原生粒子层 | **部分支持** | 清空链路存在，但依赖尚未等价的单效果 `pixiPerform` 模型，且没有视觉 golden |
| 8 | `intro` | 多行全屏文字；字号/前景/背景色/背景图；五种动画；delay/hold/userForward | P+C+R+T+M：多页文字、黑底白字淡入、自动翻页与 hold 基础状态可用 | **部分支持** | 不支持颜色、字号、背景图、动画选择、`delayTime`、`userForward`；上游文档与源码默认字体色还存在漂移 |
| 9 | `miniAvatar` | 显示、替换、隐藏文本框小头像 | P+C+R+T+M：路径/none 状态与淡入 UI 投影已接通 | **部分支持** | 替换、隐藏过渡与多 DPI 行为尚未证明等价 |
| 10 | `changeScene` | 替换当前场景且不返回；保留未主动清理的舞台状态 | P+C+T：递归加载并保留嵌套相对路径；场景替换、无返回和缺失目标诊断有自动测试 | **已实现** | 无已知命令本地语义差距；视觉舞台保留仍在人工矩阵中回归 |
| 11 | `choose` | 选项阻塞；label/scene 目标；条件显示与条件启用；无效目标继续 | P+C+R+T+M：条件过滤/禁用、键鼠选择、label/changeScene 以及额外的 callScene 目标可用；转义分隔符与 `[clues[2]]` 等嵌套数组条件已测试 | **部分支持** | `-when` 表达式是 keine 安全子集；预览专用 `defaultChoose` 未实现；hover/pressed/touch 与多尺寸仍无完整视觉证据 |
| 12 | `end` | 结束场景并返回标题 | C+R+T+M：清理剧情舞台、交互和音频状态，设置 ended 并回到标题链路 | **已实现** | 无已知命令本地语义差距 |
| 13 | `setComplexAnimation` | `universalSoftIn/universalSoftOff` 复杂动画，target 与 duration | P+C+R+T+M：与本地 `Animate` 统一时间线，未知名称有有界 fallback | **部分支持** | 官方两个复杂动画没有等价曲线；fallback 只是原生近似，不能视为兼容 |
| 14 | `label` | 定义当前场景 label；命令本身无副作用 | P+C+T：Program 构建时一次性索引，执行期 no-op | **已实现** | keine 与 WebGAL 4.6.2 实际源码均对重复名取最后一个；官方文档仍写“第一个”，见下方漂移 |
| 15 | `jumpLabel` | 跳到当前场景 label；支持 `-when` | P+C+T：O(1) label 查询、条件跳转、缺失 label 安全继续和 runaway 上限有测试 | **已实现** | 无已知命令本地语义差距；条件表达式全集差距归公共 `-when` |
| 16 | `setVar` | 设置本地/全局变量；数值、布尔、字符串与表达式 | P+C+T+M：数字/布尔/带引号字符串、数组、算术/比较/逻辑、索引赋值、global 与插值可用；global 独立写入 profile，成功恢复 fingerprint 匹配的存档或 rollback 时保留当前长期值 | **部分支持** | 未识别裸值不会像 WebGAL 那样稳定回退为字符串；不执行任意 JS/用户数据对象语义 |
| 17 | `callScene` | 临时进入场景，自然结束后返回调用点；保留舞台 | P+C+T：嵌套相对路径、LIFO 调用栈、自然 EOF 返回和嵌套调用有端到端测试 | **已实现** | 无已知命令本地语义差距；视觉舞台保留仍需人工回归 |
| 18 | `showVars` | 在对话框显示全部本地和全局变量 | P+T：保留字被识别并产生明确 unsupported warning | **不支持** | 无调试对话 Action/UI，也没有稳定的排序/格式合同 |
| 19 | `unlockCg` | 按路径/名称/系列收录 CG，重复路径采用最后一次元数据 | P+C+R+T+M：路径和 name 写入持久 gallery，并在 EXTRA 显示 | **部分支持** | State 只保存 path→name，不保存 `series`；无分页/全屏视觉验收记录 |
| 20 | `unlockBgm` | 按路径/名称/系列收录 BGM，重复路径采用最后一次元数据 | P+C+R+T+M：路径和 name 写入持久 gallery，EXTRA 可选曲播放 | **部分支持** | 不保存 `series`；鉴赏播放与持久化缺正式人工证据 |
| 21 | `filmMode` | 空/none 关闭，其他内容开启电影模式 | P+C+R+T+M：切换固定舞台上下黑边 | **部分支持** | 仍没有 resize/DPI/完整层级生命周期和自动视觉 golden |
| 22 | `setTextbox` | `hide` 持续隐藏，其余值显示；`:` 是自动恢复的简写 | P+C+R+T+M：持续 hide/show 与自动隐藏到下一句均有状态/UI 链路 | **部分支持** | 无“所有 overlay/控制栏/mini avatar”组合视觉证据；公共 `-continue` 仍不等价 |
| 23 | `setAnimation` | 按名称加载可复用多段动画；target、writeDefault、keep | P+C+R+T+M：若干内建 preset、target、duration、阻塞和本地 GPU film effect 可用 | **部分支持** | 不读取 WebGAL animation 文件/表；忽略 `writeDefault/keep`；自定义名 fallback 不等价；目标/滤镜全集缺失 |
| 24 | `playEffect` | 无 id 单次播放/替换/停止；有 id 独立循环；volume | P+C+R+T+M：单次 cue、按 id 循环/替换/停止、音量钳制与分总线已接通 | **部分支持** | 尚无可引用的人工听测/录屏；一帧累计多个一次性 cue 只保留最终消费事件，需与上游时序再对照 |
| 25 | `setTempAnimation` | 直接以内联 JSON 定义多段动画；target、writeDefault、keep | P+C+R+T+M：命令进入统一 Animate 生命周期 | **部分支持** | 未解析 JSON keyframe 数组；只把整段内容当 preset/custom 名处理；easing、继承、keep 均不等价 |
| 26 | `setTransform` | 对现有状态做稀疏 transform patch；duration、14 easing、writeDefault、keep；多类 filter | P+C+R+T+M：32-byte `TransformPatch` 只覆盖出现字段并区分显式 0；position/scale/rotation/alpha/blur、默认 500 ms、显式 0 ms、target 与 4 类 easing 可用 | **部分支持** | 尚缺 10 类 easing、writeDefault/keep、stage-main 与大量 filter；仍无连续 patch 的视觉 golden |
| 27 | `setTransition` | 为目标设置后续 enter/exit 动画 | P+C+R+T+M：target、enter/exit、duration 与若干 native preset 的持久规则可用 | **部分支持** | 自定义动画名与 WebGAL animation table 不等价；默认时长/完整 target 和退出行为尚缺视觉证据 |
| 28 | `getUserInput` | title/button/defaultValue；regex rule/flags/error 文案；确认后写变量并继续 | P+C+R+T+M：标题、按钮、键盘输入、非空确认、变量写入和阻塞恢复可用 | **部分支持** | 不支持 defaultValue、rule/ruleFlag/ruleText/ruleButtonText；空输入行为与 WebGAL 不同；IME/触控/高 DPI 输入行为未完整验收 |
| 29 | `applyStyle` | 将一个或多个 UI 模板样式名运行时映射到新样式 | P+T：保留字被识别并产生明确 unsupported warning | **不支持** | keine 没有 WebGAL React UI class/template 映射层；不会把 CSS 路径或映射静默套用到 Bevy UI |
| 30 | `wait` | 等待指定毫秒，结束后自动继续 | P+C+R+T+M：确定性的秒数状态、阻塞与计时推进可用 | **部分支持** | 上游源码存在未文档化 `-nobreak`，keine 不支持；`wait -next` 的上游“不兼容”行为与本地 flow 包装仍需专门回归 |
| 31 | `callSteam` | Electron/Steam 桥接解锁 `achievementId` | P+T：保留字被识别并产生明确 unsupported warning | **不支持** | 无 Steam AppID/桥接/成就 API；普通桌面构建也不会伪造成功 |

## Parser 与公共参数

### 已覆盖

- 第一个未转义 `;` 之后是注释；支持 `\:`、`\,`、`\.`、`\;`、`\|`。
- 对话里的未转义 `|` 转为换行；Choice/Intro 用未转义 `|` 分段。
- 官方 `say:文本 -speaker=名称` 和简写 `名称:文本` 均可解析。
- 官方 `changeFigure -blendMode=` 优先，兼容旧项目的 `-blend=`。
- 官方 `setTransform -ease=` 优先，兼容旧项目的 `-easing=`。
- scene key 保留嵌套相对路径；脚本目录递归加载，同 stem 的不同目录不会碰撞。
- `showVars/applyStyle/callSteam` 产生带 source span 的 warning，不会误降级为普通对话；`playVideo` 产生 typed Action 并扫描视频资源；其他未知行保持 WebGAL 的“尝试作为 say”策略。

对应新增/更新自动测试：

- `parses_webgal_semicolon_comments_escapes_and_dialogue_lines`
- `parses_official_and_simplified_say_speaker_rules`
- `parses_official_blend_mode_and_ease_with_legacy_aliases`
- `parses_escaped_choice_delimiters_and_nested_targets`
- `parses_nested_array_access_in_choice_conditions`
- `json_transform_patch_preserves_every_absent_field`
- `set_transform_distinguishes_default_and_explicit_zero_duration`
- `reports_reserved_unsupported_commands_without_dialogue_fallback`
- `video_is_typed_blocking_and_resource_tracked`
- `parses_scene_control_commands`
- `recursively_loads_and_executes_nested_scene_paths`
- `same_stem_in_different_directories_has_distinct_scene_names`

### 横切差距

| 语义 | WebGAL 4.6.2 | keine |
|---|---|---|
| `-next` | 当前命令与后续命令同步启动；若干命令默认自带 | 非阻塞 Action 会自然继续；演出 Action 通过 `Flow.next` 取消阻塞，基本路径可用 |
| `-when` | 条件为真才执行 | 有统一条件包装；Choice parser 能正确跨过嵌套数组索引/括号/引号，但表达式仍是安全 Rust 子集，不是 JavaScript 全集 |
| `-continue` | 当前演出结束后自动执行下一句 | 参数可被 parser 吃掉而不污染正文，但 core 没有等价的“展示完成后自动继续”状态 |
| 重复参数 | 上游 parser 采用首次命中的参数 | keine `HashMap` 当前以后出现的值覆盖先出现的值 |
| 未知命令 | 尝试作为对话 | 保留该策略，但三个明确不支持的官方保留字会先被拦截并告警 |

## 官方文档与 4.6.2 源码漂移

兼容实现不能只复制文档示例；下列项目在同一基线内已经不一致：

| 项目 | 文档 | WebGAL 4.6.2 源码 | Kēne 合同 |
|---|---|---|---|
| 重复 `label` | 从上到下取第一个 | 构建映射时后项覆盖前项，实际取最后一个 | keine 取最后一个并记录漂移 |
| `intro fontColor` 默认值 | 黑色 | [`intro.tsx`](https://github.com/OpenWebGAL/WebGAL/blob/e7f0abeb855b5b442460743bdaa9778ca751b43f/packages/webgal/src/Core/gameScripts/intro.tsx) 使用白色 | 以运行时源码为行为基线 |
| `changeFigure -none` | 文档称可清空内容 | [`changeFigure.ts`](https://github.com/OpenWebGAL/WebGAL/blob/e7f0abeb855b5b442460743bdaa9778ca751b43f/packages/webgal/src/Core/gameScripts/changeFigure.ts) 明确读取 `-clear`；内容 `none`/空仍可退场 | 两种参数均不能宣称完整兼容 |
| `say fontSize` | 设置后持续到再次设置 | [`say.ts`](https://github.com/OpenWebGAL/WebGAL/blob/e7f0abeb855b5b442460743bdaa9778ca751b43f/packages/webgal/src/Core/gameScripts/say.ts) 在缺参时回到用户默认 | keine 尚未实现，先记录冲突 |
| transform 键 | 文档列 `shockwave`、`radiusAlpha` | runtime/type 使用 `shockwaveFilter`、`radiusAlphaFilter` | 未实现前不猜测别名 |
| `pixiInit` 示例 | 示例写 `pixi:rain` | 注册表只有 `pixiPerform` | 矩阵以注册表为准 |
| 场景首句 | 教程描述首句需要点击 | 4.6.2 scene load 后会自动继续首个可执行动作 | 以 4.6.2 runtime 为准 |
| 越界音量 | 教程曾描述回退 100 | runtime 实际钳制到 0..100 | keine 同样钳制 |
| `setFilter` | 不在 31 命令文档目录 | 注册但 [`setFilter.ts`](https://github.com/OpenWebGAL/WebGAL/blob/e7f0abeb855b5b442460743bdaa9778ca751b43f/packages/webgal/src/Core/gameScripts/setFilter.ts) 是 no-op | 作为 keine 扩展记录，不计兼容数量 |
| `animationFlag` | 文档化为差分动画标志 | 值被保存但后续嘴型/眨眼按目标 id 与差分资源读取，flag 本身未消费 | 不把“成功解析”计为行为支持 |

## 上游源码存在、当前文档未公开的参数

这些参数不改变 31 命令统计，但会影响“源码级兼容”判断：

- `wait -nobreak`
- animation `-parallel`
- animation `ignoreDefault`
- CG 的 `order` 等元数据
- 仅实时预览消费的 `choose -defaultChoose`

在为这些参数补实现前，应先写固定 4.6.2 源码行为测试；不能依据名称猜语义。

## 当前自动验证结果

当前工作树包含稀疏 TransformPatch、嵌套 Choice 条件和 profile 分离测试。实际结果以仓库根目录执行测试命令的输出为准；本文件不复制可能很快过期的全量数字。

自动测试通过也不证明 GUI 视觉矩阵已经通过。
