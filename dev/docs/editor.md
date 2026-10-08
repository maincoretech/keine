# Editor

```text
工作台
├── 左栏
│   ├── Explorer：工程文件，记住展开状态；首次默认折叠
│   ├── Assets：映射资源 / Unmapped、搜索、类型/目录/标签过滤、List/Grid
│   ├── Characters
│   ├── Search：全文搜索，⌘/Ctrl+Shift+F；位于角色下方、分隔线上方
│   └── 分隔线 → Inspector / Problems / Performance / Build
├── 中央文档
│   ├── Text：补全、语法着色、诊断、智能缩进/配对、右侧概览
│   └── Blocks：同一份源码的结构投影、拖动、行内编辑、右侧概览
├── 右栏：可关闭的 Asset Preview + Inspector
└── Output：操作、解析、Preview 和加载错误
```

## 单一源码

Explorer 同目录的剧本默认按场景排列：入口场景所在文件优先，其余按 `scene` 名称自然排序（2 在 10 前）；多场景文件取最早场景。目录和其他文件的位置保持原样，显示顺序不改变剧情跳转。

Text、Blocks、Inspector 共用源文档。Block 编辑只替换准确 source range，
保留周围空白、注释和未知语法；不通过重新序列化整个工程写回。
无法理解的语法仍可见且只读。兼容 JSON 保持只读；转换必须显式 `migrate`。
未保存文档覆盖磁盘内容参与索引、搜索和 Preview。关闭/保存冲突走现有保护与恢复流程。
关闭确认中的 Save and Close 保存成功后退出，Cancel 保留窗口和修改；Close Without Saving
不写项目源文件并关闭，恢复草稿仍尝试保留，草稿写入失败记录错误但不取消关闭。
定位 Block 重建持久舞台、BGM 和循环音效；历史单次音效/语音不重播，只播放选中 Block 的音频。相同 BGM 保持播放，显式选择 BGM Block 才重新播放。
保存 `.shou` 时自动使用两空格缩进，长调用按 100 列拆分参数；字符串和注释原样保留。
保存保留 Text 选区和屏幕位置；格式化只映射源码位置，Blocks 复用已有行高与编辑框，不重新定位光标。
Text 行号旁悬浮箭头可折叠多行调用、数组与嵌套块；收起后保留首尾行，不修改源码。
搜索结果或从 Blocks 跳回隐藏行时自动展开对应范围。
输入法组词期间只显示原生候选文本，源码选择、Preview 定位和 Text 概览等提交后更新，不使用延迟判断。Blocks 的 Enter 在当前对白后插入 Text，Shift+Enter 在同一块内换行；草稿首次写回后仍保留焦点与相邻块范围。正常关闭项目保存每个文档的 Text/Blocks 模式和编辑位置，重开定位到原光标或 Block。
Text、Blocks 及关闭窗口时的保存共用此流程，格式化可撤销；未闭合或有词法错误的源码保持原样保存。
关闭最后一个源码页后，再开页优先复用存活文档组或 Output 所在组；无可用组才新建，避免落进 Explorer 窄栏。

```text
crates/editor/src/
├── app.rs                  启动、主题、共享控件
├── app/
│   ├── documents.rs        共享文档状态、派生索引、选择与面板登记
│   ├── panel.rs            面板标识、载荷与 Dock 生命周期
│   ├── panel/construction.rs 构造与输入事件订阅
│   ├── render.rs           面板分派、焦点与共享弹层
│   ├── document/           文档视图状态、Text/Blocks/Scenes 组合
│   ├── files/              Explorer 状态与文件树视图
│   ├── window.rs           工程窗口、保存/关闭保护、Engine 生命周期
│   ├── edits.rs            源码事务与撤销/重做
│   ├── controls.rs         共享控件与滚动容器
│   ├── blocks.rs           Block 交互与源码范围编辑
│   ├── blocks/             drag：剧本拖放；motion：重排映射；view：布局；picker：插入及偏好拖放
│   ├── reorder.rs          Block/列表共用的位移插值与边缘滚动
│   ├── inspector.rs        字段编辑与源码同步
│   ├── inspector/          state：面板状态；controls：字段控件；view：组合；spatial：位置与时间轴
│   ├── resource.rs         资源清单事务、筛选弹层与预览
│   ├── resource/state.rs   搜索、筛选、选择锚点与缩略图缓存
│   ├── resource/toolbar.rs 查询、筛选标签、多选操作与原生导入
│   ├── resource/browse.rs  响应式网格/列表、可见资源窗口
│   ├── resource/picker.rs  资源选择弹层
│   ├── minimap.rs          共用视口/拖动/平移计算与 Block 概览
│   └── text/minimap.rs     Text 换行映射、语法缩略图与滚动适配
├── authoring.rs            公共入口与标识符规则
└── authoring/
    ├── commands.rs        命令目录、分类、插入与补全模板
    ├── fields.rs          Loader 参数清单的消费、标签、候选值、数值与资源规则
    ├── index.rs           工程资源、引用、场景与对白索引
    ├── edit.rs            场景、角色、对白等纯源码修改
    ├── projection.rs      结构模型、字段范围与只读查询
    └── projection/
        ├── parse.rs       基于 Loader token 的结构识别
        └── edit.rs        Block 的有界源码修改
```

面板持有交互状态，源文档仍由 `EditorDocuments` 统一管理；投影与索引均可从源码重建。
补全、Block、Inspector 共用 `commands` / `fields`，字段规则不依赖 GPUI 控件。
原生命令及舞台事件的合法字段由 Loader 的各命令族 signature/字段表定义，
通过 `native_command_argument_names` 供 Inspector 与补全使用；新增参数只改对应命令族。
音效统一使用 `se(asset, id: ..., loop: true, volume: ..., fade: ...)`；Inspector/补全提供 Loop、Fade 和 Fade out 字段；旧 `se.loop` 不再解析，现有剧本须改为统一写法。
显示顺序保留，标签、候选值、分组提示与特殊控件仍归 Editor。
`style` 及对白尾部选项也复用 Loader 的字段表。`frame` 等子行通过
`native_child_command_argument_names(parent, child)` 按父命令查询；Inspector 与补全
共用序列 fps/逐帧时长的提示过滤，不混用序列帧和关键帧字段。
文本的 speaker/voice/stable ID 查询共用投影元数据；语音和尾部选项范围共用基于 Loader token
的查询，由元数据、字段编辑及写回共同消费。多选摘要读取完整源码范围，
Voice 修改保留尾部注释和选项；return/break 仍按 Loader 规则作为控制语句。
插入模板覆盖全部 70 个入口的解析与可编辑 Block 回归；sprite.update 模板只换图，不隐式重设位置或缩放。
立绘、移动、隐藏及聚焦规则模板优先共用已有角色 ID，使聚焦能够自动跟随对白。
源码层只返回修改结果；应用修改、冲突保护与撤销仍由 `app/edits.rs` 负责。
中文等 Unicode 标识符沿用 Loader 的字母/数字规则，编辑器不另设 ASCII 限制。
Text/Blocks 共用概览导航状态和边界计算；各自仅提供原生行布局与滚动位置。
概览缓存、选中态、配色与源码更新路径保持独立，拖动概览不修改源码或选中 Block。
Blocks 的源码、行索引、顺序和静态几何按版本缓存；普通滚动二分查找可见行，源码、折叠或实测行高改变时重建。拖放/折叠动画仍使用原有冻结几何路径。
Text 概览的配色、换行与笔画在后台生成；改宽复用配色，新任务取消旧任务，发布前复核版本，无固定输入延迟。换行调用与原生 DisplayMap 相同的 GPUI LineWrapper。

面板专属状态由 `document/files/inspector/resource/characters/blocks/picker` 各模块拥有；
共享文档、源码事务、文件事务及派生索引仍使用原有 owner，渲染只准备视图与控件。
文件/场景名称的 Enter 在输入订阅里延迟提交，不在 render 中执行文件或源码修改。
共享运动、测量展开、边缘滚动与弹层关闭规则仍归 `controls/reorder`，不建立跨 GPUI/Bevy 的 UI 框架。

## 交互

展开/收起共用 160 ms 可反向过渡：箭头旋转、内容淡入及高度变化使用同一进度；
资源统计卡和筛选分组使用 GPUI 已有的 `Collapsible` / `MotionReveal` 测量真实内容高度，
窄栏换行不依赖猜测高度。文件树与搜索保留按可见范围挂载，使用动画行高的前缀位置计算滚动。

| 交互范围 | 处理 |
|---|---|
| 资源统计卡、筛选分组、Explorer、搜索分组 | 共用展开/收起过渡，快速反向点击连续；搜索键盘跳过折叠结果 |
| Block 右键菜单 | 120 ms 淡入 / 90 ms 淡出；关闭期间不重复执行，旧关闭任务不移除新菜单 |
| 文件/场景右键菜单、资源选择、Block 插入 | 保留原开关过渡，统一遵循减少动态效果 |
| Build 进度 | 旋转图标加状态文字；减少动态效果使用静态图标 |
| Dock、Block 章节折叠/拖放 | 沿用现有过渡 |
| 引擎菜单、存读档切换/翻页、Backlog、确认框 | 已有动画；确认操作及关闭结果即时执行 |
| 源码输入、Inspector 写回、搜索查询、资源筛选结果 | 即时更新数据与选择位置，避免输入过程中反复淡入或移动焦点 |

系统减少动态效果由 GPUI 的 transition 统一处理；稳态不追加循环重绘任务。
代码与布局回归不代表 macOS/Windows/Linux 原生动效已经目测验收。

- Block 插入弹窗默认打开 Favorites，分类顺序为 Favorites → Text/Scene/Media/Flow/Data → All；普通分类仍可自定义排序。默认收藏按常用编剧操作排列，已有收藏保留，收藏内部可跨分类调整顺序。旧空收藏首次升级补入默认条目，之后主动清空会保持为空。
- 插入弹窗自定义模式用左侧拖动柄排序，沿用 Block 的预览卡、200 ms 位移和占位让位；边缘自动滚动。Favorites 可跨分类排序，普通分类只在分类内排序；搜索期间不提供拖动柄。松手一次保存 Editor 偏好，拖到外面、关闭或切换分类取消，点击条目不插入剧本。减少动态效果时直接到达目标位置。
- Block 插入弹窗打开时轻微上移并淡入，X、Esc、Tab 或选中条目后淡出；关闭期间不重复插入，快速重开不被旧关闭任务移除。系统减少动态效果时立即开关。
- 卡片背景与圆角由外框绘制，标签滚动层保持透明；Dock 内容与 drop overlay 共用定位容器。
- Preview 控件参与最右上方标题栏的排版，单标签页、多标签页与视图缩放共用，不覆盖文档工具按钮。
- 外侧 Dock 的保存宽度受当前工作区宽度约束；Editor 与 Preview 并排时，文档工具和 Preview 控件保持在窗口内，标签过多时滚动标签栏。
- 淡色行内补全用右方向键/Tab 接受；回车保留缩进，智能处理引号/括号。
- 不按输入法语言分支：组合文字确认后触发补全和语法检查，无固定等待；语法检查在后台执行，新修改取消旧任务，结果写回前复核源码和组合状态。
- 空 Text Block 用 Delete/Backspace 删除；关键帧使用紧凑单行，嵌套结构保持层级。
- Text Block 输入/退格只替换源码中的变化范围，保留输入实体、焦点、选中位置和行高；源码与恢复草稿持续同步，离开输入框、Enter 或保存时更新诊断、项目索引和 Preview，不逐字执行 Preview 或引入输入延迟。
- 行内 `[wait=1000]` 在原位置显示 Wait 标签，选中直接编辑毫秒，不切回整行原始语法。
- Block 类型标识着色；卡片不增加轮廓、左侧树形线或多余上下移动按钮。
- Block 右键菜单取得面板焦点，点击外部、Esc 或执行菜单项关闭；滚动不关闭菜单，菜单内鼠标与滚动不穿透到下方卡片。
- 序号在卡片中线垂直居中；Engine 当前执行卡片的蓝色底只从左边缘延伸至序号区域，内外圆角与卡片一致，序号用暗色粗体，停止后清除，普通选中不显示执行底色。
- 拖动预览与占位冻结实际卡片尺寸，关键帧保留紧凑宽度；结构及多选包含子行高度，拖出区域取消不改源码。
- 占位只显示在源码允许的同层、同作用域落点；前后槽独立收起，槽内预览不参与行高计算，概览与主布局使用同一动画高度。
- Block 拖放由单一生命周期管理：空闲 → 拖动 → 写回 → 收尾。开始时固定文档/版本、选择、卡片尺寸和布局；落点按固定几何判断，200 ms 位移预览不增大行距，结构携带子项。松手最终复核位置并只写回一次；Esc、面板外松手、失焦、模式/源码变化均结束拖动。边缘自动滚动，滚动后重新判断落点；减少动态效果时立即完成，源码重排仍是一次可撤销修改。
- 每句旁白/对白右侧有结束开关：灰色保留文本框，蓝色在本句结束后隐藏；关联的 text.box 指令收进该开关，独立命令仍单独显示。
- 同命令 Block 多选可批量编辑共同的普通字段；不同命令/资源身份不混改，非法值整组拒绝。
- Text/Blocks 概览显示语法/类型颜色、视口与选中位置，支持点击、拖动和独立滚动。
- Inspector 分开 Position 锚点偏移、Transform 变换偏移和 Layout 分组；组内字段与批量修改保留未改参数。镜头补间选择 ◆ 随时长变化 / ◇ 立即应用。
- 点击 Block 字段编辑只更新选中态与 Inspector，不触发 Preview 执行；显式 Replay 仍可执行选中指令。
- Block 资源下拉框按内容宽度靠左，最长 320 px；参数紧跟资源，Inspector 保持字段宽度。
- 极窄窗口由用户拖动分隔线或关闭一列 View 腾出文档空间；不为三列同时保留的极限布局裁剪类型标识或增加特殊适配。
- 下拉框使用统一无描边触发器与淡入淡出 popup，支持搜索和键盘选择；减少动态效果时立即切换。
- Search 按文件显示高亮结果，Up/Down 选择、Enter/点击跳到准确源码或 Block。
  后台搜索有取消、120 ms 输入合并、2,000 条结果上限；会提示截断与跳过文件。

## Build

侧栏 Package 图标打开 Build view；点击 `Export game…` 选择父目录，生成新的
`<工程名>-playtest` 文件夹（重名追加数字），完成后提供 `Play` / `Open folder`。
导出前复用现有保存、格式化和冲突保护；复制在后台运行，导出期间禁止其他文件操作。
当前支持原生 Eiyashou 工程的本机试玩：复用已安装的开发 Engine，无需编译或 publisher key；
复制登记资源、全部 `.shou` 和配置/清单，不复制原格式副本、未登记素材、存档、缓存或私钥。
源工程和导出副本分别校验，失败清理本次新目录；工程的 scene ID、引用和布局保持不变。
macOS 输出 `Game.app`，Windows/Linux 输出 `keine.exe` / `keine`；均可直接启动。
`project.icon` 可指定工程内 PNG/WebP；导出时校验并保留图标，macOS/Linux 自动生成对应应用图标。
Windows 临时导出复用 Engine 的 EXE 文件图标，窗口使用项目图标；正式发行通过 `bundle` 嵌入，见 [发布](release.md#应用图标)。
试玩副本包含可读源码，只针对当前操作系统；正式 Hakutaku 发行仍使用 [bundle](release.md)。

## 资源

`assets.yaml` 是资源 ID、类型、路径与 tags 的唯一来源；派生缩略图与使用次数不写回清单。
```text
Assets
├── 图片：默认完整构图缩略图网格；音频/视频：默认紧凑列表，音频行内播放/暂停
├── 搜索：ID / 文件名 / 路径 / Tags，空格分词 AND 与模糊匹配
├── 筛选菜单：Type / Folder / Tags / Used、Unused、Missing / 大小 / 修改时间 / Sort
├── View：Auto / List / Grid；缩略图 Compact / Large 两档，随面板宽度排列
├── 卡片：文件名、ID、引用数、Resource / 实际格式、大小与 Missing；音频有时长，不能确定时显示 —
├── Unmapped：只识别 .unmapped 保留文件，与未引用不同
├── Inspector：ID / Type / Tags；多选 Tags 增量增删，引用按文件聚合，可浏览全部位置
├── 改名：确认引用影响，默认物理文件随 ID 改名；Type 同时移到对应资源目录
├── 删除：Keep files 移除映射并加 .unmapped；Trash 使用系统废纸篓；引用留给诊断
│   └── Remap 恢复文件名/映射，严格检查冲突；文件与源码共用撤销/重做
├── 单击：右上 Preview + 右下 Inspector；保留多选、范围选择与 Asset → Block 拖放，图标与 ID/数量随鼠标显示
│   └── 插入复用 Block 的占位撑开、平滑重排及取消回位；兼容命令中央替换资源，Voice 附加到对白
└── 只挂载可见行及邻近行；后台生成小尺寸缩略图，64 项内存缓存，滚动占位保留完整内容高度
```

一个映射资源对应一个物理文件；资源重命名/移动与引用变更必须一起验证。
拖入资源目录时立即转换并登记，成品保留原文件主名，只改扩展名；已有合规文件直接复制。
PNG/JPEG/BMP/TIFF/静态 GIF：背景转为 Q80 WebP，立绘/粒子保留无损 WebP；
保留原分辨率、透明度和 ICC 色彩配置，应用图片方向；已有合规 WebP 直接复制，不反复有损编码。
WAV/MP3/FLAC/Vorbis/AAC 转为 48 kHz、192 kb/s VBR Opus；视频转为 H.264 MP4。
动态 GIF/APNG 明确拒绝，不静默丢帧。音视频转换需要带 libopus/libx264 的 FFmpeg，
从 Editor 旁、Resources、PATH 或 macOS Homebrew 目录查找；缺少工具时显示错误。
原文件只读，成品放在选中的工程资源目录；工程内原地导入则生成同目录的新格式文件。
重名不覆盖；转换、校验和清单提交失败清理半成品。文件与映射共用一次撤销/重做，原文件保留。
清单修改通过同一文档 owner，避免并行导入丢条目；不在播放时转换，存量素材仅在显式点击 Convert all 后转换。
Assets 顶部可展开图标统计卡显示登记文件体积、规范格式/待转换、未引用、缺失文件、未定义引用及 Unmapped 数量；点击状态可筛选。
Assets 搜索下显示结果数/总数及多选数；工具栏提供导入、选择全部结果、清除选择和删除确认。
类型/目录/标签/状态/体积/修改时间/Unmapped 筛选以可移除标签呈现；清除筛选保留排序、List/Grid 和缩略图大小。
空工程与无匹配结果分别提示；窄面板的工具栏和标签换行，不覆盖列表。
统计状态切换清除其他筛选；Undefined 打开 Problems，Unmapped 显示未登记文件。
导入使用当前目录筛选，或请求工程内的资源分类目录，沿用既有转换、进度、清单更新和文件撤销流程；
不会把工程外目录作为目标，批量任务进行中禁用重复导入/删除。跨筛选的已选资源仍保留，可显式清除。

Convert all 在后台逐个转换待转换资源，在同目录写新文件，原文件只读保留；保留 ID/标签并同步所有同文件别名，碰到同名输出自动加短后缀、不覆盖。按完成文件数显示进度条（收起统计卡仍可见），使用已有错误反馈和撤销；清单有未保存修改时要求先保存。
原生发布仅在临时副本排除未登记的兼容格式原稿及 `.unmapped`；登记的 PNG/WAV 等仍按现有发行规则拒绝。
规范格式通过有界文件头检查 WebP、Ogg Opus、H.264 MP4，不以扩展名或登记记录冒充转换历史。
预计打包资源大小按当前登记文件去重求和，包含未引用资源；未转换文件按当前体积计，转换后更新。缺失/无法读取文件使估算不完整；不含引擎、脚本、索引和封装开销。
资源卡用 GPUI 状态图标标明规范格式/待转换及未引用，格式/体积保持短文本；悬浮提示解释状态，Explorer 用 `U` 标记未引用，不作为错误。
未定义或缺失的资源引用在章节 Text 内用波浪线和诊断提示标出，Blocks 内显示错误图标；Explorer 文件和父目录显示红色错误数。确认输入后更新，IME 组合期间不扫描。
Assets 按索引与筛选/布局参数缓存排序结果、目录和行布局；修改时间筛选仍实时计算。列表压紧文字行距、增加卡片内边距，保持原行高。
缩略图解码最多同时两项、以 GPUI 低优先级运行；静态 WebP 复用 media 的有界缩放解码，保留透明度和 EXIF 方向，PNG 等仍走原解码路径，不生成磁盘缓存。
脚本索引更新只复制未替换的贡献；新编辑/关闭工程取消旧计算，文件之间与发布前检查取消状态。公共索引模型不变。
Asset Preview 的 X 关闭面板并记住布局，下一次实际选择资源可重新打开。音频在同一预览区域提供播放、暂停/继续与从头重播；暂停保留播放位置，换音频只保留一个试听源。
文件/目录右键使用平台无关的“在文件管理器打开”。

## Engine

每个物理工程一个工作台与一个 Engine child。右上角图标控制独立原生窗口，
不传输内嵌画面。源码 revision、cursor 与退出由有界协议协调；不匹配的协议/能力明确报错。
Engine 发现支持 sibling app、同目录、Resources、PATH 与 `KEINE_ENGINE`。
Editor 只启动预构建 Engine，不在作者交互中运行 Cargo。

## Performance

```text
Performance（singleton；打开/关闭/移动只影响自身）
├── Engine：Off / Starting / Running / Failed，30 秒生命周期带
├── CPU：仅 Preview child；100% = 1 logical core，允许超过 100%
├── Memory：当前 RSS、该次子进程的 observed peak
└── CPU / RSS：30 秒小折线；缺测留空，不作故障判断
```

现有 Preview worker 约 500 ms 采样，CPU 用累计 CPU 时间差 / 实际 elapsed。
历史归属工程的 PreviewController：最多 61 个样本、64 个状态转换，按 30 秒过期，
生命周期保留窗口前的一个状态锚点。关闭 View 仍采样；重启保留历史但清空 peak、
CPU baseline 并分开折线。历史只在内存，工程/Editor session 结束后丢弃。

macOS 复用锁定的 `libc`，通过 `proc_pid_rusage(RUSAGE_INFO_V2)` 查询 owned child；
`ri_user_time + ri_system_time` 由 Mach timebase 转换为 CPU 时间，`ri_resident_size` 为 RSS。
Linux 从有界 `/proc/<pid>/stat` 读取 user/system ticks 与 RSS 页数，按 sysconf 单位换算；
Windows 用 query-only 句柄读取 GetProcessTimes 与 working set，查询后关闭句柄。
退出/查询失败显示 Unavailable；Peak 仅为采样到的最高常驻内存。
平台单位依据 [Linux proc stat](https://man7.org/linux/man-pages/man5/proc_pid_stat.5.html)
和 [Windows process times](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocesstimes)。
数据口径以 [Apple libproc](https://github.com/apple-oss-distributions/xnu/blob/main/libsyscall/wrappers/libproc/libproc.h)
与 [XNU task/rusage](https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/kern/bsd_kern.c) 为准。
独立 Engine 不提供帧传输：不保留 Publish rate、overwrite、Paused 或 profiler/Capture 功能。

系统废纸篓支持 macOS、Linux 和 Windows；不回退永久删除。Linux 使用 freedesktop
files/info 恢复记录与同卷私人 Trash（外部卷 `.Trash-uid`），无法安全创建时拒绝移动；
Windows 使用 Shell 回收站 namespace 项恢复。撤销不覆盖同名新文件，重做重新入篓。
格式/API 依据 [freedesktop Trash](https://specifications.freedesktop.org/trash/latest/)
和 [Shell PostDeleteItem](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ifileoperationprogresssink-postdeleteitem)。
媒体时长只读取有界 Ogg Opus/WAV 头尾；其他格式显示未知。
IME、多显示器、1× DPI、主观音频和实际 Preview FPS 见 [验收](testing.md)。

资源阻塞加载时，Engine 右下角显示 24 px 圆环，每三秒旋转一周；加载完成隐藏，不再显示 Loading 文案。
