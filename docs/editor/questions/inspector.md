# Kēne Editor 与 Inspector 问答文档

## 1. 文档用途

这份文档用于与 Chat 从零讨论 Kēne Editor 的核心编辑体验，以及 Inspector 应当承担的职责。
它不是一份“照着常见 IDE 复制”的需求单，也不允许通过建立第二份隐藏数据模型来获得表面上的可视化编辑。

问答的最终目标是确定：作者当前在编辑什么、选择如何传播、哪些属性可以安全修改、源码如何保持权威，
以及 Editor、Inspector、Problems 与 Preview 如何协同而不重复事实。

## 2. 给 Chat 的工作方式

1. 一次只问一个问题。
2. 每题提供 2–3 个方向，说明对新手、熟练作者、源码保真和实现复杂度的影响。
3. 先确认工作流，再讨论控件形态；不要从“Inspector 应该有哪些输入框”开始。
4. 用户回答后记录决定、尚存歧义和需要真实验证的场景。
5. 不把 Text、Block View、Dialogue 当作三份文档；它们必须是同一源码的投影。
6. 不提出多窗口泛滥、假标签页、通用节点编辑器或插件框架。
7. 问答阶段不改代码；若方向跨越 Editor、Loader、Core 或 Runtime，必须明确指出。

可直接使用的开场提示：

> 请基于本文主持 Kēne Editor + Inspector 产品问答。先用自己的话概括当前实现和不可破坏的约束，然后一次只问一个待讨论问题。不要默认照搬 VS Code、Unity 或 LetsGal，也不要把推荐方向写成已实现功能。

## 3. 当前工作台结构

Kēne Editor 当前是单主窗口原生工作台：

- 左侧活动栏负责打开 Explorer、Assets、Characters、Scenes、Problems、Performance 和 Preview。
- Explorer 展示工程文件；长路径可以横向滚动。
- 中央文档组使用真正的文档标签页。
- Preview 默认位于右侧上方，Inspector 位于右侧下方。
- Output 位于中央下方。
- 单独存在的 Explorer、Inspector、Output 和工具视图使用集成标题区，不使用假标签页。
- Dock 支持重排、合并和边缘拆分，布局保存在 Editor app-data 中，不写入项目配置。
- 完全 dark mode，主强调色为 `#BAEBFF`，使用短圆角、统一外边距和克制动画。

## 4. 当前编辑能力

Eiyashou 原生工程中的以下内容可写：

- `config.yaml`；
- 配置指定的 Asset 与 Character Manifest；
- `scripts/**/*.shou`。

兼容 JSON、LetsGal 和 WebGAL 输入保持只读。

`.shou` 文档当前提供三种视图：

- Text：直接编辑带语法高亮的权威源码；
- Cards：把已知源码范围投影为结构化卡片，只重写被编辑的已知范围；
- Dialogue：连续展示并编辑叙述和对白，同样回写权威源码。

当前实现中的 `Cards` 是历史名称；目标产品名称统一为 **Block View**。改名不改变 source-first
合同，也不意味着建立新的文档模型。

三种视图共享同一文档、revision、dirty state、选择、撤销/重做、恢复草稿与保存流程。
未知语法必须保持可见，不能被 Card 或 Dialogue 投影吞掉。

当前插入栏可以根据工程已有资源和声明提供 Narration、Dialogue、Background、Figure、Choice 等入口。

## 5. 当前 Inspector 是什么

Inspector 当前是只读、选择驱动的摘要视图，主要显示：

- 工作区路径；
- 文本文件数量；
- 已打开文档数量；
- 当前文件、行和列；
- 当前选择对应 Source、Scene、Narration 或 Dialogue；
- 最多三条与当前文件相关的运行时诊断。

文档光标、Card/Dialogue 选择和 Preview source cursor 已建立关联，但 Inspector 目前不是完整的属性编辑器。
它没有多选、锁定、面包屑、可编辑属性组、引用列表或对象级撤销界面。

## 6. 不可破坏的合同

- 源码是唯一权威内容；Inspector 不能维护第二份正文或私有场景图。
- Editor UI 状态不能进入 Core 或 Runtime 的确定性状态。
- Core 与 Loader 必须保持 Bevy-free。
- 任何属性编辑都必须能映射到已知、有界的源码范围；不能理解的语法保持只读。
- 选择、诊断和 Preview 光标应共享明确的来源身份，不能靠模糊文本匹配。
- 保存必须保留现有原子写入、外部修改检查和恢复合同。
- 普通编辑不能隐式迁移或重排整个文档。

### 已确认的目标方向

- 主布局调整为左侧 File / Asset、中央 Editor、右侧 Inspector；布局持久化需要安全升级。
- Block View 连续展示当前 `.shou` 文件中的全部 Scene，以轻量、可折叠 section 组织。
- Block View 负责 Scene 定位、展开、折叠和 Scene 内 Block 编辑；Block 可以跨 Scene 选择、复制和移动。
- Scene 新建、重命名、删除和结构调整继续由 File、Text View 或已有源码路径承担，不在 Scene header
  再造第二套结构管理菜单。
- Text Block 直接编辑正文；其他 Block 在 Block View 显示摘要，由 Inspector 编辑安全参数。
- Toast 与 Tasks 采用当前简洁设计，只呈现失败、风险或确实耗时的操作，不逐项刷通知。
- 优先使用现有图标；常驻文字、Tooltip 和提示文本只保留完成判断所需的最少内容。
- 点击与选择同帧生效；动画只用于表现布局变化，保持短、平滑、可中断，不允许阻塞输入。
- UI 不在每帧重建源码投影、扫描文件或进行媒体工作；不可见列表项和缩略图不提前构建。

## 7. 待讨论问题

### Q1. Editor 的首要作者工作流是什么？

- A：源码优先，其他视图只是辅助导航。
- B：可视化投影优先，源码是高级模式。
- C：根据任务自由切换，三种视图保持同等可信。

**建议方向：C，但源码始终是权威。** 新手可以使用 Dialogue/Block View，熟练作者可以直接编辑 Text，
切换不应导致保存差异或丢失选择。

### Q2. Inspector 应当只读还是可编辑？

- A：继续只做证据与导航摘要。
- B：成为上下文属性编辑器，只编辑能够安全映射到已知源码范围的字段。
- C：承载完整的第二套可视化文档编辑器。

**建议方向：B。** C 会制造第二正文；A 则浪费已经建立的稳定选择和 source-range 能力。

### Q3. Inspector 跟随什么粒度的选择？

- A：只跟随当前文件。
- B：识别 Scene、Dialogue、Narration、Choice、命令、Asset 引用和 Character 引用。
- C：任何光标位置都生成一套通用键值属性。

**建议方向：B。** 不认识的节点退化为 Source Selection，不伪造通用属性。

### Q4. Inspector 是否需要锁定选择？

- A：永远跟随当前编辑位置。
- B：允许临时 Pin，继续浏览其他文件时保持当前对象。
- C：支持多个并排 Inspector。

**建议方向：B。** Pin 必须清楚显示来源文件和是否已经失效；不需要为此引入多窗口。

### Q5. 属性修改何时写回源码？

- A：每次输入立即产生文档 edit，沿用现有撤销与 dirty state。
- B：Inspector 内先积累草稿，点击 Apply 后统一写回。
- C：根据字段类型混用，但没有统一提示。

**建议方向：A 用于普通字段，B 仅用于多字段原子操作。** 无论哪种都必须进入同一文档撤销历史。

### Q6. 不完整或无效输入如何表现？

- A：禁止输入任何中间无效状态。
- B：允许控件内短暂草稿，离开或提交时验证；失败不写入权威源码。
- C：立即写入无效源码，再由 Problems 报错。

**建议方向：B。** 需要明确草稿状态、取消方式和键盘行为，不能显示假保存成功。

### Q7. Text、Block View、Dialogue 如何保持选择连续？

- A：切换模式后回到文档顶部。
- B：使用稳定 source identity/source range 映射到同一对象和最接近的位置。
- C：每个模式独立记忆自己的选择。

**建议方向：B，并可保留各模式滚动位置。** 对象身份优先，纯屏幕坐标不能成为选择依据。

### Q8. Inspector 应展示多少信息？

- A：所有可获得信息全部展开。
- B：顶部显示对象身份与状态，下面按内容、呈现、引用和问题分组，默认只展开当前任务需要的组。
- C：每种对象使用完全不同的独立页面。

**建议方向：B。** 避免卡片套卡片和大面积空白，也避免把一个简单属性埋进新页面。

### Q9. Asset 与 Character 应如何进入编辑动作？

- A：输入 Asset ID/Character ID 文本。
- B：Inspector 字段提供受限选择器，并能跳转到对应 Asset/Character。
- C：从独立管理器拖放到文档或字段。

**建议方向：B 作为基础，C 仅在真实拖放流程验证后增加。** 选择器必须显示缺失和类型不匹配。

### Q10. 多选是否进入第一阶段？

- A：不做，多选保持普通源码选择。
- B：只支持同类型对象的有限批量字段。
- C：任何对象都可组合编辑共有字段。

**建议方向：A 或非常窄的 B。** 没有明确批量用例前，不建立复杂的 mixed-value 属性系统。

### Q11. Problems 与 Inspector 如何分工？

- A：Inspector 复制当前对象全部问题，Problems 保留全项目列表。
- B：Inspector 只显示与当前字段/对象直接相关的最重要问题，并提供跳转；Problems 是完整集合。
- C：所有问题只放在 Problems。

**建议方向：B。** 同一错误不应在多个位置使用同等视觉重量重复出现。

### Q12. Preview 如何响应选择与编辑？

- A：任何光标移动都驱动 Preview 跳转。
- B：选择只同步 source cursor；真正改变运行状态需要明确的 Preview 行为。
- C：Inspector 编辑后自动重启 Preview。

**建议方向：B。** 需要分别决定 snapshot/patch、旧 revision 丢弃、运行位置变化和失败恢复，
不能把“选择同步”与“运行时已经执行到这里”混为一谈。

### Q13. Inspector 的宽度和小窗口策略是什么？

- A：固定宽度，中心编辑器承担全部压缩。
- B：设置可用的最小/默认宽度，窄窗口时优先收起次级组并允许 Inspector 滚动。
- C：小窗口自动变为覆盖式抽屉。

**建议方向：先验证 B。** 只有真实最小窗口证明确有需要时，再考虑覆盖式结构。

### Q14. 新建内容的主入口在哪里？

- A：文档顶部 Insert Palette。
- B：Inspector 根据当前选择显示 Add Before/After/Inside。
- C：右键菜单和快捷键优先。

**建议方向：保留 A 为可发现入口，再验证 B/C 是否能减少移动。** 不应出现三套功能相同但行为不同的插入系统。

### Q15. 哪些状态需要长期可见？

- A：dirty、只读、诊断、Preview revision、外部冲突全部放在 Inspector。
- B：状态放在最接近来源的位置，Inspector 只汇总当前对象确实需要的信息。
- C：统一放到底部状态栏。

**建议方向：B。** 一个事实只显示一次；例如文档 dirty 不应同时出现在标题、Inspector、Output 和通知中。

## 8. 问答结束后的交付结构

最终应输出：

- 三种文档视图的角色与切换合同；
- 选择模型和稳定身份的传播图；
- Inspector 对每类选择显示与可编辑的字段矩阵；
- 单字段编辑、多字段 Apply、取消、撤销、保存和外部冲突流程；
- Inspector、Asset Browser、Characters、Scenes、Problems 和 Preview 的信息分工；
- 键盘、焦点、滚动、最小窗口与长内容验收场景；
- 需要新增的 Editor-only 能力和任何跨 Loader/Core/Runtime 的接口提案；
- 真实 UI 验收矩阵，而不是只提供静态 mockup。

## 9. 明确禁止的捷径

- 不为 Block View 或 Inspector 建立第二份序列化正文。
- 不在不能安全理解源码时整体重写文件。
- 不让未知语法在可视化模式中消失。
- 不用多窗口、模态页或假文档标签解决简单属性编辑。
- 不用通知替代应长期可见的 dirty、只读或错误状态。
- 不为了通用性先搭属性插件框架。
- 不为了视觉丰富添加大卡片、重复标题、常驻说明、长动画或与任务无关的装饰。
