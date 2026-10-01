# 开发与验收

## 每次集成

从当前 checkout 的分支、worktree list、status、diff 开始。保留未提交修改。

```sh
cargo fmt --all --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo validate tests/fixtures/native-smoke
```

Feature 变更再检查相应 feature；发布者/媒体路径使用：

```sh
cargo check --features hot-reload,video-native,video-ffmpeg,publisher
```

## 测试布局

```text
tests/
├── authoring/process.rs       真实 Engine IPC / 生命周期 / 协议 / GPU Preview
├── coverage.rs               冻结兼容输入与 Stage 属性/事件覆盖
├── letsgal/sample.rs          本机官方示例资源与内容完整性
├── fixtures/
│   ├── native-smoke/          最小 tracked 原生工程
│   ├── letsgal-timeline/      舞台/镜头/退格/音频回归与 benchmark 场景
│   ├── webgal-showcase/       只作 parser / IR 冻结回归
│   └── video/                生成的合法与损坏视频
├── bench/                    core、loader、runtime、save、backup、video
├── fuzz/                     独立 nightly workspace，生产 WebP decoder
└── video/acceptance.rs        文件系统与加密 Hakutaku 视频验收
```

各 crate 的私有 unit tests 贴近 owner；Editor 的真实单实例进程回归位于
`crates/editor/tests/instance.rs`。不再保留只测试上游 Dock prototype 的 spike。
保留格式边界、路径隔离、事务/恢复、协议、执行和资源回归；不为了界面目测新增测试工程。

显式 Cargo `path` 注册嵌套 tests/benches，参见 [Cargo targets](https://doc.rust-lang.org/cargo/reference/cargo-targets.html)。

```sh
cargo bench -p keine-core
cargo bench -p keine-loader
cargo bench -p keine
cargo +nightly fuzz run --fuzz-dir tests/fuzz webp tests/fuzz/corpus/webp -- -max_total_time=60
cargo run --no-default-features --features publisher,video-native --bin keine-video-acceptance -- tests/fixtures/video/playback.mp4
cargo run --no-default-features --features publisher,video-native --bin keine-video-acceptance -- --expect-error tests/fixtures/video/damaged-header.mp4
bash dev/scripts/video-fixtures.sh
```

Linux 使用 `video-ffmpeg`；CI 另跑 FFmpeg ASan。fuzz 的目录参数由
[cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz/blob/main/src/options.rs) 提供。
视频 fixtures 只有生成的 pattern/tone：1 秒 H.264+AAC、无音轨、4 秒长 GOP、尾部 moov、截断头。
合法文件必须读到 EOF 并可 rewind，损坏头在 FS/加密包中均必须拒绝。

## 性能

性能修改必须提供同主机、同输入、同 release 配置的前后命令与原始结果。
`cargo perf --startup` 测隔离启动；`cargo bundle <project> --benchmark` 生成独立 portable 发行基准。
bench 源码保留可重复的热点测量，日志放忽略的 `target/`，不持续新增日期报告。

### 独立性能阶段（暂缓，功能收尾后启动）

当前不新增耗时性能跑分，也不据旧结果宣称当前性能通过。阶段包含 Editor 大文档/资源浏览/搜索、Engine 启动与持续 CPU/RSS、实际 Preview FPS/GPU 帧时、粒子热点、音视频流与 Hakutaku I/O；使用现有 bench 与 portable benchmark 入口，不建立另一套采集协议。

启动时固定同主机、同 release、同输入及分辨率；先记录基线，再定位与修改热点，最后保存前后原始命令/结果并复验功能。Windows 自动 benchmark 的构建/打包入口保留；平台跑分和性能结论留到此阶段。

功能阶段继续运行以下接口/正确性回归，不依赖帧率、耗时或机器性能阈值：

| 接口 | 当前保留的测试合同 |
|---|---|
| Editor → Engine | 协议版本、长度边界、启动握手、崩溃重开、不同工程隔离 |
| Performance 采样 | 实际采样间隔、CPU >100%、计数回退、缺测、PID 重用、peak 重置、有界历史 |
| 源码 → Parser → Block | 全部 70 个插入入口、EYS 旧入口拒绝、分组字段与精确写回、换图省略保留 |
| 资源与文件 | 改名/类型迁移、映射/文件共同撤销、冲突与路径隔离、导入失败回滚 |
| Editor 交互模型 | 拖放取消/过期版本/嵌套几何、概览两端和独立平移、失去文档容器后的开页落点 |
| Audio / Video / Media | 循环 rewind/Opus pre-skip、BGM sink 交接、视频 EOF/rewind/损坏输入、队列与解码预算 |
| 发行与 benchmark | 正常包与 benchmark 分离、挂载覆盖、确定性 payload、保留目录识别；不执行跑分 |

对应 owner：`tests/authoring/process.rs`、`crates/authoring`、`crates/editor`、`src/runtime/audio.rs`、`src/runtime/package/benchmark.rs`、`src/scene/video`、`crates/media`。现有 `tests/bench` 与 CI 正确性检查保持可编译，不因暂缓删除测试或接口。

粒子专项：

- 复用 letsgal-native 雪景和已有粒子输入，同设备、同 release、固定分辨率/粒子数，记录窗口与全屏的 CPU、GPU 帧时和帧率；单独区分加载/全屏切换的瞬时峰值与持续占用。
- 先定位模拟更新、网格上传、透明混合和其他合成 pass 的实际热点，再决定优化；现有批量绘制和固定 60 Hz 模拟不重复重写。
- 用相同输入保留优化前后原始结果，检查雪/雨/其他预设的速度、数量、淡入淡出与背景恢复行为。未测量前不归因、不降低默认效果质量。

最近已记录的 0.11.0 → 0.11.1 macOS arm64 / Rust 1.97.1、warm-cache 中位数：

| 操作 | 输入 | 前 ms | 后 ms |
|---|---|---:|---:|
| Block projection | 5,000 commands | 156.142 | 3.759 |
| Native parser | 5,000 commands | 87.422 | 2.476 |
| 完整索引 | 500 files / 25,000 lines | 40.521 | 35.754 |
| WorkspaceSession | 165 LetsGal entries | 1.032 | 0.702 |

原始 probe/paired logs：`target/authoring/editor-audit-20260929/0111/`。
普通 probe 为 11 samples；增量单文档索引 31 samples，中位数 2.341 ms。
小输入完整索引有变慢；这些数字不是 UI FPS、GPU、磁盘或长期内存结果，不声称全局改善。

Performance 进程采样的 release probe（macOS arm64，11×5,000 次）复用实际
`preview/performance/process.rs`：关闭采样中位数 0.0002 µs，启用系统查询 0.3869 µs/次。
Mach timebase 换算的 CPU 增量 0.299014 s，`getrusage` 对照 0.299035 s（ratio 0.9999）。
这只测本机系统查询，不代表完整 View/渲染开销；原始命令与结果位于
`target/authoring/performance/probe.rs`、`process-probe.txt`。
Focused tests 覆盖实际 elapsed / >100% / counter reset、有界过期、PID 重用时 peak 重置、
unavailable 缺测和 Mach 换算边界；不新增验收工程。

## 当前验收边界

```text
0.12.0 / EYS v2.0
├── macOS：原生 Engine、Text/Block/Inspector 写回、保存重开、故障恢复已有运行态证据
├── 最新工作台：下拉框、Text 概览、全文搜索、Asset Preview 关闭/重开已目测
├── 每句文本结束开关：灰/蓝状态、增删关联指令、Inspector 同步及 Cmd+Z 恢复已目测
├── Asset 生命周期：macOS 改名、Unmapped/Remap、系统 Trash、Type 迁移、撤销/重做与保存重开已目测
├── Performance：macOS 启停/失败恢复、关闭后持续采样、重开历史、重启 peak 重置已目测
├── UI 容器简化：多标签圆角、切换、面板缩放/滚动与跨分组拖放已用新 release 目测
├── 中文 IME：用户已通过本轮无固定延迟输入验收
├── Audio：macOS 两秒 Opus 循环与两次 BGM 交接已由用户实听通过
├── 单屏：2× 默认/最小/宽矮 Editor 窗口、Preview 最大化与全屏黑色留白已目测
├── 待显示验收：1× 与 Preview 极端比例；多显示器按用户要求延后
├── 独立性能阶段（暂缓）：实际 Preview FPS/GPU 帧时、持续 CPU/RSS、粒子与其他热点
├── Windows x64 / Linux：构建 CI 与运行态验收分别看待；运行态暂缓
└── 暂缓：媒体规范化、正式签名/notarization；Windows/Linux 系统废纸篓尚未实现
```

当前代码已通过 fmt、workspace check、clippy、Editor debug 构建与 native-smoke validate；默认 workspace tests 729 passed / 1 ignored，no-default-features + publisher workspace tests 763 passed / 1 ignored。无默认 feature 与 bundled-opus 的音频边界各 2 项及 video-native 视频 8 项回归通过。audio-opus 单独配置的 check 通过，测试链接因本机缺少系统 libopus 未完成；bundled-opus 覆盖相同的无 seekable 路径。组合 hot-reload/video-native/video-ffmpeg/publisher 的 check 通过。
测试覆盖源码/文件事务、macOS 废纸篓恢复、对白尾部参数解析/执行/迁移；这些不代替完整运行态验收。

功能约定核对：EYS 统一入口/旧写法拒绝、Position/Layout 分组、稀疏换图与 scale、逐立绘 light、对白尾部选项和行内 wait；工作台下拉框、Text/Block 概览、全文搜索、Preview 关闭、拖放、序号/执行标识、结束开关、输入检测；资源网格/音频列表、筛选/Tags、物理改名/类型移动、Unmapped/Remap/Trash；迁移裸 ID/objects 映射/particles 清单和原子镜头动作均有当前实现与功能回归。原生视觉证据与用户通过项以上表为准。
本轮补齐：sprite.update 插入模板省略位置/缩放，Loader 与 Block 投影不再把对白后的 return/break 吞作语音 ID。插入模板回归扩展为全部 70 个入口。
这两项新修复已换上新 Editor/Engine 原生复验：插入模板只生成目标与图片参数；对白后的 Return/Break 各自显示为 Block，实际执行能返回调用处、退出循环，换图后位置与缩放保持。原有未保存草稿保留，项目文件未改写。
无解码器配置的 Preview 曾因 AudioSource 未注册而 SIGABRT；现在由音频配置入口补齐类型注册，并保留已有音频资源。回归覆盖普通/图库播放及已有 registry 不被重建；无音频构建原生 Preview 已验证缺少 Opus 时不崩溃、对白仍能继续。Preview 失败长提示改为固定尺寸图标，详情留在 tooltip/Output；最小窗口已检查图标不遮挡 Text/Blocks 标签。
宽矮 Editor 的 Text/Blocks、概览、资源框和 Inspector 滚动已目测。按用户确认，极窄窗口通过调整分隔线或关闭一列 View 使用，不要求三列同时保留的极限布局适配；撤销专为此添加的类型标识收缩/裁剪，不再列为本轮待修复项。
音频复验复用临时环境工程，播放无首尾静音的两秒 Opus 纯音；用户确认循环无停顿/爆音、第一首→第二首→第一首的 1 秒淡入淡出均正常。停止指令后脚本继续到下一句。此结果只覆盖本机该输入，不推定所有设备/素材已通过。临时新增脚本/音频已移除；环境工程 53 个文件和 letsgal-native 95 个文件的路径与 SHA256 均恢复基线。
明确未完成的功能仍为 Windows/Linux 系统废纸篓与进程指标、媒体规范化导入；正式签名/notarization 属发行工作，跨平台/显示适配/主观音频属运行态验收。它们不计为本机核心功能已完成，也不混入性能阶段。

EYS v2.0 的配置版本为 2；默认值、显式旧版/未知版拒绝及迁移输出版本已通过回归。0.12.0 的 Editor/Engine 构建与版本输出、fmt/check/clippy、workspace tests、可选 feature 和 native-smoke/letsgal-native 校验通过。
统一入口的解析、执行、迁移及旧入口拒绝已通过回归；IR schema 为 v5。
Position/Layout 分组语法覆盖解析、旧平铺字段拒绝、嵌套补全、Inspector 精确范围/批量写回及迁移 Action 对等回归。
当前 publisher workspace tests、clippy、含 hot-reload/video-native/video-ffmpeg/publisher 的 check 与 debug 构建通过。
macOS 原生界面已检查 Position X、viewport Height 修改保存保留其他参数，以及淡色 viewport 补全用右方向键接受。
Block 拖放采用固定尺寸/布局快照、中央落点判断、200 ms 位移预览和一次写回。macOS 新构建已目测普通行重排、Option 携带 Goto、整个 Choice 移动、跨分支拒绝、面板外取消及一次撤销；连续操作后嵌套高度/间距未增长，试改已撤销。用户已通过本轮 Block 拖放验收。几何、取消、文档身份/版本、结构与重复行映射回归通过。
sprite.update 的省略保留、显式重设、缺席目标、动画目标与编译包 roundtrip 回归通过；迁移保留旧输入的重设行为，统一缩放输出 scale。无默认 feature 的 video-native 视频测试已有 8 项通过证据。本地沙箱阻断 IPC socket 时放行后重跑，通过才算完成。
现有 letsgal-native（10 scene / 1008 action）重新校验通过，仅一条未使用背景资源警告；环境光工程（65 scene / 1137 action）已有分组写法校验证据；native-smoke 无警告。
环境光的线性取样、明暗/色偏分离、透明像素、GPU 提取后缓存及重载边界已通过回归。
macOS debug 原生预览用只读复制的 tday 教室、海边夕阳、暴雨卧室目测；
用户通过夕阳和夜景，采用验收强度 1.0 为默认。临时工程位于
`/private/tmp/keine-environment-tday`，已只读复制全部 46 张背景并逐文件校验内容一致，
按章节提供目录及前后切换，先展示画面再打开导航；工程校验无警告。
用户反馈大部分画面正常并结束本轮验收；这不等于逐张无条件通过。
逐立绘 light 开关覆盖严格布尔解析、独立对象状态、恢复开启及存储回归；
原生 Inspector 的 Light 灰/蓝状态切换与保存写回已检查；旧作者字段 environment_light 被拒绝。
同背景双立绘的开启/关闭对照已检查。
no-default-features + video-native 测试也通过。
临时工程未加入仓库，原背景未修改。
这些是本机画面证据，不代表物理光照、全素材效果或 Windows/Linux 运行态验收。

遮幕/遮罩以计时完成解除等待，不能因透明度提前舍入到目标值而停止更新。2.2 秒 / 60 FPS 的脚本续行与已到目标透明度的等待解除回归通过（默认 feature 和 no-default-features + video-native）；fmt、workspace check/clippy/tests 通过。macOS 新 Engine 在 letsgal-native 的铁路雪景已检查开幕后进入对白，以及自动播放后续对白、收幕并返回标题；未重跑迁移。

引擎窗口底色显式设为黑色。macOS 新 Engine 全屏雪景已目测上下留白为黑色，舞台和对白正常；fmt、workspace check/clippy/tests 与 debug 构建通过。

macOS 新 Editor 在现有 letsgal-native 验收 Keep files → Unmapped → Remap、映射/未映射资源的系统 Trash、每种操作的撤销/重做；解绑和 Background → Figure 物理迁移均保存并关闭重开验证。PNG → Voice 被格式检查拒绝；改变类型后不兼容引用进入 Problems，撤销后消失。文件内容始终保持同一 SHA256；验收后 demo 的全部 95 个文件路径和内容恢复至基线。
关闭所有源码页后，资源撤销重新开页曾落入左侧窄栏；现在按存活文档组、Output 组、新文档组依次选择。落点只读取已登记的面板 ID 与布局，避免资源回调内重复借用正在更新的面板。macOS 新构建已验证无源码页时解绑开页，以及保存/关闭后撤销、再次关闭后重做，均落在中央且文件可恢复。容器消失及重新插入回归、fmt、workspace check/clippy/tests（727 passed、1 ignored）与 Editor debug 构建通过；IPC 测试放行沙箱后重跑。

视觉验收直接操作当前构建的原生应用；不得为采证新增 screenshot hook、环境变量协议或自动化。
输入检测由确认文字的 Change 事件触发，取消补全 120 ms、语法检查 180 ms 与派生索引 60 ms 固定等待；后台任务保留取消和过期结果保护，语法诊断写回前检查组合状态。fmt、workspace check/clippy/tests（727 passed、1 ignored）和 Editor debug 构建通过；macOS 新构建已目测淡色补全、右方向键接受、错误提示及撤销恢复。用户按本轮验证步骤反馈通过，中文 IME 组合/候选及确认后检测记为本机验收通过；不扩展为所有输入法或其他平台通过。
构建、测试、包签名和 UI/音视频验收必须分别报告。历史一次通过不替代当前代码复验。
