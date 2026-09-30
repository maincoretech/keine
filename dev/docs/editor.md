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
│   ├── blocks/             view：卡片布局；picker：插入面板
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
源码层只返回修改结果；应用修改、冲突保护与撤销仍由 `app/edits.rs` 负责。
中文等 Unicode 标识符沿用 Loader 的字母/数字规则，编辑器不另设 ASCII 限制。
Text/Blocks 共用概览导航状态和边界计算；各自仅提供原生行布局与滚动位置。
概览缓存、选中态、配色与源码更新路径保持独立，拖动概览不修改源码或选中 Block。

## 交互

- 卡片背景与圆角由外框绘制，标签滚动层保持透明；Dock 内容与 drop overlay 共用定位容器。
- 淡色行内补全用右方向键/Tab 接受；回车保留缩进，智能处理引号/括号。
- 空 Text Block 用 Delete/Backspace 删除；关键帧使用紧凑单行，嵌套结构保持层级。
- 行内 `[wait=1000]` 在原位置显示 Wait 标签，选中直接编辑毫秒，不切回整行原始语法。
- Block 类型标识着色；卡片不增加轮廓、左侧树形线或多余上下移动按钮。
- 拖动预览与占位冻结实际卡片尺寸，关键帧保留紧凑宽度；结构及多选包含子行高度，拖出区域取消不改源码。
- 占位只显示在源码允许的同层、同作用域落点；前后槽独立收起，槽内预览不参与行高计算，概览与主布局使用同一动画高度。
- 拖放松手后，卡片及邻近行用 200 ms 缓动落入新顺序；减少动态效果时立即完成，源码重排仍是一次可撤销修改。
- 每句旁白/对白右侧有结束开关：灰色保留文本框，蓝色在本句结束后隐藏；关联的 text.box 指令收进该开关，独立命令仍单独显示。
- 同命令 Block 多选可批量编辑共同的普通字段；不同命令/资源身份不混改，非法值整组拒绝。
- Text/Blocks 概览显示语法/类型颜色、视口与选中位置，支持点击、拖动和独立滚动。
- Inspector 分开 Position 锚点偏移、Transform 变换偏移和 Layout 分组；组内字段与批量修改保留未改参数。镜头补间选择 ◆ 随时长变化 / ◇ 立即应用。
- Block 资源下拉框按内容宽度靠左，最长 320 px；参数紧跟资源，Inspector 保持字段宽度。
- 下拉框使用统一无描边触发器与淡入淡出 popup，支持搜索和键盘选择；减少动态效果时立即切换。
- Search 按文件显示高亮结果，Up/Down 选择、Enter/点击跳到准确源码或 Block。
  后台搜索有取消、120 ms 输入合并、2,000 条结果上限；会提示截断与跳过文件。

## 资源

`assets.yaml` 是资源 ID、类型、路径与 tags 的唯一来源；派生缩略图与使用次数不写回清单。
```text
Assets
├── 图片：默认完整构图缩略图网格；音频/视频：默认紧凑列表，音频行内试听
├── 搜索：ID / 文件名 / 路径 / Tags，空格分词 AND 与模糊匹配
├── 筛选菜单：Type / Folder / Tags / Used、Unused、Missing / 大小 / 修改时间 / Sort
├── View：Auto / List / Grid；缩略图 Compact / Large 两档，随面板宽度排列
├── 卡片：文件名、ID、引用数、大小与 Missing；音频有时长，不能确定时显示 —
├── Unmapped：只识别 .unmapped 保留文件，与未引用不同
├── Inspector：ID / Type / Tags；多选 Tags 增量增删，引用按文件聚合，可浏览全部位置
├── 改名：确认引用影响，默认物理文件随 ID 改名；Type 同时移到对应资源目录
├── 删除：Keep files 移除映射并加 .unmapped；Trash 使用系统废纸篓；引用留给诊断
│   └── Remap 恢复文件名/映射，严格检查冲突；文件与源码共用撤销/重做
├── 单击：右上 Preview + 右下 Inspector；保留多选、范围选择与 Asset → Block 拖放
└── 只挂载可见行及邻近行的缩略图，滚动占位保留完整内容高度
```

一个映射资源对应一个物理文件；资源重命名/移动与引用变更必须一起验证。
外部导入先完成受限复制，清单修改通过同一文档 owner，避免并行导入丢条目。
Asset Preview 的 X 关闭面板并记住布局，下一次实际选择资源可重新打开。
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
接口失败显示 Unavailable；Windows/Linux 暂未实现进程指标。Peak 仅为采样到的最高 RSS。
数据口径以 [Apple libproc](https://github.com/apple-oss-distributions/xnu/blob/main/libsyscall/wrappers/libproc/libproc.h)
与 [XNU task/rusage](https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/kern/bsd_kern.c) 为准。
独立 Engine 不提供帧传输：不保留 Publish rate、overwrite、Paused 或 profiler/Capture 功能。

系统废纸篓已实现 macOS；Windows/Linux 目前明确拒绝删除，不回退永久删除。
媒体时长只读取有界 Ogg Opus/WAV 头尾；其他格式显示未知，媒体规范化导入仍暂缓。
IME、多显示器、1× DPI、主观音频和实际 Preview FPS 见 [验收](testing.md)。
