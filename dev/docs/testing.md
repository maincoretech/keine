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
├── Asset 生命周期：改名确认与物理文件/映射更新已目测；Unmapped/Remap/Trash 整套 UI 尚待验收
├── Performance：macOS 启停/失败恢复、关闭后持续采样、重开历史、重启 peak 重置已目测
├── UI 容器简化：多标签圆角、切换、面板缩放/滚动与跨分组拖放已用新 release 目测
├── 用户验收：中文 IME、1×/多显示器/极端比例、主观音频、实际 Preview FPS
├── Windows x64 / Linux：构建 CI 与运行态验收分别看待；运行态暂缓
└── 暂缓：媒体规范化、正式签名/notarization；Windows/Linux 系统废纸篓尚未实现
```

当前代码已通过 fmt、workspace check、clippy、workspace tests 与 native-smoke validate。
测试覆盖源码/文件事务、macOS 废纸篓恢复、对白尾部参数解析/执行/迁移；这些不代替完整运行态验收。

EYS v2.0 的配置版本为 2；默认值、显式旧版/未知版拒绝及迁移输出版本已通过回归。0.12.0 的 Editor/Engine 构建与版本输出、fmt/check/clippy、workspace tests、可选 feature 和 native-smoke/letsgal-native 校验通过。
统一入口的解析、执行、迁移及旧入口拒绝已通过回归；IR schema 为 v5。
Position/Layout 分组语法覆盖解析、旧平铺字段拒绝、嵌套补全、Inspector 精确范围/批量写回及迁移 Action 对等回归。
当前 publisher workspace tests、clippy、含 hot-reload/video-native/video-ffmpeg/publisher 的 check 与 debug 构建通过。
macOS 原生界面已检查 Position X、viewport Height 修改保存保留其他参数，以及淡色 viewport 补全用右方向键接受。
Block 松手重排增加 200 ms 缓动；多选重复行和嵌套结构的行身份/高度映射回归通过，原生界面已检查拖放、Text 源码同步及 Cmd+Z 恢复。
拖动预览和占位改用真实卡片宽高；结构浮动预览保持抓取行高度，占位计入子行。macOS 当前构建已检查关键帧、普通卡片、整组移动、拖出取消及撤销；拖动持有期间和连续缓动的主观手感仍交用户验收。
嵌套拖放的悬停与源码编辑共用作用域校验；占位的前后槽分别收起，绝对定位内容不再撑高子行。回归覆盖同层/同分支、整组选项、跨层级/跨分支/跨场景及混合选择；macOS 新构建已检查子行与整组重排、误拖、取消、撤销后高度与间距，临时试改未保存。持有期间的连续动画手感仍交用户验收。
sprite.update 的省略保留、显式重设、缺席目标、动画目标与编译包 roundtrip 回归通过；迁移保留旧输入的重设行为，统一缩放输出 scale。当前 workspace 762 tests 通过、1 ignored；无默认 feature 的 video-native 视频测试 8 项通过。本地沙箱阻断 IPC socket 时放行后重跑，通过才算完成。
现有 letsgal-native（10 scene / 1007 action）和环境光工程（65 scene / 1137 action）改为分组写法并重新校验；native-smoke 无警告。
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

视觉验收直接操作当前构建的原生应用；不得为采证新增 screenshot hook、环境变量协议或自动化。
构建、测试、包签名和 UI/音视频验收必须分别报告。历史一次通过不替代当前代码复验。
