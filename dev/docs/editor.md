# Editor

```text
工作台
├── 左栏
│   ├── Explorer：工程文件，记住展开状态；首次默认折叠
│   ├── Search：全文搜索，⌘/Ctrl+Shift+F
│   ├── Assets：映射资源 / Unmapped、搜索、类型/目录/标签过滤、List/Grid
│   └── Characters / Problems / Preview 管理
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

## 交互

- 卡片背景与圆角由外框绘制，标签滚动层保持透明；Dock 内容与 drop overlay 共用定位容器。
- 淡色行内补全用右方向键/Tab 接受；回车保留缩进，智能处理引号/括号。
- 空 Text Block 用 Delete/Backspace 删除；关键帧使用紧凑单行，嵌套结构保持层级。
- 行内 `[wait=1000]` 在原位置显示 Wait 标签，选中直接编辑毫秒，不切回整行原始语法。
- Block 类型标识着色；卡片不增加轮廓、左侧树形线或多余上下移动按钮。
- Text/Blocks 概览显示语法/类型颜色、视口与选中位置，支持点击、拖动和独立滚动。
- Inspector 修改对应字段；镜头补间选择 ◆ 随时长变化 / ◇ 立即应用。
- 下拉框使用统一无描边触发器与淡入淡出 popup，支持搜索和键盘选择；减少动态效果时立即切换。
- Search 按文件显示高亮结果，Up/Down 选择、Enter/点击跳到准确源码或 Block。
  后台搜索有取消、120 ms 输入合并、2,000 条结果上限；会提示截断与跳过文件。

## 资源

`assets.yaml` 是资源 ID、类型、路径与 tags 的唯一来源；派生缩略图与使用次数不写回清单。
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

尚未完成：映射资源删除/恢复/remap 的完整产品流程、媒体规范化导入。
IME、多显示器、1× DPI、主观音频和实际 Preview FPS 见 [验收](testing.md)。
