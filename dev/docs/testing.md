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

官方 LetsGal 示例验收和 loader benchmark 通过 `KEINE_LETSGAL_PROJECT` 指定外部原工程；
不依赖本地 demo。`projects/tday` 是唯一忽略的开发 demo，CI 打包默认使用 tracked native-smoke。

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

### Editor 滚动热点

本轮针对 tday 的 Assets 掉帧启动 Editor 性能检查；粒子 CPU 已定位，GPU 专项仍待测。
macOS arm64 / GPUI 0.3.5：滚动采样定位到未变化 Text 概览的 Scene replay，
逐笔画插入 bounds tree 占主要活跃主线程样本。Text/Blocks 概览现使用同一绘制图层，
保留笔画顺序、视口/选择标识与导航。浏览器使用后台生成的 384×256 内存缩略图，
64 项 LRU（像素数据上限 24 MiB/面板）；大小/mtime 参与缓存键，淘汰和关闭释放图片。
路径约束和解码在 worker 中执行，原图与 Asset Preview 不变，不新增磁盘缓存。

实际 GPUI Scene replay 的 release A/B：5 组各 30 次，取组均值的中位数。

| 笔画数 | 原方式 ms | 单图层 ms |
|---:|---:|---:|
| 1,200 | 0.311 | 0.029 |
| 4,800 | 2.918 | 0.081 |
| 19,200 | 67.795 | 0.339 |

```sh
cargo bench -p keine-editor --bench browser
sample <editor-pid> 20 2 -file target/authoring/performance/scroll/paired-before.txt
sample <editor-pid> 20 2 -file target/authoring/performance/scroll/paired-after.txt
```

原始命令、release 输出和调用栈保存在 `target/authoring/performance/scroll/`；
bench 源码为 `tests/bench/editor/browser.rs`，直接调用锁定 GPUI 的 Scene replay。
原生应用另以相同 dev profile、tday、3024×1772 / 2× 窗口与 24 次交替上下 3 页滚动对照：
physical footprint 1.1 GiB → 388.9 MiB，观测 peak 1.1 GiB → 465.5 MiB。
这些是该次样本的进程 footprint，并非 RSS 或跨机器保证；release A/B 是绘制路径微基准，
不能当作整窗 FPS/GPU 帧时。首见图片仍需解码，原图预览仍占用原图内存。
搜索、透明立绘、音频列表、原图预览及两个概览导航已做原生检查；
缩略图尺寸/通道/透明度、缓存容量/更新键回归通过。

Blocks 继续按源码版本缓存文本/索引/顺序/结束关联、几何与概览；普通滚动仅定位并挂载可见行，折叠、实测行高或源码变化使几何失效。当前行/执行行也复用索引。拖放和折叠动画保留原有冻结布局。Text 配色、原生换行与笔画构建移到后台，改宽复用配色；新任务取消旧任务并拒绝过期结果，不添加输入延迟。

同主机 release 微基准（`tests/bench/editor/{blocks,text}.rs`）：Block 为 11 组 × 100 次，Text 为 11 次，取中位数；测试串行运行。

| 路径 / 规模 | 原计算 ms | 新计算 ms |
|---|---:|---:|
| Block 静态准备 / 1,000 | 0.062621 | 0.000132 |
| Block 静态准备 / 5,000 | 0.283800 | 0.000152 |
| Text 概览 / 1,000 行 | 0.589 | 0.348 |
| Text 概览 / 5,000 行 | 3.239 | 1.881 |

Block 测量包括源码复制、行索引、顺序、行位置与概览，对照实际缓存命中和可见行定位；不包含控件/绘制。Text 在 GPUI 测试平台比较原 DisplayMap 构建与新 worker 构建；5,000 行改宽复用配色为 1.905 ms。它们是计算微基准，不代表原生输入延迟或整窗 FPS。

```sh
cargo test -p keine-editor --release --lib benchmark:: -- --ignored --nocapture --test-threads=1
sample <editor-pid> 20 2 -file target/authoring/performance/layout/blocks-before.txt
sample <editor-pid> 20 2 -file target/authoring/performance/layout/blocks-after-final.txt
sample <editor-pid> 20 2 -file target/authoring/performance/layout/text-before-ascii.txt
sample <editor-pid> 20 2 -file target/authoring/performance/layout/text-after-final.txt
```

原始命令、输出和 dev 调用栈位于 `target/authoring/performance/layout/`。tday、3024×1772 / 2×、相同三列布局下，24 次交替上下 3 页滚动：原采样中全文行索引重建累计 291 栈样本，最终采样未出现该调用；这仅是本次采样热点消除证据。24 次 ASCII 插入/删除的最终采样中，概览构建在 default-qos worker，而非主线程。
原生检查覆盖概览导航、改宽换行、折叠/展开、普通及嵌套重排/撤销、多行文本增高/撤销；tday 测试输入已恢复。回归覆盖行高增加/移除、折叠、版本/冻结源码，以及中文、emoji、Tab、CRLF、空行和极窄宽度与原 DisplayMap 的映射一致性、取消/过期发布。workspace 785 通过、5 个显式忽略的性能测试；fmt/check/clippy 通过。

中低项：Assets 缓存筛选/排序、目录、选择顺序与行布局，空搜索不转换所有字段；脚本索引只复制保留下来的贡献，旧任务在文件之间取消；缩略图低优先级且最多同时两项，静态 WebP 用现有 media 解码器直接缩小输出。未变文件的公共 Vec 索引仍需要复制，未新增持久化缓存或另一套索引模型。

同主机 release、11 次中位数（筛选每组 100 次），原始输出位于 `target/authoring/performance/resources/`：

| 计算路径 | 原方式 ms | 新方式 ms |
|---|---:|---:|
| 5,000 句对白 / 5,000 资源，更新 1/10 个文件 | 2.227 | 1.556 |
| 同规模，更新全部 10 个文件 | 9.034 | 8.414 |
| 5,000 资源，空搜索筛选（缓存未命中） | 0.500 | 0.039 |
| 1920×1080 背景，派生有损 WebP 缩略图 | 34.026 | 11.750 |

PNG 为 17.976 → 18.041 ms，无损 WebP 为 26.964 → 26.507 ms，单张解码没有明显改善；并发上限用于限制竞争。浏览缓存命中微基准约 0.000006 ms，只测结果复用；所有结果均不等于整窗帧时或首屏延迟。生产解码库复用 `keine-media`，没有增加外部 codec 依赖。缩放后的输出缓冲有界，但 codec 工作内存另计。

```sh
cargo test -p keine-editor --release --lib index::benchmark:: -- --ignored --nocapture --test-threads=1
KEINE_BENCH_IMAGE=/absolute/path/to/background.png cargo test -p keine-editor --release --lib benchmark:: -- --ignored --nocapture --test-threads=1
```

第二条使用同一背景派生无损/有损 WebP，交替执行原解码与缩放解码；未指定图片时生成固定图案。计时不包含编码、任务排队或 GPU 绘制。回归涵盖索引错误恢复/未变文件/取消、浏览结果失效/时间筛选/Unmapped、解码并发上限、透明度与 EXIF 旋转。

本轮 workspace 791 通过、8 个显式忽略的性能测试；fmt/check/clippy、关闭音频的视频构建及 audio-opus/audio-seekable 检查、native-smoke validate 通过。原生 tday 检查音频卡片紧凑行距/上下留白、同一 Asset Preview 的播放→暂停→继续、重播和列表按钮同步；暂停沿用当前 Player，继续不重建解码器。协议升至 v9，旧 Editor/Engine 必须成对更新。系统输出的听感未代替用户验收。

### 独立性能阶段（其余项目待测）

剩余范围为 Editor 完整帧时/大文档/搜索、Engine 启动与持续 CPU/RSS、实际 Preview FPS/GPU 帧时、粒子 GPU/全屏与瞬时峰值、音视频流与 Hakutaku I/O；使用现有 bench 与 portable benchmark 入口，不建立另一套采集协议。

启动时固定同主机、同 release、同输入及分辨率；先记录基线，再定位与修改热点，最后保存前后原始命令/结果并复验功能。Windows 自动 benchmark 的构建/打包入口保留；平台跑分和性能结论留到此阶段。

功能阶段继续运行以下接口/正确性回归，不依赖帧率、耗时或机器性能阈值：

| 接口 | 当前保留的测试合同 |
|---|---|
| Editor → Engine | 协议版本、长度边界、启动握手、崩溃重开、不同工程隔离 |
| Performance 采样 | 实际采样间隔、CPU >100%、计数回退、缺测、PID 重用、peak 重置、有界历史 |
| 源码 → Parser → Block | 全部 70 个插入入口、EYS 旧入口拒绝、分组字段与精确写回、换图省略保留；保存格式化的 token/语义不变、幂等、共享文档写入与撤销、外部冲突保护；自动聚焦的角色别名、旁白与角色继承 |
| 资源与文件 | 改名/类型迁移、映射/文件共同撤销、冲突与路径隔离、导入失败回滚 |
| Editor 交互模型 | 拖放取消/过期版本/嵌套几何、概览两端和独立平移、失去文档容器后的开页落点 |
| Audio / Video / Media | 循环 rewind/Opus pre-skip、BGM sink 交接、视频 EOF/rewind/损坏输入、队列与解码预算 |
| 发行与 benchmark | 正常包与 benchmark 分离、挂载覆盖、确定性 payload、保留目录识别；不执行跑分 |

对应 owner：`tests/authoring/process.rs`、`crates/authoring`、`crates/editor`、`src/runtime/audio.rs`、`src/runtime/package/benchmark.rs`、`src/scene/video`、`crates/media`。现有 `tests/bench` 与 CI 正确性检查保持可编译，不因暂缓删除测试或接口。

粒子优化实测：Apple M5 Pro / Metal，release LTO，仅额外保留采样符号。
tday 临时追加独立 benchmark fragment，复用同一背景，结束后逐字节恢复脚本。
可见窗口 1920×1080 / scale 1 / Fifo，`scene` profile 隔离 UI/对白绘制。
本组 CPU 对照使用旧 `perf` 的 60 Hz 唤醒模式，不作为高刷新率/全屏验收证据。
旧/新二进制交替运行三轮；每轮预热 3 秒、采帧 12 秒，下表为中位数。

| 用例 | 粒子数 | CPU % 前 → 后 | FPS 前 / 后 | p99 帧间隔 ms 前 → 后 |
|---|---:|---:|---:|---:|
| 无粒子对照 | 0 | 12.69 → 12.09 | 60.0 / 60.0 | 17.89 → 18.04 |
| 大雪 | 192 | 15.81 → 12.49 | 60.0 / 60.0 | 17.97 → 17.79 |
| 16 层大雪（压力用例） | 3,072 | 22.12 → 21.86 | 60.0 / 60.0 | 18.41 → 17.92 |

CPU 为 `time` 的 `(user+sys)/real`，含启动和预热；帧间隔排除预热，并非 GPU 执行时长。
18 次短测均为 720 帧；新版最大帧间隔 28.11 ms。
已移除 `perf` 独有的固定 60 Hz 唤醒计时，采用与正常播放一致的更新方式。
macOS 活跃帧按 `OnMonitor` 的实际刷新率安排 deadline，合成 redraw 不再额外触发更新；
输入在下一个显示周期消费，静态/后台休眠规则保持不变。未知刷新率及其他平台保持 Continuous + Fifo。
旧 60 秒测试片段结束早于采帧结束，其结果只供诊断，不能作为帧边界验收；
全屏检查延长临时片段持有时间，覆盖预热和完整采帧。
普通雪景 CPU 下降，压力场景 CPU 变化很小，不能声称全局提速或 FPS 提升。
“不因优化新增掉帧”为验收硬边界，粒子数量、质量及运动插值保持不变。

原始 `sample` 7 秒 / 1 ms：旧版压力场景网格分配/上传路径 252/627 个非等待叶样本，
其中顶点打包/拷贝 112 个样本，不能相加；粒子运动仅 26 个样本。
新版对应 Mesh 分配/打包路径为 0，粒子更新 18/437 个非等待叶样本；
仍有 ShaderBuffer 上传与 Metal staging 成本，采样分布不能替代前后帧耗时测量。
锁定 Bevy 0.19 源码确认：旧 Mesh 修改会释放并重建顶点/索引分配；
现用固定 ID/角点网格和 ShaderBuffer，同尺寸更新通过 `RenderQueue::write_buffer` 复用目标 GPU 缓冲。
四顶点展开放到顶点着色器；每粒子动态数据由 216 降至 64 字节（减少约 70%），
固定 UV/索引只在创建时上传，移除无效动态 AABB 重算，密度变更/清除同步释放缓冲。

```sh
cargo rustc --release --bin keine -- -C strip=none -C debuginfo=1
/usr/bin/time -lp target/particle-profile/before/keine perf projects/tday --seconds 12 --timeline profile_snow --camera scene
/usr/bin/time -lp target/release/keine perf projects/tday --seconds 12 --timeline profile_snow --camera scene
sample <engine-pid> 7 1 -file target/particle-profile/after-snow16.sample.txt
```

原始临时源码、旧二进制、逐轮命令/日志/调用栈、分析与构建记录在 `target/particle-profile/`；
交替结果在 `paired/`，长时间帧边界检查在 `frame-gate/`。这些 timeline 已从 demo 恢复移除。
隐藏窗口仅作准备路径对照，不作为绘制/FPS 证据；采样器运行不计入帧边界验收。
fmt/check/clippy 通过；workspace 752 passed / 8 ignored（IPC 在沙箱外重跑），
粒子默认功能和 `--no-default-features --features video-native` 各 10 passed。
原生 Metal 窗口确认雪/雨绘制；用户目测确认全屏无卡顿并要求停止窗口验收。
120 Hz 全屏压力场景常态 p99 从 17.14 降至 10.28 ms；完整日志包含窗口切换/操作，
用户确认验收期间操作过窗口，不能把这些运行当成零长帧或零丢帧的客观证明。
其他平台和有效 GPU pass 时间尚未测量。
Metal 的 Bevy RenderDiagnostics 输出 0 不能作为 GPU 时间；
透明混合、场景/文本框模糊及原先瞬时峰值不在本轮性能结论内。

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

当前唯一 demo 为 `projects/tday`：原工程 13 个章节加入口包装，共 14 scene / 3583 action。
原工程 4403 个文件的路径与 SHA256 均未改变。副本跳过缺失的 equipment_power_down.wav、
window_slam.wav、convenience_store_chime.wav，位置标注在脚本开头；旧本地 demo 已移入废纸篓。
震动振幅/频率补间、混合镜头原子动作、带 ID 单次/循环音效及淡入淡出、退格和点击等待的
解析/执行/迁移 roundtrip 回归通过。IR schema v6，Editor–Engine 协议 v9，save 仍为 v11。
fmt、workspace check/clippy/tests（publisher）、官方 LetsGal 外部示例验收、native-smoke 与 tday
校验通过；无默认 feature + video-native 的 tests check 通过。tday 唯一警告为标题背景未被脚本引用。
macOS 新 Editor/Preview 已启动 tday，日期字幕、排练室背景与开场对白可见。
按用户要求，完整剧情、画面和实际音效验收交由用户后续完成；素材仍为开发格式，未做发行规范化。

Editor 资源导入已实现即时规范化，源文件只读、成品留在目标资源目录；PNG 透明像素、
JPEG 方向、动态图片拒绝、冲突/失败清理和文件/映射共同撤销回归通过。
本机显式运行 FFmpeg 验收：WAV/FLAC/MP3/Vorbis → Opus 的解码样本数不变、
合规 Opus 字节不变、损坏音频失败不留输出，以及 MOV → H.264 MP4 实际解码均通过。
这两项外部工具测试默认 ignored，安装 FFmpeg 后运行：
`cargo test -p keine-editor --lib workspace::file_ops::tests:: -- --include-ignored`。
fmt、workspace check/clippy（publisher）、workspace tests（781 passed / 3 ignored）、
Editor debug 构建与 native-smoke/tday 校验通过；Trash/IPC 测试在沙箱放行后通过。
本轮新导入的界面操作与主观媒体验收留给用户；存量 tday 素材仍保留迁移时的开发格式。
后续资源状态提示已通过 workspace check/clippy/tests 与 Editor 构建；macOS 新 Editor
打开 tday 的 Assets 已目测 `Resource · PNG` 信息行，保留完整缩略图及既有 ID/引用数。

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
└── 暂缓：正式签名/notarization；Windows/Linux 系统废纸篓尚未实现
```

当前代码已通过 fmt、workspace check、clippy、Editor debug 构建与 native-smoke validate；默认 workspace tests 729 passed / 1 ignored，no-default-features + publisher workspace tests 763 passed / 1 ignored。无默认 feature 与 bundled-opus 的音频边界各 2 项及 video-native 视频 8 项回归通过。audio-opus 单独配置的 check 通过，测试链接因本机缺少系统 libopus 未完成；bundled-opus 覆盖相同的无 seekable 路径。组合 hot-reload/video-native/video-ffmpeg/publisher 的 check 通过。
测试覆盖源码/文件事务、macOS 废纸篓恢复、对白尾部参数解析/执行/迁移；这些不代替完整运行态验收。

功能约定核对：EYS 统一入口/旧写法拒绝、Position/Layout 分组、稀疏换图与 scale、逐立绘 light、对白尾部选项和行内 wait；工作台下拉框、Text/Block 概览、全文搜索、Preview 关闭、拖放、序号/执行标识、结束开关、输入检测；资源网格/音频列表、筛选/Tags、物理改名/类型移动、Unmapped/Remap/Trash；迁移裸 ID/objects 映射/particles 清单和原子镜头动作均有当前实现与功能回归。原生视觉证据与用户通过项以上表为准。
本轮补齐：sprite.update 插入模板省略位置/缩放，Loader 与 Block 投影不再把对白后的 return/break 吞作语音 ID。插入模板回归扩展为全部 70 个入口。
这两项新修复已换上新 Editor/Engine 原生复验：插入模板只生成目标与图片参数；对白后的 Return/Break 各自显示为 Block，实际执行能返回调用处、退出循环，换图后位置与缩放保持。原有未保存草稿保留，项目文件未改写。
无解码器配置的 Preview 曾因 AudioSource 未注册而 SIGABRT；现在由音频配置入口补齐类型注册，并保留已有音频资源。回归覆盖普通/图库播放及已有 registry 不被重建；无音频构建原生 Preview 已验证缺少 Opus 时不崩溃、对白仍能继续。Preview 失败长提示改为固定尺寸图标，详情留在 tooltip/Output；最小窗口已检查图标不遮挡 Text/Blocks 标签。
宽矮 Editor 的 Text/Blocks、概览、资源框和 Inspector 滚动已目测。按用户确认，极窄窗口通过调整分隔线或关闭一列 View 使用，不要求三列同时保留的极限布局适配；撤销专为此添加的类型标识收缩/裁剪，不再列为本轮待修复项。
音频复验复用临时环境工程，播放无首尾静音的两秒 Opus 纯音；用户确认循环无停顿/爆音、第一首→第二首→第一首的 1 秒淡入淡出均正常。停止指令后脚本继续到下一句。此结果只覆盖本机该输入，不推定所有设备/素材已通过。临时新增脚本/音频已移除；环境工程 53 个文件和 letsgal-native 95 个文件的路径与 SHA256 均恢复基线。
明确未完成的功能仍为 Windows/Linux 系统废纸篓与进程指标；正式签名/notarization 属发行工作，跨平台/显示适配/主观音频属运行态验收。它们不计为本机核心功能已完成，也不混入性能阶段。

EYS v2.0 的配置版本为 2；默认值、显式旧版/未知版拒绝及迁移输出版本已通过回归。0.12.0 的 Editor/Engine 构建与版本输出、fmt/check/clippy、workspace tests、可选 feature 和 native-smoke/letsgal-native 校验通过。
统一入口的解析、执行、迁移及旧入口拒绝已通过回归；IR schema 为 v6。
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
