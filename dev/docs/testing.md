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
0.11.1
├── macOS：原生 Engine、Text/Block/Inspector 写回、保存重开、故障恢复已有运行态证据
├── 最新工作台：下拉框、Text 概览、全文搜索、Asset Preview 关闭/重开已目测
├── Performance：macOS 启停/失败恢复、关闭后持续采样、重开历史、重启 peak 重置已目测
├── UI 容器简化：多标签圆角、切换、面板缩放/滚动与跨分组拖放已用新 release 目测
├── 用户验收：中文 IME、1×/多显示器/极端比例、主观音频、实际 Preview FPS
├── Windows x64 / Linux：构建 CI 与运行态验收分别看待；运行态暂缓
└── 非本次完成：资源删除/remap 产品闭环、媒体规范化、正式签名/notarization
```

视觉验收直接操作当前构建的原生应用；不得为采证新增 screenshot hook、环境变量协议或自动化。
构建、测试、包签名和 UI/音视频验收必须分别报告。历史一次通过不替代当前代码复验。
