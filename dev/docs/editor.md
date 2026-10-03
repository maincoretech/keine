# Editor

```text
工作台
├── 左栏
│   ├── Explorer：工程文件，记住展开状态；首次默认折叠
│   ├── Assets：映射资源 / Unmapped、搜索、类型/目录/标签过滤、List/Grid
│   ├── Characters
│   ├── Search：全文搜索，⌘/Ctrl+Shift+F；位于角色下方、分隔线上方
│   └── 分隔线 → Problems / Performance
├── 中央文档
│   ├── Text：补全、语法着色、诊断、智能缩进/配对、右侧概览
│   └── Blocks：同一份源码的结构投影、拖动、行内编辑、右侧概览
├── 右栏：可关闭的 Asset Preview + Inspector
└── Output：操作、解析、Preview 和加载错误
```

## 单一源码

Text、Blocks、Inspector 共用源文档。Block 编辑只替换准确 source range，
保留周围空白、注释和未知语法；不通过重新序列化整个工程写回。
无法理解的语法仍可见且只读。兼容 JSON 保持只读；转换必须显式 `migrate`。
未保存文档覆盖磁盘内容参与索引、搜索和 Preview。关闭/保存冲突走现有保护与恢复流程。
定位 Block 重建持久舞台、BGM 和循环音效；历史单次音效/语音不重播，只播放选中 Block 的音频。相同 BGM 保持播放，显式选择 BGM Block 才重新播放。
保存 `.shou` 时自动使用两空格缩进，长调用按 100 列拆分参数；字符串和注释原样保留。
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
│   ├── panel.rs            面板状态、构造与 Dock 注册
│   ├── window.rs           工程窗口、保存/关闭保护、Engine 生命周期
│   ├── edits.rs            源码事务与撤销/重做
│   ├── controls.rs         共享控件与滚动容器
│   ├── blocks.rs           Block 交互与源码范围编辑
│   ├── blocks/             drag：拖放生命周期；motion：重排映射；view：布局；picker：插入
│   ├── inspector.rs        字段编辑与源码同步
│   ├── inspector/          controls：字段控件；view：面板组合；spatial：位置与时间轴
│   ├── resource.rs         资源过滤、预览与角色新增
│   ├── resource/browse.rs  响应式网格/列表、可见资源窗口
│   ├── resource/picker.rs  资源选择弹层
│   ├── minimap.rs          共用视口/拖动/平移计算与 Block 概览
│   └── text/minimap.rs     Text 换行映射、语法缩略图与滚动适配
├── authoring.rs            公共入口与标识符规则
└── authoring/
    ├── commands.rs        命令目录、分类、插入与补全模板
    ├── fields.rs          参数清单、标签、候选值、数值与资源规则
    ├── index.rs           工程资源、引用、场景与对白索引
    ├── edit.rs            场景、角色、对白等纯源码修改
    ├── projection.rs      结构模型、字段范围与只读查询
    └── projection/
        ├── parse.rs       基于 Loader token 的结构识别
        └── edit.rs        Block 的有界源码修改
```

面板持有交互状态，源文档仍由 `EditorDocuments` 统一管理；投影与索引均可从源码重建。
补全、Block、Inspector 共用 `commands` / `fields`，字段规则不依赖 GPUI 控件。
插入模板覆盖全部 71 个入口的解析与可编辑 Block 回归；sprite.update 模板只换图，不隐式重设位置或缩放。
立绘、移动、隐藏及聚焦规则模板优先共用已有角色 ID，使聚焦能够自动跟随对白。
源码层只返回修改结果；应用修改、冲突保护与撤销仍由 `app/edits.rs` 负责。
中文等 Unicode 标识符沿用 Loader 的字母/数字规则，编辑器不另设 ASCII 限制。
Text/Blocks 共用概览导航状态和边界计算；各自仅提供原生行布局与滚动位置。
概览缓存、选中态、配色与源码更新路径保持独立，拖动概览不修改源码或选中 Block。
Blocks 的源码、行索引、顺序和静态几何按版本缓存；普通滚动二分查找可见行，源码、折叠或实测行高改变时重建。拖放/折叠动画仍使用原有冻结几何路径。
Text 概览的配色、换行与笔画在后台生成；改宽复用配色，新任务取消旧任务，发布前复核版本，无固定输入延迟。换行调用与原生 DisplayMap 相同的 GPUI LineWrapper。

## 交互

- 卡片背景与圆角由外框绘制，标签滚动层保持透明；Dock 内容与 drop overlay 共用定位容器。
- Preview 控件参与最右上方标题栏的排版，单标签页、多标签页与视图缩放共用，不覆盖文档工具按钮。
- 淡色行内补全用右方向键/Tab 接受；回车保留缩进，智能处理引号/括号。
- 不按输入法语言分支：组合文字确认后触发补全和语法检查，无固定等待；语法检查在后台执行，新修改取消旧任务，结果写回前复核源码和组合状态。
- 空 Text Block 用 Delete/Backspace 删除；关键帧使用紧凑单行，嵌套结构保持层级。
- 行内 `[wait=1000]` 在原位置显示 Wait 标签，选中直接编辑毫秒，不切回整行原始语法。
- Block 类型标识着色；卡片不增加轮廓、左侧树形线或多余上下移动按钮。
- 序号在卡片中线垂直居中；Engine 当前执行卡片的蓝色底只从左边缘延伸至序号区域，内外圆角与卡片一致，序号用暗色粗体，停止后清除，普通选中不显示执行底色。
- 拖动预览与占位冻结实际卡片尺寸，关键帧保留紧凑宽度；结构及多选包含子行高度，拖出区域取消不改源码。
- 占位只显示在源码允许的同层、同作用域落点；前后槽独立收起，槽内预览不参与行高计算，概览与主布局使用同一动画高度。
- Block 拖放由单一生命周期管理：空闲 → 拖动 → 写回 → 收尾。开始时固定文档/版本、选择、卡片尺寸和布局；落点按固定几何判断，200 ms 位移预览不增大行距，结构携带子项。松手最终复核位置并只写回一次；Esc、面板外松手、失焦、模式/源码变化均结束拖动。边缘自动滚动，滚动后重新判断落点；减少动态效果时立即完成，源码重排仍是一次可撤销修改。
- 每句旁白/对白右侧有结束开关：灰色保留文本框，蓝色在本句结束后隐藏；关联的 text.box 指令收进该开关，独立命令仍单独显示。
- 同命令 Block 多选可批量编辑共同的普通字段；不同命令/资源身份不混改，非法值整组拒绝。
- Text/Blocks 概览显示语法/类型颜色、视口与选中位置，支持点击、拖动和独立滚动。
- Inspector 分开 Position 锚点偏移、Transform 变换偏移和 Layout 分组；组内字段与批量修改保留未改参数。镜头补间选择 ◆ 随时长变化 / ◇ 立即应用。
- Block 资源下拉框按内容宽度靠左，最长 320 px；参数紧跟资源，Inspector 保持字段宽度。
- 极窄窗口由用户拖动分隔线或关闭一列 View 腾出文档空间；不为三列同时保留的极限布局裁剪类型标识或增加特殊适配。
- 下拉框使用统一无描边触发器与淡入淡出 popup，支持搜索和键盘选择；减少动态效果时立即切换。
- Search 按文件显示高亮结果，Up/Down 选择、Enter/点击跳到准确源码或 Block。
  后台搜索有取消、120 ms 输入合并、2,000 条结果上限；会提示截断与跳过文件。

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
├── 单击：右上 Preview + 右下 Inspector；保留多选、范围选择与 Asset → Block 拖放
└── 只挂载可见行及邻近行；后台生成小尺寸缩略图，64 项内存缓存，滚动占位保留完整内容高度
```

一个映射资源对应一个物理文件；资源重命名/移动与引用变更必须一起验证。
拖入资源目录时立即转换并登记，成品保留原文件主名，只改扩展名；已有合规文件直接复制。
PNG/JPEG/BMP/TIFF/静态 GIF 转为无损 RGBA8 WebP（保留透明度、应用图片方向）；
WAV/MP3/FLAC/Vorbis/AAC 转为 48 kHz、192 kb/s VBR Opus；视频转为 H.264 MP4。
动态 GIF/APNG 明确拒绝，不静默丢帧。音视频转换需要带 libopus/libx264 的 FFmpeg，
从 Editor 旁、Resources、PATH 或 macOS Homebrew 目录查找；缺少工具时显示错误。
原文件只读，成品放在选中的工程资源目录；工程内原地导入则生成同目录的新格式文件。
重名不覆盖；转换、校验和清单提交失败清理半成品。文件与映射共用一次撤销/重做，原文件保留。
清单修改通过同一文档 owner，避免并行导入丢条目；不在播放时转换，不批量改写存量素材。
Assets 的现有信息行以 `Resource · 实际格式` 标明已登记文件，悬浮显示 Registered resource；
导入通知显示已登记的资源数量。Missing/Unmapped 不显示已登记提示，格式标记不声称历史素材已转码。
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
