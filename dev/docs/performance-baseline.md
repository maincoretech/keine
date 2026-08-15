# Performance baseline

This file records repeatable engine measurements, not visual acceptance results.
Settled runtime captures disable persistence, warm up for three seconds, use
the 1920x1080 design resolution, and sample raw frame intervals in a release
build. The process-start protocol below intentionally has no warm-up.

## Portable benchmark release

Build a benchmark edition without replacing the normal release:

```text
cargo bundle projects/test-project --benchmark
```

The output directory is always suffixed `-benchmark` (the default is
`target/release-package-benchmark`). Double-click `keine.exe` once on Windows,
or run `./keine` once on macOS/Linux. The package writes
`keine-benchmark-report.txt` beside the executable after completing:

- seven isolated process-start samples with median/p95 and peak RSS;
- seven settled five-second rendering samples after three-second warm-ups:
  the initial, blur, atmosphere, and complete-event workloads plus scene + UI,
  scene + dialog, and scene-only camera comparisons;
- average FPS, 1% low, frame-time percentiles, entity/asset counts, peak RSS,
  GPU identification, and available render diagnostics.

The window is invisible, but this is deliberately not a headless benchmark:
the normal winit window, wgpu surface, render schedule, and presentation path
remain active so results retain the costs paid by the shipped game. The marker,
memory-counter feature, hidden-window mode, automatic exit, and disabled
persistence exist only in the separately built benchmark package. Ordinary
releases and tests follow their existing paths unchanged. GitHub's manual
Release workflow exposes the same `benchmark` switch and uploads artifacts such
as `keine-windows-x64-benchmark`. On GitHub, open **Actions**, select the
specific **Release** workflow (the **All workflows** page does not show its run
button), choose **Run workflow**, and enable `benchmark` only when a performance
package is needed. After all three platform jobs pass, CI updates the rolling
`benchmark-latest` prerelease with one ZIP per platform; testers do not need
access to the workflow-run artifact page. Ordinary release runs leave the
option disabled and do not build or publish benchmark packages.

### 2026-08-15 portable-package verification

The first complete `release-package-benchmark` run on the M5 Pro reference
machine produced the report successfully. Startup median/p95 was 0.86/0.89 ms
for project open, 127.38/165.21 ms for app build, 233.44/751.07 ms for first
frame, and 236.24/754.34 ms for the interactive title frame; the retained first
GPU initialization accounts for the high p95. Peak RSS was 209.2 MiB.

| Hidden surface-backed profile | Avg FPS | 1% low | P95 | P99 | Peak RSS |
| --- | ---: | ---: | ---: | ---: | ---: |
| Initial stage · full | 60.0 | 52.8 | 18.62 ms | 18.94 ms | 204.3 MiB |
| Blur family · full | 60.0 | 53.2 | 18.41 ms | 18.81 ms | 209.3 MiB |
| Atmosphere effects · full | 60.0 | 52.9 | 18.46 ms | 18.92 ms | 209.2 MiB |
| All timeline events · full | 60.0 | 52.4 | 18.62 ms | 19.09 ms | 209.1 MiB |
| Initial stage · scene + UI | 60.0 | 53.5 | 18.21 ms | 18.68 ms | 203.5 MiB |
| Initial stage · scene + dialog | 60.0 | 53.7 | 18.31 ms | 18.62 ms | 201.2 MiB |
| Initial stage · scene only | 60.0 | 54.5 | 17.86 ms | 18.34 ms | 199.3 MiB |

## Repeatable process-start baseline

`cargo perf` measures settled frame delivery and deliberately excludes startup.
Cold-start work uses a separate process-level harness:

```text
cargo startup-perf projects/test-project 7
```

Cargo builds the release executable once. That executable then launches seven
isolated child processes, waits until the title exists, all blocking assets are
ready, and a subsequent frame has completed the Bevy render schedule, then exits
the child automatically. `Instant` supplies one monotonic clock for cumulative
milestones from process entry:

- project: saved engine configuration plus project/config/content opening;
- app built: plugin and ECS application construction before `App::run`;
- first frame: first render completed through `RenderSystems::PostCleanup`;
- interactive: first completed frame after the title and blocking asset gate are ready;
- peak RSS: platform process high-water mark, compiled only by `startup-metrics`.

Every row starts a new process. The harness intentionally does not claim that the OS
filesystem cache, shader cache, or GPU driver cache is cold: clearing those
globally requires platform-specific privileges and would make the command less
portable. Compare median/p95 across all runs, and
record hardware, RAM, OS, power mode, display target, commit and feature set.
Do not emulate a low-end machine by lowering process priority; establish the
low-end gate by running this exact command on the actual reference device.

### 2026-08-15 reference capture

- Machine: 15-core Apple M5 Pro MacBook Pro, 24 GiB unified memory, Metal
- OS: macOS 27.0 (26A5406e), arm64, AC power
- Project: `projects/test-project`, 1920×1080
- Build: release/LTO, default audio/UI features plus benchmark-only
  `startup-metrics`; no video backend
- Runs: seven new visible-window processes; filesystem/GPU cache state uncontrolled

| Cumulative milestone | Median | P95 |
|---|---:|---:|
| Project open | 0.34 ms | 0.89 ms |
| App built | 125.09 ms | 140.99 ms |
| First completed frame | 235.82 ms | 254.11 ms |
| First interactive title frame | 238.09 ms | 256.94 ms |

Peak RSS reached 210.1 MiB across the seven runs. This is the high-end reference,
not the low-end acceptance result; a low-end row becomes authoritative only
after the same release command is run on the selected physical 8 GiB-class
device. This capture predates the portable package's invisible-window protocol,
so it remains a historical reference rather than a direct comparison point.
Normal game runs do not install either capture system, and normal builds do not
enable the platform memory-counter feature.

## 2026-07-22 baseline

- Machine: Apple M5 Pro, integrated GPU, Metal
- Project: `projects/test-project` (2 scenes, 42 compiled actions, 5 assets,
  about 1 MiB on disk)
- Build: macOS `--release --features video-native`；Windows/Linux `--release --features video-ffmpeg`
- Each render result: 10 seconds after a 3-second warm-up
- Frame rate: the benchmark-only lifecycle uses a 60 Hz reactive deadline so
  captures remain comparable whether the window is focused or on another
  display. Normal runtime animation remains frame-rate independent and follows
  the platform presentation rate.

| Workload | Avg FPS | 1% low | P95 | P99 | Max | CPU/core | Entities | Max RSS | Peak footprint |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Initial stage | 60.0 | 57.2 | 17.25 ms | 17.49 ms | 17.87 ms | 27.2% | 420 | 245.8 MiB | 430.4 MiB |
| Blur family | 60.0 | 56.8 | 17.31 ms | 17.59 ms | 18.10 ms | 29.9% | 424 | 250.5 MiB | 485.4 MiB |
| Atmosphere effects | 60.0 | 57.0 | 17.21 ms | 17.54 ms | 18.17 ms | 29.1% | 436 | 244.6 MiB | 421.3 MiB |
| All timeline event types | 59.9 | 55.4 | 17.37 ms | 18.04 ms | 33.40 ms | 26.8% | 425 | 245.0 MiB | 445.0 MiB |

`CPU/core` is process user plus system time divided by wall time, where 100% is
one fully occupied CPU core. A normal static title screen returned to the
reactive lifecycle and measured 0.0% CPU in five one-second samples with about
223.5 MiB RSS.

Project parsing and validation completed 100 sequential runs in 1.96 seconds,
or 19.6 ms per run, with about 18.9 MiB maximum RSS.

### Camera composition A/B

The three runtime cameras remain part of normal rendering. Benchmark-only
camera profiles can deactivate individual views without despawning their
entities or the UI assigned to them, which isolates render-view cost from ECS
and layout cost. All four profiles below used the same release binary, project,
1920x1080 target, action cursor 0, and a five-second capture after warm-up.

| Active cameras | Max RSS | Peak footprint |
| --- | ---: | ---: |
| Scene + UI + dialog | 237.3 MiB | 381.5 MiB |
| Scene + UI | 235.3 MiB | 373.0 MiB |
| Scene + dialog | 234.5 MiB | 370.4 MiB |
| Scene only | 230.2 MiB | 202.1 MiB |

Three shorter repetitions gave a median of 237.2 MiB RSS / 381.4 MiB peak for
the complete composition and 230.3 MiB RSS / 202.0 MiB peak for scene-only.
One first-run complete-composition sample reached 251.3 MiB RSS / 439.5 MiB
peak, so peak footprint should be compared across repeated runs rather than
treated as a stable resident value.

Bevy 0.19 deduplicates the pair of main view textures by render target, usage,
format, and MSAA. The cameras therefore share those textures instead of
allocating a full pair per camera. The measured non-linear jump occurs when the
first UI-bearing view becomes active: either UI or dialog alone produces nearly
the complete peak, while enabling the third camera adds only about 9-12 MiB.
The large peak is consequently associated with activating the UI render path
and its Metal/wgpu allocation pool, not three independent full-HD camera
targets. Combining UI and dialog cameras would risk the required blur ordering
for a comparatively small saving and is not currently justified.

## 2026-07-22 first optimization pass

This pass removed eager construction of the presentation, text-input, and F3
diagnostic overlays. They are now created on first use. It also confines asset
and script watchers to explicit `dev` sessions; normal binaries and
benchmarks no longer create development-only watchers. The benchmark itself no
longer injects a 2 ms wake event and instead owns a stable 60 Hz deadline.

| Workload | Avg FPS | 1% low | P95 | P99 | Max | CPU/core | Entities | Max RSS | Peak footprint |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Initial stage | 60.0 | 57.1 | 17.15 ms | 17.51 ms | 18.94 ms | 21.0% | 314 | 226.2 MiB | 388.0 MiB |
| Blur family | 60.0 | 57.1 | 17.22 ms | 17.52 ms | 18.57 ms | 28.2% | 402 | 231.5 MiB | 408.3 MiB |
| Atmosphere effects | 59.9 | 52.8 | 17.69 ms | 18.94 ms | 33.86 ms | 28.4% | 414 | 231.3 MiB | 407.9 MiB |
| All timeline event types | 60.0 | 54.8 | 17.58 ms | 18.24 ms | 27.21 ms | 26.5% | 403 | 232.9 MiB | 432.7 MiB |

The combined row uses the median memory result of three runs because Metal's
first allocation peak varied by roughly 60 MiB. The other rows are single
captures and should be treated as regression points rather than precise power
measurements.

Compared with the original pass, the initial stage saves about 19.6 MiB RSS,
42.4 MiB peak footprint, 106 entities, and 6.2 percentage points of one CPU
core. Blur saves about 19 MiB RSS and 77 MiB of peak footprint. The combined
effect workload saves about 12 MiB RSS and 12 MiB median peak footprint while
keeping CPU within measurement noise.

A normal release binary left at the static title screen measured about 153 MiB
RSS and 253 MiB physical footprint, with repeated one-second CPU samples
returning to 0%. This is the relevant baseline for a player's idle engine; the
forced 60 Hz rows above intentionally measure sustained rendering cost.

### 2026-08-12 macOS redraw regression

The first macOS reactive-loop patch (`f5b3e8d`) excluded every synthetic
`RedrawRequested` event from update scheduling. That correctly stopped the idle
feedback loop, but also prevented Bevy's `Continuous` mode from driving the
next animation frame: menu input advanced one frame and then waited for another
unrelated window event. The corrected dependency revision `916ee96` ignores
synthetic redraws only in reactive mode and counts them normally in continuous
mode.

Interactive acceptance covered repeated SAVE, LOAD, CONFIG, settings-page and
back navigation with complete animations. Four one-second `top` samples on the
settled development title screen were 0.0%, 0.2%, 0.2%, and 0.3% CPU, preserving
the low-power idle behavior. This is a lifecycle regression check, not a render
throughput benchmark.

## 2026-07-22 lifecycle and high-refresh pass

- Ordinary `dev` previews now use the same focused/unfocused lifecycle as a
  shipping runtime. Only the explicit `dev --sync` session keeps continuous
  rendering while unfocused, because selected-block and authored-file changes
  must appear immediately.
- Weather simulation and dynamic particle-mesh uploads run on a bounded 60 Hz
  fixed clock. A 120/144 Hz presentation still renders at the display rate,
  while particle motion catches up by elapsed time instead of being integrated
  redundantly once per presented frame.
- The dialog camera is inactive during ordinary dialogue when its layer has no
  visible title, menu, modal, quick-preview, input, or presentation content. It
  wakes from hierarchy/display state before rendering the first visible frame.

These changes deliberately do not impose a global frame-rate cap. They remove
work whose cadence does not need to equal the monitor refresh rate and preserve
time-based animation semantics.

### Renderer floor

A temporary bare Bevy 0.19 probe using the same 1920x1080 window, `Camera2d`,
feature set, and 1.0 scale-factor override measured about 146 MiB RSS and
299 MiB physical footprint. Approximately 213 MiB was reported as graphics
footprint and 24 MiB as IOSurface storage. This establishes that most of the
native Metal footprint is the Bevy/wgpu renderer and full-HD swapchain rather
than retained project images.

The 1.0 backing-scale override is intentional: the same probe allowed to use a
2x Retina backing surface reached roughly 452 MiB physical footprint. The
engine still lays out at 1920x1080 design resolution and scales its viewport;
it simply avoids silently allocating a 3840x2160 native backing surface.

## Interpretation

- Every tested workload sustained the 60 FPS delivery target. The 1% low stayed
  between 55.4 and 57.2 FPS.
- Blur is the largest measured steady cost: about 2.7 percentage points more
  CPU than the initial stage and a 55 MiB larger peak footprint.
- The combined event timeline had one 33.4 ms frame, the only measured two-frame
  deadline miss. Asset transitions and video decode need their own longer run
  before that spike can be attributed.
- Static visual-novel scenes correctly stop continuous rendering. Benchmark CPU
  deliberately represents a continuously animated scene, not normal idle cost.
- Weather simulation and dynamic mesh uploads run at a fixed 60 Hz. Each mesh
  retains its previous and current fixed state; a compact particle material
  derives interpolation directly from Bevy's existing GPU global clock and
  blends those positions in the vertex stage at the actual display cadence.
  A 120/144 Hz window therefore stays visually smooth without per-frame CPU
  material mutation or 120/144 Hz particle integration and mesh uploads.
- This small project is a regression baseline, not an upper memory bound. A
  production-size mixed asset pack and simultaneous video, blur, and particles
  are still required before establishing the shipping memory budget.

## Reproduce

```sh
# Initial stage, 15-second sample by default
cargo perf projects/test-project

# Initial stage, 10-second sample
cargo perf projects/test-project 10

# Sustained authored timelines (stable authored ids)
cargo perf projects/test-project 10 "10-04 blur family"
cargo perf projects/test-project 10 "10-05 atmosphere effects"
cargo perf projects/test-project 10 "10-07 all event types"

# Benchmark-only camera composition A/B. A target is required before the final
# profile argument; cursor 0 is the explicit initial-stage escape hatch.
cargo perf projects/test-project 5 0 full
cargo perf projects/test-project 5 0 scene-ui
cargo perf projects/test-project 5 0 scene-dialog
cargo perf projects/test-project 5 0 scene

# Project parser/validator
cargo validate projects/test-project
```

The benchmark prints every available timeline id and its resolved cursor before
sampling. Commands use authored ids so action insertion cannot silently move a
capture to a different workload; numeric cursors remain available for ad-hoc
low-level investigation.

## 2026-08-07 script / load / rollback baseline

First criterion-based runtime baselines (`benches/`). Quick mode (1 sample);
full runs should repeat with `cargo bench --workspace` and record machine/config/commit.

- Machine: 本机 macOS（与 07-22 渲染基线同机）
- Build: `cargo bench --workspace`（release bench profile）
- Commit: `2eab45a` 之后、基准落地 commit 时记录
- Inputs: 合成 100k Action 单场景；rollback 压力场景 100 sprites + 1000 local vars

| Benchmark | Workload | Time | Throughput |
| --- | --- | ---: | ---: |
| `script_runtime/step` | 100k 非阻塞 Action 推进 | 3.36 ms | 29.7 Melem/s |
| `program_load/parse_and_build` | 100k Action WebGAL 解析 + Program 构建 | 32.10 ms | 3.12 Melem/s |
| `program_load/decode_and_build` | 100k Action 严格解码 + fingerprint 校验 + Program 构建 | 7.10 ms | 14.1 Melem/s |
| `rollback/record_200_checkpoints` | 200 个真实 checkpoint | 13.21 ms | 66.0 µs/checkpoint |

Rollback 于本次审查中修正：原基准没有设置当前 dialogue，`record_dialogue` 会提前
返回，因此 76.8 µs 的旧数据无效。新数据来自 `cargo bench -p keine-core --bench
rollback -- --quick`，每次确实复制 100 sprites 与 1000 local vars 到 200 个快照。

Program load 于同次审查增加 compiled 路径的同负载对照，命令为 `cargo bench -p
keine-loader --bench program_load -- --quick`；严格校验后的 compiled load 约为源脚本
解析路径的 4.5 倍速度。

对比口径：后续所有优化必须用同一 bench 与同一 release 配置做前后对比；结果记录
commit、机器与编译配置。

## 2026-08-09 runtime P0-P2 pass

Full Criterion runs on the same Apple M5 Pro development machine, using the
release bench profile and 100 measured samples:

| Benchmark | Result | Change |
| --- | ---: | ---: |
| `script_runtime_mixed/dialogue_turns/1000` | 3.991 ms / 250.6 K turns/s | time -16.1%, throughput +19.2% |
| `program_load/decode_and_build_100000` | 4.301 ms / 23.25 M actions/s | time -42.3% from 7.450 ms |
| `rollback/record_200_checkpoints` | 2.238 ms | time -56.8% from 5.181 ms |
| `rollback/record_200_mutating_checkpoints` | 4.831 ms | new worst-case control |

The mixed script benchmark evaluates conditions and assignments, interpolates
speaker and dialogue strings, advances each line, and therefore exercises the
allocation reductions in the borrowed expression scanner. The rollback case
contains 100 sprites and 1000 local variables. Adjacent checkpoints now share
unchanged collections; the mutating control confirms that a changed variable
map remains within the previous static baseline rather than hiding copy cost.
Compiled loading validates the envelope and content fingerprint directly over
borrowed decoded scenes, avoiding a second 100k-action clone and label-index
construction before the final `Program` takes ownership.

## 2026-08-12 save preview encoding

Quick Criterion comparison on the same Apple M5 Pro development machine:

```text
cargo bench --bench save_preview -- --quick
```

The input is a deterministic 480x270 opaque RGBA gradient with fine synthetic
grain, intended to exercise both flat visual-novel artwork and higher-entropy
detail. Both rows use the same native libwebp dependency already shipped by the
runtime.

| Encoder | Time | Output | Change from previous path |
| --- | ---: | ---: | ---: |
| lossless RGB | 76.782 ms | 39,732 bytes | baseline |
| lossy RGBA, quality 80 | 4.819 ms | 5,668 bytes | time -93.7%, bytes -85.7% |

The runtime now renders directly into a target bounded by 480x270 instead of
capturing the window-sized target and resizing it on the worker. At a 1920x1080
window this reduces the screenshot readback and queued pixel buffer from
8,294,400 to 518,400 bytes (16x smaller) and removes the CPU thumbnail/RGB-copy
step. Non-16:9 windows retain their original aspect and logical camera extent.
The save-page cache was already bounded to its ten visible slots; a regression
test now makes that lifetime policy explicit.

The same pass also makes speculative asset prefetch non-blocking, compiles a
single-sample stage shader variant for ordinary images, lets static film
patterns return to the reactive render lifecycle, reuses FFmpeg's conversion
frame, and updates same-size video images without rebuilding their descriptor,
sampler, or view state. Shader/video changes are guarded by unit tests and both
the macOS native-video and cross-platform FFmpeg feature builds; they still
need a long native playback capture before assigning an FPS or memory delta.

## 2026-08-09 event-driven invalidation pass

This pass does not cap or sample down the runtime frame rate. It preserves
elapsed-time animation and removes only writes whose resulting state is
identical: stable camera viewports and blur regions, settled UI surfaces,
unchanged audio/video presentation state, clean persistence snapshots, and
unchanged background/particle presentation data. Particle simulation retains
its existing fixed clock; no other subsystem gained a frame-rate cadence.

The `10-04-blur-family` timeline was captured for 8 seconds in the same
release configuration before and after the pass:

| Metric | Before | After |
| --- | ---: | ---: |
| Render P95 | 17.34 ms | 16.82 ms |
| Render P99 | 17.51 ms | 16.93 ms |
| Maximum | 17.60 ms | 17.49 ms |
| Wall time | 11.50 s | 11.46 s |
| User CPU time | 0.97 s | 0.97 s |
| System CPU time | 0.55 s | 0.57 s |
| Retired instructions | 9,362,494,773 | 9,328,465,448 |
| CPU cycles | 6,147,715,981 | 5,934,704,680 |
| Maximum RSS | 227.59 MB | 227.87 MB |
| Peak footprint | 418.65 MB | 419.07 MB |

CPU time and memory are effectively neutral within run-to-run noise. Retired
instructions fell by about 0.36%; the larger cycle reduction is encouraging
but too noisy to treat as a guaranteed gain from one pair of runs. The useful
result is that substantially fewer ECS changes and downstream render
invalidations did not regress sustained rendering.

After startup and control-surface settling, a static release title returned to
the macOS reactive event loop and sampled at 0.0% CPU. A three-second exit-time
`leaks` audit reported 310 retained allocations / 21,328 bytes. Symbolized
debug inspection traced the small engine-side entries to Bevy renderer startup
assets and plugin handles; the remaining largest roots were macOS NSXPC
framework cycles. No project-owned growing container was found. This is a
bounded exit-time retention result, not a claim of zero leaks. At that point,
native video still lacked a representative valid playback fixture, so its
setter suppression had no playback performance number.

## 2026-08-09 encrypted video random-access pass

This historical pre-Hakutaku comparison packed one deterministic 32 MiB
incompressible video-shaped entry. The legacy control copied the entire decoded
entry to a plaintext sink before returning; the current source preparation only
opens the seekable entry, records its length, and leaves reads to the platform
decoder:

| Preparation path | Time per open | Plaintext written before playback |
| --- | ---: | ---: |
| Legacy full copy | 122.682 ms | 32 MiB |
| Direct random-access source | 292 ns | 0 bytes |

The current equivalent command is `cargo run --release --no-default-features
--features publisher --bin keine-video-source-benchmark`. It is an explicit
developer benchmark rather than an ignored unit test, and measures source
preparation rather than decode throughput.
The committed H.264/AAC fixture separately passes the FFmpeg encrypted-Hakutaku
decode/seek/loop test and the macOS AVFoundation filesystem/Hakutaku first-frame acceptance
probe. The direct path removes the size-proportional startup copy and plaintext
temporary file; decode and texture-upload costs are unchanged.

## 2026-08-10 macOS native video frame-transfer pass

The previous AVFoundation path locked every `CVPixelBuffer`, allocated a packed
BGRA `Vec<u8>`, copied each row on the CPU, then uploaded the same bytes to a
stable Bevy texture. The new path imports the retained pixel buffer through
`CVMetalTextureCache` and performs one GPU texture copy in the render world;
the stable destination preserves the existing material bind group.

For a 1920×1080 BGRA frame, the deterministic transfer budget is:

| Transfer stage | Before | After |
| --- | ---: | ---: |
| Main-world pixel allocation | 8,294,400 bytes/frame | 0 bytes/frame |
| CPU pixel copy | 8,294,400 bytes/frame | 0 bytes/frame |
| CPU copy traffic at 60 decoded frames/s | 497,664,000 bytes/s (474.6 MiB/s) | 0 bytes/s |
| GPU texture copy/upload | queue upload from CPU bytes | one texture-to-texture blit |

This is an algorithmic byte-transfer baseline, not an FPS claim: the fixture is
320×240 and too short to represent production decode throughput. The acceptance
command below decodes both filesystem and encrypted-Hakutaku sources and executes
the real Core Video → Metal → wgpu copy, including a blocking queue completion:

```sh
cargo run --features publisher,video-native --bin keine-video-acceptance -- \
  dev/fixtures/video/playback.mp4
```

Windows/Linux still use the FFmpeg software-frame upload path; this result must
not be extrapolated to those platforms. Directly sampling the imported Metal
texture could remove the remaining GPU blit, but would trade a stable material
binding for per-frame external-texture synchronization and is intentionally
deferred.

The same fixture can exercise the FFmpeg filesystem/Hakutaku decoder and audio
path on Windows or Linux:

```sh
cargo run --no-default-features --features publisher,video-ffmpeg \
  --bin keine-video-acceptance -- dev/fixtures/video/playback.mp4
```

## 2026-08-11 Hakutaku v1 runtime baseline

The named `stream-32m-v1` scenario generates one deterministic 32 MiB
incompressible `opening.mp4`, packs it into 128 independently authenticated
256 KiB AES-256-GCM blocks, then exercises the same `hakutaku-core` reader that
will be linked by the engine. Sequential reads use a 256 KiB caller buffer;
random reads perform 10,000 seeks followed by exact 4 KiB reads. No GUI or
frame-rate loop participates.

- Machine: Apple M5 Pro, arm64, local APFS storage
- Build: release, LTO, one codegen unit
- Cache: default 64 MiB plaintext budget; `Package::trim()` before each phase
- Result: median of three independently generated package runs

| Metric | Median |
| --- | ---: |
| Full pack and staged verification | 113.509 ms |
| Signed snapshot open | 0.074 ms |
| Sequential authenticated read | 1,507.2 MiB/s |
| Random authenticated 4 KiB read | 5,932 IOPS |
| Immutable segment bytes | 33,560,576 bytes |

Reproduce with `cargo bench --offline -p hakutaku-pack --bench runtime` from
the Hakutaku workspace root. This is an in-memory/OS-buffered runtime regression
point, not a cold-device storage claim. The prepared snapshot/segment AES keys
and cursor-private decoded blocks ensure a small caller read does not repeat the
AES key schedule or decrypt the same 256 KiB block.

### Streaming buffer ownership optimization

After the baseline, the RAW decode path was changed to keep decrypted
Streaming/Transient blocks in their owned `Vec<u8>`. Only Hot blocks and
second-hit Normal blocks are converted into shared cache entries. Kēne now
stores the resulting `AssetCursor` directly in `ContentFile`, removing its old
64 KiB container-specific read-ahead layer.

The table is the median of three before and three after runs of the same
`stream-32m-v1` fixture on the machine above:

| Metric | Before | After | Change |
| --- | ---: | ---: | ---: |
| Full pack and staged verification | 109.727 ms | 107.664 ms | -1.9% |
| Signed snapshot open | 0.059 ms | 0.058 ms | noise floor |
| Sequential authenticated read | 1,556.7 MiB/s | 1,590.1 MiB/s | +2.1% |
| Random authenticated 4 KiB read | 6,308 IOPS | 6,486 IOPS | +2.8% |

The important invariant is structural: uncached video blocks now have no
decode-result copy before bytes reach the caller. The measured gains are modest
because APFS cache, AES-GCM, BLAKE3 verification, and the final caller copy still
dominate this small fixture.

### 2026-08-12 buffered cursor and bounded prefetch

Kēne pins Hakutaku `144c9e3`. Sequential and complete reads now recycle
ciphertext/decompression buffers, cached plaintext keeps its decoded allocation,
and a Streaming cursor retains the current and previous block. The `A -> B -> A`
regression test confirms that the final return to `A` issues no segment read.

The current reader also retains its active immutable-segment handle and encodes
catalog/page/block AAD into fixed-size stack arrays. This removes the shared
idle-handle cache lookup and a small heap allocation at block boundaries without
adding a platform path: Kēne and Hakutaku Core still depend only on the generic
`PositionedFile` contract.

The upstream three-run median moved 128 KiB sequential reads from 1,612.7 to
1,636.3 MiB/s (+1.5%), 256 KiB reads from 1,629.8 to 1,659.3 MiB/s (+1.8%), and
uniform random 4 KiB reads from 6,689 to 6,784 IOPS (+1.4%). These remain warm
APFS regression figures. The canonical protocol and raw interpretation live in
<https://github.com/maincoretech/hakutaku/blob/main/PERFORMANCE.md>.

Against `0438e78` in a later same-session three-run A/B, `144c9e3` improved the
median 128/256 KiB sequential paths by 2.3%/3.5%, random 4 KiB reads by 2.9%,
and full packing by 6.0%. A cursor-local block-map page window was measured and
rejected because its changes stayed at noise level while retaining more state.

`Asset::prefetch_range` is available with a separate bounded cache, but Kēne does
not yet call it speculatively. Existing Bevy asset loading already runs on its
task system, while video decoders maintain their own read cadence; adding a
second predictor without a cold-read trace could duplicate authentication work.

### 2026-08-13 end-to-end Opus streaming

The project Opus loader previously called `read_to_end` before constructing the
incremental decoder. Playback avoided a full PCM allocation, but a large
compressed source was still copied completely into an `Arc<[u8]>`. The loader
now retains a reopenable logical source and gives each playback a seekable
`ContentFile`; filesystem and Hakutaku assets therefore share the same bounded
decoder path. Embedded UI cues remain memory-backed because they are small and
have no project mount.

The deterministic regression fixture chains valid Ogg Opus logical streams to
slightly over 16 MiB and decodes the first 100 ms (4,800 samples). The former
loader necessarily read the complete fixture before playback. The new probe
reads only the container/packet data demanded by Symphonia:

| Startup operation | Before | After |
| --- | ---: | ---: |
| Compressed bytes read before first 100 ms | 16,787,192 bytes | 96,027 bytes |
| Persistent compressed allocation | complete asset | logical path + mounted reader state |
| Decoder output | incremental PCM | incremental PCM |

Reproduce the byte-bound assertion with `cargo test --release
initial_opus_playback_does_not_read_the_complete_asset -- --nocapture`. The
regular `cargo bench --workspace` suite remains the image-preview performance
guard and is run for this performance-changing commit; it is not an audio
throughput benchmark. This result proves bounded startup I/O, not cold-storage
latency or simultaneous multi-stream throughput.

### 2026-08-13 bounded Opus loops

Bevy's `PlaybackMode::Loop` delegates to Rodio's `repeat_infinite`. Rodio
documents that this stores decoded samples and uses memory proportional to the
sound length: <https://docs.rs/rodio/0.22.2/rodio/source/trait.Source.html#method.repeat_infinite>.
That defeated the bounded project-Opus read path after the first pass even
though the compressed source itself remained streamed.

Project Opus loops now use a labeled Bevy sub-asset whose decoder seeks its
reopenable `ContentFile` to zero at EOF. Bevy receives `PlaybackMode::Once`, so
Rodio never wraps this source in `Buffered`. Non-looping Opus playback retains
its existing duration and seek behavior. The regression test decodes one
complete embedded cue plus another 2,400 samples and therefore crosses EOF
without constructing a decoded-pass cache.

At 48 kHz stereo `f32`, the removed cache grew by 384,000 bytes per second, or
21.97 MiB per minute (109.86 MiB for a five-minute BGM). The replacement keeps
only the current decoded Opus packet in `OpusStream.samples`; an Opus packet is
limited to 120 ms by RFC 6716, so stereo decoded packet storage is at most
46,080 bytes. Symphonia's bounded read-ahead and Hakutaku's current/previous
256 KiB Streaming blocks remain separate bounded reader state. This is a
structural allocation bound rather than an RSS estimate.

The complete `cargo bench --workspace` suite passed on both the pre-change
`HEAD` clone and the modified tree. Because the first full runs were affected by
compile heat and ordering, the core runtime guard was immediately rerun in both
trees with `cargo bench -p keine-core --bench script_runtime`:

| Unrelated runtime guard | Before median | After median | Change |
| --- | ---: | ---: | ---: |
| 100,000 comment steps | 1.9498 ms | 1.9260 ms | -1.2% |
| 1,000 mixed dialogue turns | 3.1857 ms | 3.1408 ms | -1.4% |

These small changes are within the suite's noise threshold and are recorded
only as a regression guard; they are not claimed as benefits of the audio
change. Reproduce the loop invariant with `cargo test
looping_opus_stream_rewinds_without_buffering_a_decoded_pass`.

### 2026-08-13 cross-format gallery seeking

The Extra BGM player now uses one byte-length-aware asset for WAV, MP3, Ogg
Vorbis, and FLAC. Story playback, asset prefetch, and the gallery resolve the
same typed asset, so opening Extra does not retain a second copy of compressed
audio. Each playback constructs an independent Rodio decoder over the shared
`Arc<[u8]>`; duration and random-access seek therefore work for every supported
gallery format. Duration metadata is initialized once on the first gallery
query, so ordinary voice/effect asset loads do not construct an extra decoder.
Opus remains on its mount-backed incremental path.

Vorbis and FLAC switched from Bevy's Lewton and Claxon features to its
Symphonia backends because the former decoders do not implement random-access
seek. Looping non-Opus sources use Rodio's `LoopedDecoder`, which reconstructs
the decoder at EOF instead of buffering a decoded pass. A deterministic WAV
test crosses EOF and verifies that the source continues without Bevy's
`PlaybackMode::Loop` buffer.

One-second WAV, MP3, Ogg Vorbis, and FLAC files generated in `/tmp` were used
for an acceptance probe. Every decoder reported at least 900 ms duration,
successfully sought to 500 ms, and produced another sample. The files are not
repository fixtures. The portable checked-in regression uses a generated PCM
WAV and verifies duration, seek, case-insensitive extension routing, and
bounded loop restart.

The stripped macOS release binary was measured with
`hardened,audio-all,ui-sounds` and release LTO:

| All-format engine | Bytes | Change |
| --- | ---: | ---: |
| Before, mixed native backends | 45,260,784 | — |
| After, seekable shared asset | 46,087,520 | +826,736 (+1.83%) |

Project packaging still selects only extensions that occur in the staged
project, so this is the all-format upper bound rather than the cost paid by an
Opus-only release.

`cargo bench --workspace` completed after the implementation. The generic
preview, rollback, script, and program-load benchmarks all moved slower in the
same post-LTO run despite having no changed code paths, with inconsistent
follow-up results after cooling; those machine-state figures are not attributed
to audio. The release-size A/B above and the format-specific decode/seek/loop
checks are the reproducible measurements for this change.

### 2026-08-14 backup import allocation bound

Backup import now deserializes V2 file names and payloads as slices borrowed
from the already bounded envelope. The former owned representation copied each
payload while the complete input buffer was still resident. The accepted
envelope limit also dropped from 512 MiB to 128 MiB because export still holds
the collected files and its serialized output at the same time; a streaming
container remains the long-term route to a lower bound.

The `backup_decode` target in `cargo bench --workspace` decodes the same 32 MiB,
single-file V2 envelope with the old owned representation and the current
borrowed representation. This run used the same Apple M5 Pro development
machine and release bench profile as the other local baselines.

| Representation | Median decode time | Payload allocation while input is resident |
| --- | ---: | ---: |
| Owned V2 | 18.450 ms | 32 MiB copy |
| Borrowed V2 | 18.436 ns | 0 payload bytes |

The borrowed timing is effectively constant-time envelope validation for this
shape because postcard can return the encoded byte range directly; it is not a
general storage-throughput claim. At the 128 MiB envelope limit, the structural
import bound removes up to another envelope-sized payload copy. Export can
still approach two envelope-sized buffers, but its new limit caps that payload
component near 256 MiB instead of the former near-1-GiB worst case.

The complete `cargo bench --workspace` suite passed after this benchmark was
added. Unrelated render-preview, rollback, script-runtime, and program-load
benchmarks were slower together in this post-LTO run despite having no changed
code paths; those machine-state shifts are not attributed to backup decoding.

### 2026-08-14 asynchronous project hot reload

Before this change, any accepted source-watcher event synchronously rescanned,
read, parsed, validated, and rebuilt the complete project inside Bevy `Update`.
The 100,000-action `program_load/parse_and_build` fixture measured 33.366 ms in
the pre-change quick run, already twice a 60 Hz frame budget.

Hot reload now coalesces watcher events, performs the complete build on a named
worker thread, and checks `JoinHandle::is_finished()` before joining and
atomically replacing config, manifest, and `Program` at a frame boundary. If a
new event arrives while a build is running, that completed result is discarded
and rebuilt from the newer filesystem state. Kēne deliberately does not enable
Bevy's global `multi_threaded` feature for this path.

The post-change benchmark separates background construction from the remaining
frame-boundary work:

| Work | Median | Frame-critical |
|---|---:|---|
| Source parse + Program build, before | 33.366 ms | yes |
| Source parse + Program build, after | 32.500 ms | no |
| Apply prebuilt 100k-action Program | 459.43 µs | yes |

The full post-change suite reported no parse performance change (`p = 0.39`);
the improvement is scheduling, not parser throughput. The apply benchmark
prepares the immutable `Program` outside Criterion's timed section, then
measures fresh state install and the initial bounded runtime step. It is a
conservative proxy for the frame-boundary swap; config and manifest ownership
transfers are constant-time moves for this fixture.

### 2026-08-14 cancellable FFmpeg frame queue

The FFmpeg decoder previously retried a full two-frame `sync_channel` every
2 ms. A paused frame consumer could therefore wake each decoder about 500 times
per second, while a newly available slot still waited for the next polling
interval. The queue now uses a blocking Crossbeam selection between frame
capacity and a dedicated bounded cancellation signal. Cancellation is biased
over delivery when both become ready together.

`video_queue/full_queue_handoff` fills a one-entry queue, confirms backpressure,
then releases a consumer and measures producer completion plus thread handoff.
It isolates the old fixed polling delay from media decode time:

| Full-queue handoff | Median |
|---|---:|
| Legacy 2 ms polling | 2.9778 ms |
| Selectable blocking send | 21.380 µs |

The benchmark is a scheduling microbenchmark, not decoded-frame throughput.
The production queue remains bounded at two RGBA frames; the change removes
periodic wakeups and handoff delay without raising its memory budget. The
complete `cargo bench --workspace` suite passed afterward; existing backup,
save-preview, rollback, script-runtime, and program-load targets showed no
regression attributable to this change.

### 2026-08-14 content layer and package directory lookup

`OverlayAssetReader::read_meta` previously opened and discarded the asset file
while choosing the highest-priority layer, then opened its metadata. It now
uses the mount's containment-aware existence probe and still reads metadata
only from the layer that actually supplies the asset. A filesystem fixture
measured the two layer-selection primitives as follows:

| Filesystem layer probe | Median |
|---|---:|
| Open asset and discard reader | 16.328 µs |
| Containment/existence probe | 8.996 µs |

Hakutaku already retained the snapshot's file set, but each `read_directory`
filtered the complete set and rebuilt a `BTreeSet`. The archive now constructs
a sorted parent-to-direct-children table once at open. On an 8,192-file package
with 128 files in the queried directory:

| Hakutaku directory query | Median |
|---|---:|
| Full file-set scan | 151.18 µs |
| Direct-children index lookup and clone | 1.701 µs |

The index intentionally trades persistent memory proportional to canonical
path components for query time; it is not part of Hakutaku's evictable block
cache budget. `content_lookup` builds and opens a real signed Hakutaku package
outside Criterion's timed loops, then measures only the public archive/mount
operations.

The complete `cargo bench --workspace` suite passed. The two unchanged WebP
save-preview targets moved about 2% slower together in that full post-LTO run;
no save-preview, WebP, or storage code participates in this lookup change, so
that machine-state shift is not attributed to the directory index.

### 2026-08-14 low-frequency runtime hotspots

Static audit found three small loops whose cost scaled with project or dialogue
size. `handle_bgm` copied and sorted every unlocked track on every Extra-menu
frame even without an interaction; sprite synchronization searched the complete
rendered-node query once per desired sprite; and active dialogue paths decoded
the full UTF-8 string to count characters on every rendered frame.

The runtime now constructs a borrowed, deterministically ordered BGM list only
for selection/previous/next/end-of-track actions. Sprite spawn/despawn maintains
a persistent ID-to-entity index. Dialogue consumers cache the Unicode scalar
count by complete text content, so save restore and same-cursor hot reload still
invalidate correctly.

`cargo bench --bench runtime_hotspots -- --quick` compares the previous and
current algorithms on the same Apple M5 Pro release-bench build:

| Hotspot | Workload | Before | After |
|---|---|---:|---:|
| Idle Extra BGM | 4,096 unlocked tracks | 209.43 µs copy + sort | no list construction; below timer resolution |
| Sprite lookup | 256 desired/rendered sprites | 55.928 µs nested scan | 2.3285 µs indexed lookup |
| Dialogue length | cached 3,072-scalar UTF-8 line | 282.45 ns decode | 173.48 ns content-cache hit |

The sprite row is about 24 times faster at the deliberately large fixture. The
dialogue cache still compares bytes to make invalidation content-safe, so its
benefit is intentionally smaller than an identity-only cache. The BGM row
measures only the work removed from an idle frame; interaction queries and
audio sink maintenance remain in the production system.

The complete `cargo bench --workspace` suite passed afterward. In that
continuous post-LTO run, unchanged core rollback/script, loader parse/lookup,
video-queue, and WebP targets moved slower together while the unchanged lossy
WebP path moved faster. None of those crates or algorithms depend on the root
UI/ECS code changed here, so the correlated machine-state drift is not
attributed to these hotspot changes; the paired rows above are the relevant
same-process comparison.

### 2026-08-14 FFmpeg timestamp audio seek

The former `FfmpegAudioStream::try_seek` reopened the asset and decoded every
sample from the beginning to the requested position. The replacement asks the
demuxer for the closest decodable position at or before the target, flushes the
audio decoder and resampler, then discards only timestamped preroll frames.
Loop wrap uses the same seek path instead of reopening the media.

The local macOS release backend is AVFoundation, while the installed FFmpeg 9
is newer than the project's pinned FFmpeg 8 Rust binding. A command-line proxy
therefore isolated the same two FFmpeg algorithms without claiming end-to-end
Kēne timings: a 10-minute stream-copy expansion of
`dev/fixtures/video/playback.mp4` was sought to 09:30 and one audio frame was
decoded. Three warm Apple M5 Pro / FFmpeg 9.0.1 runs produced:

| Seek algorithm | Wall time | User CPU | System CPU |
|---|---:|---:|---:|
| Decode and discard from start | 0.21–0.22 s | 0.24–0.25 s | 0.12 s |
| Demux timestamp seek + preroll | 0.02 s | 0.01 s | 0.00 s |

Commands placed `-ss 570` after `-i` for the decode/discard control and before
`-i` for demux seeking. The important production invariant is complexity:
work now scales with the demuxer's seek preroll rather than the complete media
prefix. The pinned FFmpeg 8 CI feature job supplies the real Kēne decode,
encrypted random-access, exact remaining-duration, and loop regression tests.

### 2026-08-14 global video surface budget

Both video backends now reserve four RGBA surface equivalents per active
session from a shared 256 MiB pool. The reservation is recomputed only when a
stream changes resolution; normal same-size frames compare the cached
dimensions and return before touching the shared mutex.

A temporary 20,000,000-iteration harness called the production 1920×1080
same-size fast path under the optimized development profile on Apple M5 Pro.
It measured 1.617 ns/call. The previous implementation had no global budget
check; this is the complete added steady-frame CPU cost, well below decoder,
copy, upload, and frame-scheduling work. Resolution changes take the mutex and
checked arithmetic once, while session teardown releases its reservation.

### 2026-08-14 sprite render revision

`SpriteRenderCache` previously compared its cloned sprite `HashMap` with the
complete live map on every frame before checking camera scalars. Core `State`
now carries a transient stage revision that script actions, animated
presentation updates, rollback/save restore, and whole-state replacement all
invalidate. The idle render check is one integer comparison and the cache no
longer owns a second sprite map.

`cargo bench --bench runtime_hotspots -- --quick` measured the two idle checks
on the same 256-sprite maps in one Apple M5 Pro release/LTO process:

| Sprite state check | Median |
|---|---:|
| Full `HashMap` equality | 2.3190 µs |
| `u64` stage revision | 602.81 ps |

The synthetic token comparison is roughly 3,847 times faster. Script actions
conservatively bump the token even when an action does not touch the stage;
those events are low-frequency compared with rendered frames and this avoids
mutation branches silently omitting invalidation. Active animations already
perform stage work each frame and bump only after that work changes render
inputs.

### 2026-08-15 metadata-first quick-save startup

The title screen formerly decoded the complete quick-save state before its first
frame merely to decide whether CONTINUE was available and populate its hover
card. Startup now reads the bounded metadata prefix only. The preview WebP is
loaded on the I/O task pool only after the player first hovers CONTINUE; a save
without a preview keeps the text-only card instead of decoding state for a
background fallback. Clicking CONTINUE remains the point where the complete
state is decoded and restored.

`cargo bench --bench save_preview -- --quick` compared both codec paths over the
same 4,239,346-byte save containing 50,000 representative local variables:

| Startup save operation | Median |
|---|---:|
| Complete state decode | 3.6236 ms |
| Metadata prefix inspection | 46.491 ns |

The roughly 77,900-fold codec difference excludes filesystem I/O because both
inputs were already memory-resident. On disk, the new path additionally avoids
reading the state payload at startup, so its absolute benefit scales with save
size and storage speed. No renderer quality, frame-rate, or asset-prefetch
policy changed in this pass.
