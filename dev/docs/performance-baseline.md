# Performance baseline

This file records repeatable engine measurements, not visual acceptance results.
Settled runtime captures disable persistence, warm up for three seconds, use
the 1920x1080 design resolution, and sample raw frame intervals in a release
build. The process-start protocol below intentionally has no warm-up.

## 2026-08-21 official LetsGal sample baseline

This local baseline uses the complete `letsgal-template` bundled with LetsGal
Studio, copied to the ignored `projects/letsgal` directory and renamed
`letsgal`. Its commercial media is not redistributed through this repository.
At this capture it contains 9 compiled scenes, 896 actions, 145 files, and
about 258 MiB on disk. The adapter acceptance resolves every static resource
and confirms that the sample still exercises JPG, PNG, MP3, WAV, and MP4
compatibility paths:

```text
cargo letsgal-test
```

The same test runs automatically as part of the workspace suite whenever the
local sample is present. CI and clean clones without the commercial sample
print an explicit skip; setting `KEINE_LETSGAL_PROJECT` makes a missing or
invalid configured path fail instead. The tracked `test-project` remains the
always-present CI contract.

Loader measurements use Criterion's normal warm-up and sampling protocol:

```text
cargo letsgal-perf
```

The process-start measurement reuses the release-mode hidden surface-backed
window harness:

```text
cargo startup-perf projects/letsgal 7
```

Reference environment: commit `74e9520`, 15-core Apple M5 Pro, 24 GiB unified
memory, macOS/arm64, Metal, AC power. The filesystem and GPU caches were not
globally cleared. The first process is reported separately from the median of
the following six processes.

| Loader operation | Median estimate | Throughput |
| --- | ---: | ---: |
| Adapter detection + project/manifest open | 130.57 µs | — |
| Parse 9 scenes / 896 actions | 845.60 µs | 1.060 M actions/s |
| Open + parse + program fingerprint | 1.0473 ms | 855.6 K actions/s |

| Cumulative startup milestone | First process | Repeat median |
| --- | ---: | ---: |
| Project open | 0.76 ms | 0.69 ms |
| App built | 248.25 ms | 122.75 ms |
| First completed frame | 1015.44 ms | 292.75 ms |
| First interactive title frame | 1032.55 ms | 307.11 ms |

Peak RSS was 341.2 MiB. The first-use gap is dominated by app/GPU/frame setup,
not adapter work: the source project itself opened below 1 ms in every isolated
process. This baseline is intentionally about a real imported project; the
synthetic 100k-action benchmark remains the scale/throughput stress case.

### Canonical WebP/Opus and Hakutaku acceptance

The same sample was copied to the ignored
`target/acceptance/letsgal-canonical` staging tree, leaving
`projects/letsgal` untouched. All 95 PNG/JPG images were converted to WebP at
quality 90 with alpha quality 100. Five BGM tracks, nine voice clips, and two
sound effects were converted to Opus at 160, 96, and 128 kbit/s respectively.
`cargo assets --remap` then migrated 180 references in seven source files, and
`cargo assets --pack` produced the signed Hakutaku package at
`target/acceptance/letsgal-package`.

| Production artifact | Size |
| --- | ---: |
| Original PNG/JPG media | 173.57 MiB |
| Converted WebP media | 24.30 MiB |
| Original WAV/MP3 media | 71.71 MiB |
| Converted Opus media | 17.90 MiB |
| Hakutaku `game.haku` + three segments | 49.85 MiB |

The canonical image/audio set is 82.8% smaller than its compatibility-format
source. The converted project still validates as 9 scenes / 896 actions / 0
warnings. A release engine carrying the matching derived runtime keys opened
the exact output of `assets --pack`; the ordinary development engine correctly
refused it because development builds do not embed release keys.

The canonical source's loader estimates were 137.67 µs for adapter/manifest
open, 869.28 µs for scene parsing, and 1.0512 ms for the complete
open/parse/fingerprint path. Criterion detected no statistically significant
change from the compatibility-format source, as expected: media payloads are
not decoded by this parser benchmark.

The self-running Release/LTO Hakutaku benchmark reported a 323.07 ms first
interactive run, a 270.67 ms repeat-run median, and 303.8 MiB maximum peak RSS.
The first process was captured after the same machine had already built and
opened related artifacts, so it is not claimed as a clean-machine cold start.
The actual packaged opening composition sustained 60.0 FPS average, 53.7 FPS
1% low, 18.35 ms p95, 18.62 ms p99, and 19.08 ms maximum frame time across
300 frames; its peak RSS was 287.2 MiB with 328 entities and 13 decoded images.

## Portable benchmark release

Build a benchmark edition without replacing the normal release:

```text
cargo bundle projects/test-project --benchmark
```

The output directory is always suffixed `-benchmark` (the default is
`target/bundle-benchmark`). Double-click `keine.exe` once on Windows,
or run `./keine` once on macOS/Linux. The package writes
`keine-benchmark-report.txt` beside the executable after completing:

- seven isolated process-start samples with the first launch separated from
  the repeat-launch median, plus peak RSS;
- one settled five-second sample of the actual packaged opening composition
  after a three-second warm-up;
- when the project authors Kēne's standard benchmark timelines, up to twelve
  additional settled samples: three daily workloads, all eight authored
  feature-coverage timelines, and one intentionally combined stress workload;
- average FPS, 1% low, frame-time percentiles, entity/asset counts, peak RSS,
  GPU identification, and available render diagnostics.

The window is invisible, but this is deliberately not a headless benchmark:
the normal winit window, wgpu surface, render schedule, and presentation path
remain active so results retain the costs paid by the shipped game. The marker,
memory-counter feature, hidden-window mode, automatic exit, and disabled
persistence exist only in the separately built benchmark package. Ordinary
releases and tests follow their existing paths unchanged. GitHub's manual
Release workflow exposes the same `benchmark` switch and uploads artifacts such
as `keine-windows-x64-temporary-benchmark`. On GitHub, open **Actions**, select the
specific **Release** workflow (the **All workflows** page does not show its run
button), choose **Run workflow**, keep publisher identity set to `temporary`,
and enable `benchmark` only when a performance package is needed. No repository
secret is required for this path. After all three platform jobs pass, CI updates the rolling
`benchmark-latest` prerelease with one ZIP per platform; testers do not need
access to the workflow-run artifact page. Ordinary release runs leave the
option disabled and do not build or publish benchmark packages.

The report always measures real project content. Missing standard timelines are
shown as skipped instead of substituting an unrelated cursor or failing the
package. For `test-project`, it still separates daily, feature-coverage, and
stress results so an extreme number cannot be mistaken for normal gameplay.
Its coverage group exercises all 79 StageProperty values and all five timed
event families; its stress group deliberately combines expensive effects,
portrait/background motion, 256 rain particles, camera shake, and a crossfade.
Instant control-flow, input, and persistence actions stay in the workspace
correctness suite: treating their completed static frame as a five-second
render workload would claim performance coverage without measuring their work.
Benchmark-only camera-composition A/B probes remain available through
`cargo perf` for developer diagnosis.

### 2026-08-16 representative-workload baseline

The portable suite moved its render phase into a dedicated, unreachable
`test-project` fragment. The normal acceptance story still covers every engine
feature, while benchmark reconstruction enters a compact dialogue scene with a
background, portrait, textbox, and name box. It then measures ordinary dialogue
composition, one portrait move, and one 550 ms background crossfade. This
daily group replaces the previous mixture of ordinary and diagnostic workloads;
the complete authored timelines now follow in their own coverage group, with a
separate combined stress result last.

The final self-running Release/LTO package with `startup-metrics` on the M5 Pro
reference machine produced a 1,892.78 ms first interactive launch and a
273.73 ms repeat-launch median. App construction took 1,742.29 ms on the first
run and 127.66 ms subsequently; first-use linking/cache/GPU work therefore must
remain visible rather than being hidden in a seven-sample percentile.

| Representative workload | Avg FPS | 1% low | P95 | P99 | Max | Peak RSS |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Dialogue composition | 60.0 | 56.8 | 17.40 ms | 17.61 ms | 17.63 ms | 214.4 MiB |
| Portrait movement | 60.0 | 56.4 | 17.59 ms | 17.73 ms | 17.96 ms | 214.6 MiB |
| Background crossfade | 60.0 | 53.5 | 17.73 ms | 18.69 ms | 23.31 ms | 214.6 MiB |

Each workload rendered 371 entities and retained 10 decoded images / 5.0 MiB
of CPU pixels plus two fonts / 9.8 MiB of source data.

| Feature-coverage workload | Avg FPS | 1% low | P95 | P99 | Max | Peak RSS |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Shared transforms | 60.0 | 57.1 | 17.18 ms | 17.51 ms | 17.84 ms | 211.5 MiB |
| Classic camera | 60.0 | 56.5 | 17.52 ms | 17.70 ms | 17.97 ms | 212.0 MiB |
| Optical effects | 60.0 | 56.7 | 17.35 ms | 17.64 ms | 17.81 ms | 210.3 MiB |
| Blur family | 60.0 | 56.6 | 17.42 ms | 17.66 ms | 17.73 ms | 210.7 MiB |
| Atmosphere effects | 60.0 | 58.8 | 16.88 ms | 16.99 ms | 17.96 ms | 210.6 MiB |
| Retro and mask effects | 60.0 | 55.0 | 17.65 ms | 18.20 ms | 19.48 ms | 210.3 MiB |
| Timed event types | 60.0 | 52.1 | 17.91 ms | 19.21 ms | 24.58 ms | 209.2 MiB |
| Playback controls | 60.0 | 58.3 | 16.99 ms | 17.15 ms | 17.61 ms | 210.9 MiB |
| Combined stress | 60.0 | 58.0 | 16.96 ms | 17.24 ms | 17.47 ms | 215.2 MiB |

The timed-event row, not the sustained combined-stress row, has the worst tail
latency in this capture. That points follow-up profiling toward event-triggered
resource/state changes rather than continuous multi-effect rendering. These
results are the new portable comparison baseline. The older table below is
retained as historical evidence for the superseded diagnostic-heavy suite and
must not be compared row-for-row with this one.

#### 2026-08-16 timeline resource-prefetch optimization

The loader resource manifest previously omitted assets nested in
`StageAnimation`, and the runtime lookahead started at the already-advanced
cursor. Scene-cue layers, particle textures, character targets, and timed audio
could therefore enter Bevy's asynchronous load queue only when their event
fired. The manifest now includes those resources and retains the currently
running action in the lookahead window. This follows Bevy's strong-handle model:
request the load early and keep its handle alive so event execution can reuse
the loaded asset instead of starting work on the trigger frame.

The pre-change values below are from the final portable package report above.
The post-change values are medians of three consecutive five-second Release/LTO
captures on the same M5 Pro, each after the standard three-second warm-up.

| Hotspot | 1% low before | 1% low after | P99 before | P99 after | Max before | Max after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Background crossfade | 53.5 | 56.1 | 18.69 ms | 17.81 ms | 23.31 ms | 19.00 ms |
| All timed event types | 52.1 | 56.2 | 19.21 ms | 17.78 ms | 24.58 ms | 18.09 ms |

Both workloads remained at 60.0 average FPS. The event-triggered maximum fell
by 26.4%, and neither three-run verification reproduced the original 23–25 ms
spike.

### 2026-08-15 portable-package verification (legacy workload suite)

The first complete portable benchmark run on the M5 Pro reference
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
portable. Treat the first run as the first-use observation and compare the
median of subsequent launches separately. Also record hardware, RAM, OS, power
mode, display target, commit and feature set.
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

The three runtime cameras exist throughout normal execution, while the dialog
camera sleeps whenever its overlay layer is empty. Benchmark-only decomposition
profiles pin explicit views without despawning their entities or the UI assigned
to them, which isolates render-view cost from ECS and layout cost. All four
profiles below used the same release binary, project, 1920x1080 target, action
cursor 0, and a five-second capture after warm-up.

| Active cameras | Max RSS | Peak footprint |
| --- | ---: | ---: |
| Scene + UI + dialog | 237.3 MiB | 381.5 MiB |
| Scene + UI | 235.3 MiB | 373.0 MiB |
| Scene + dialog | 234.5 MiB | 370.4 MiB |
| Scene only | 230.2 MiB | 202.1 MiB |

This historical table predates the runtime-managed profile: its complete
composition row deliberately pinned all three cameras. Current reports print
both the requested profile and the cameras that were actually active.

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
cargo perf projects/test-project 5 0 runtime
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

At the time of this capture, the local macOS release backend was AVFoundation
and the installed FFmpeg 9 was newer than the then-pinned FFmpeg 8 Rust
binding. A command-line proxy therefore isolated the same two FFmpeg algorithms
without claiming end-to-end Kēne timings: a 10-minute stream-copy expansion of
`dev/fixtures/video/playback.mp4` was sought to 09:30 and one audio frame was
decoded. Three warm Apple M5 Pro / FFmpeg 9.0.1 runs produced:

| Seek algorithm | Wall time | User CPU | System CPU |
|---|---:|---:|---:|
| Decode and discard from start | 0.21–0.22 s | 0.24–0.25 s | 0.12 s |
| Demux timestamp seek + preroll | 0.02 s | 0.01 s | 0.00 s |

Commands placed `-ss 570` after `-i` for the decode/discard control and before
`-i` for demux seeking. The important production invariant is complexity:
work now scales with the demuxer's seek preroll rather than the complete media
prefix. Since 2026-08-22, `ffmpeg-next 9.0` also passes the real Kēne filesystem
and encrypted Hakutaku acceptance paths against local FFmpeg 9.0.1. The pinned
FFmpeg 8.1.2 Windows job continues to cover its older native ABI through the
same wrapper's compile-time version detection.

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

### 2026-08-21 bounded input and idle maintenance cleanup

The WebP loader and compatibility-audio loader previously called
`try_reserve_exact` for every 64 KiB read. Both now share one bounded reader
that grows geometrically while capping every requested capacity at the same
format-specific byte limit. Profile and gallery encoding now borrow their maps
instead of cloning them into temporary wire structs. Stable foreground frames
also return before visiting audio sinks; transition frames still visit every
sink, and background frames keep visiting them so newly created players are
paused.

A temporary Criterion A/B, run through the repository's release/LTO benchmark
profile, compared the old and new algorithms in the same Apple M5 Pro process:

| Operation | Before | After |
|---|---:|---:|
| Read a 4 MiB in-memory asset in 64 KiB chunks | 225.69 µs exact growth | 92.744 µs geometric growth |
| Encode 4,096 profile variables | 169.93 µs clone + encode | 45.478 µs borrowed encode |
| Stable foreground maintenance over 64 synthetic sink states | 1.3340 ns scan | 412.62 ps early return |

The media fixture measures allocation and copying, not decoding; the new path
is about 2.43 times faster while preserving the existing logical input limits.
The profile fixture is about 3.74 times faster and, more importantly, removes a
second complete key/value map at persistence time. The synthetic audio row is a
lower bound for the loop that production now skips, not an end-to-end frame-time
claim. No serialized schema, resource budget, or background-audio behavior
changed.

### 2026-08-21 Hakutaku archive inventory

`HakutakuArchive` no longer keeps a second `HashSet<PathBuf>` beside the signed
Hakutaku catalog. It retains the compact path vector required by recursive
source enumeration and the existing direct-children directory index; file
existence now queries Hakutaku's authenticated path index. Every package open
also has to choose `TrustFirstRelease` or a persisted release floor explicitly.

The 8,192-file `content_lookup` fixture measured the intentional tradeoff:

| Operation | Before | After |
|---|---:|---:|
| Hakutaku file existence | 96.455 ns | 257.26 ns |
| 128-entry directory query | 1.7312 µs | 1.6713 µs |

The existence probe remains below 0.3 µs and avoids a second hash table whose
capacity scales with every asset. Directory lookup is unchanged within quick
benchmark noise. No block read, decoder, or rendered-frame path was changed.

### 2026-08-21 allocation-free Hakutaku path lookup

Runtime asset paths are already canonical UTF-8 package paths in the common
case. The archive adapter now validates and borrows those strings directly;
only non-canonical platform paths enter the allocating normalization fallback.
This removes the former `PathBuf`, component `Vec`, and joined `String` from
every package existence/open/read lookup without weakening parent/root path
rejection.

The same 8,192-file `content_lookup` package measured:

| Operation | Before | After | Change |
|---|---:|---:|---:|
| Hakutaku file existence | 251.52 ns | 142.73 ns | -43.3% |
| 128-entry directory query | 1.6105 µs | 1.6991 µs | quick-run noise |

Filesystem probes were unchanged. The optimized branch is allocation-free;
Windows separators, current-directory components, repeated/trailing separators,
and non-UTF-8 paths retain the normalized fallback.

### 2026-08-21 critical-path asset admission

The timeline previously submitted every distinct asset in a 20-action window
to Bevy at once. On a slow CPU or storage device, title/background resources
could therefore compete with unrelated future WebP and Opus work. The runtime
now admits assets in three ordered classes: all current-state requirements,
active animation frames, then at most eight timeline predictions. Current-state
requirements remain parallel and blocking; speculative work starts only after
they are ready and has a single in-flight admission slot.

Three isolated Release/LTO startup processes used the checked-in LetsGal test
project and the same hidden surface-backed window on an Apple M5 Pro:

| Cumulative startup milestone | Before first / repeat median | After first / repeat median |
|---|---:|---:|
| First rendered frame | 284.78 / 234.37 ms | 262.03 / 210.19 ms |
| Interactive title | 287.90 / 237.28 ms | 276.91 / 224.42 ms |

Repeat-median first frame improved by 10.3% and interactive title by 5.4%.
The first-launch interactive result improved by 3.8%. This fast machine is a
regression baseline, not a low-end acceptance result; the structural invariant
is that speculative decoding can no longer multiply CPU, storage, or allocator
contention on the critical path.

### 2026-08-21 portable Hakutaku storage matrix

The self-running benchmark bundle previously covered startup, representative VN
composition, every authored timeline feature and a combined render stress case,
but its checked-in project contained only about 1.1 MiB of media. That was too
small to distinguish local storage from an external SATA device.

Benchmark bundles now add a deterministic 204.2 MiB encrypted payload only in
publisher staging. Normal projects, normal bundles and normal runtime execution
remain unchanged. The payload maps deliberately onto Hakutaku's four existing
access classes:

| Class | Fixture | Measurement |
|---|---:|---|
| Hot | 32 × 8 KiB | first read and CLOCK repeat |
| Normal | 32 × 256 KiB | probation, second-hit admission, resident CLOCK read |
| Transient | 16 × 256 KiB | first and repeated uncached short-resource reads |
| Streaming | 6 × 32 MiB | isolated sequential, random 4 KiB and four-stream reads |

The sequential, random and concurrent Streaming measurements use disjoint
files. This prevents the benchmark itself from warming the next workload, but
the report still says `first-touch`: it does not use privileged cache eviction
and therefore does not claim a cold operating-system or drive cache. Real
project assets are swept separately, while valid WebP/Opus decode and GPU costs
remain in the rendered project workloads rather than the synthetic I/O data.

The first local package-generation check produced a 248 MiB macOS directory:
192 MiB Streaming, 9 MiB Normal, 4 MiB Transient, a 273 KiB Hot segment, an
81 KiB signed snapshot and the 43 MiB engine. This confirms that the
incompressible fixture reaches the intended encrypted access-class segments.
The internal package child then read that exact hardened output, rather than the
source tree:

| Packaged measurement | First-touch | Repeat/resident |
|---|---:|---:|
| Real 1.0 MiB project asset sweep | 754.8 MiB/s | 919.4 MiB/s |
| Hot 0.2 MiB set | 699.7 MiB/s | 5,946.4 MiB/s |
| Normal 8.0 MiB set | 963.1 MiB/s probation | 1,179.0 MiB/s CLOCK resident |
| Transient 4.0 MiB set | 1,211.6 MiB/s | 1,264.3 MiB/s |
| Streaming 32 MiB sequential | 1,390.1 MiB/s | 1,541.4 MiB/s |
| 512 random 4 KiB reads | 6,795 IOPS | 6,805 IOPS |
| Four independent 32 MiB streams | 6,247.4 MiB/s | 6,366.9 MiB/s |

These APFS figures were collected immediately after producing the package and
are therefore warm-cache implementation checks, not device claims. External
SATA throughput and first-use latency are intentionally deferred to the
portable Windows report.

### 2026-08-22 attributable portable reports

Portable reports now identify the source commit, UTC build time, and exact
Cargo feature set. Report lines are normalized to plain UTF-8 before both
printing and writing, so terminal ANSI styling cannot leak into the `.txt`
artifact.

The rendered workload matrix also runs the opening composition once with each
decomposition profile: scene + UI, scene + dialog, and scene only. The runtime
sample retains production camera sleep/wake behavior, while those three samples
pin their named camera sets. Comparing them attributes a low-end GPU regression
without turning the normal runtime sample into an artificial worst case. At five
seconds per sample this adds about fifteen seconds of measured render time plus
three child process startups to a complete portable run; normal game execution
and normal test commands remain unchanged.

### 2026-08-22 production camera policy in benchmark mode

The first Windows camera decomposition exposed an important measurement flaw on
Intel UHD 620. With the dialog camera pinned, the opening measured 40.7 FPS for
scene + dialog and 60.0 FPS for scene only; the nominal runtime sample measured
48.5 FPS because benchmark mode also pinned the otherwise empty dialog camera.
Production execution had already disabled that camera whenever its layer was
empty, so the nominal sample was measuring a synthetic render pass rather than
the player-facing opening.

The runtime benchmark profile now uses the same dialog-camera activity manager
as production. Only the three explicit decomposition profiles bypass it. A
local Apple M5 Pro Release verification remained refresh-capped at 60.0 FPS
before the change; the UHD 620 portable rerun is the meaningful after-sample.

### 2026-08-22 bounded stage-shader specialization

`StageMaterial` now keeps three bounded shader classes: plain, basic/procedural,
and multi-sample optical. This adds only one class to the previous plain/complex
split and prevents ordinary color, fog, distortion, and transition materials
from carrying the optical sampling paths. The optical shader also preserves the
existing effect precedence without calculating results that a later blur fully
overwrites. In the authored stress composition, where radial/zoom blur replaces
motion blur before bloom, the active source-sample budget falls from 16 to 10
per fragment (37.5%). No quality level or runtime feature toggle was added.

The same review found that fog hashed `floor`ed coordinates without
interpolation, producing visible square tiles rather than moving fog. It now
uses one smoothly interpolated noise field plus a continuous low-amplitude
wisp. This remains procedural: it adds no texture, allocation, entity, or render
pass. A surface-backed Metal run of the atmosphere workload completed all 600
sampled frames without a WGSL or pipeline error (60.0 FPS average, 53.1 FPS 1%
low, 18.03 ms P95, 18.85 ms P99).

The Release/LTO stress capture below uses the same Apple M5 Pro, 1920x1080
surface, three-second warm-up, five-second sample, and production camera policy
on both sides. The display cap hides shader-throughput gains on this GPU; the
small tail differences are ordinary run-to-run noise and are not claimed as a
speedup. The low-end UHD 620 portable rerun remains the meaningful throughput
measurement.

| Stress composition | Avg FPS | 1% low | P95 | P99 | Max |
| --- | ---: | ---: | ---: | ---: | ---: |
| Before | 60.0 | 54.9 | 17.78 ms | 18.20 ms | 18.35 ms |
| After | 60.0 | 54.9 | 17.77 ms | 18.21 ms | 19.17 ms |

### 2026-08-22 unified UI glass strength

Title buttons previously used `7.5` blur except for Continue, which incorrectly
borrowed the separate save-preview panel's `36.0` strength. The first consistency
pass placed every button at `12.0`; the final product decision makes the button
surfaces and preview share one `36.0` `TITLE_GLASS_BLUR` constant. The shared UI
blur scale is `1.25`, producing 45 effective units at a 1x viewport while
remaining below the existing 48-unit bound. This changes no texture, camera,
allocation, or render pass. The separable kernel moves from 7 to 19 samples per
pass for the small title-button scissor regions.

Dialog UI already had a 0.2-second `DialogFade`, but its full-screen backdrop
blur jumped to maximum as soon as `DialogRequest` existed. The blur now follows
the same eased progress from zero to the existing 36-unit maximum. This adds no
animation state or render work after the fade settles; it only distributes the
already-present full-screen blur over the dialog entrance.

The same Apple M5 Pro Release/LTO dialogue composition was captured for 600
frames on both sides with a three-second warm-up. Both runs remained refresh
capped at 60.0 FPS with a 56.6 FPS 1% low; P95 was 17.47 ms before and 17.27 ms
after. The difference is noise rather than a claimed speedup, but it rules out
a visible regression on this path.

After raising the settled title glass to the preview strength and coupling the
dialog blur to its existing fade, the same 10-second Release/LTO runtime capture
was repeated. This steady dialogue target does not display title glass or a
modal, so it is a guard against unrelated changes rather than a throughput
claim for those transient UI regions.

| Runtime guard | Avg FPS | 1% low | P95 | P99 | Max |
| --- | ---: | ---: | ---: | ---: | ---: |
| Before | 60.0 | 53.7 | 17.85 ms | 18.61 ms | 19.42 ms |
| After | 60.0 | 55.0 | 17.62 ms | 18.18 ms | 18.62 ms |

### 2026-08-22 stronger shell backdrop and immediate TITLE handoff

SAVE, LOAD, CONFIG, EXTRA, backlog, input, and modal backdrops now request the
renderer ceiling of 48 units instead of 36. At a 1x viewport the title glass is
45 effective units and the full-screen shell is 48, so a shell never looks
shallower than the buttons beneath it. Both strengths still use 19 paired
Gaussian samples per pass; the stronger shell only widens support by one pixel
and changes the existing weights.

Returning from SAVE, LOAD, or CONFIG previously faded the full-screen blur to
zero alongside the menu surface. TITLE's regional button glass was already
active underneath, but could not become visually distinct until that fade
finished. A return to TITLE now settles the persistent backdrop, route content,
and fixed header in the same update. Route-to-route transitions retain their
motion and keep the backdrop active. This removes the shell and full-screen
blur work from the exit tail rather than adding a pass.

The same Apple M5 Pro 10-second Release/LTO runtime guard was repeated. This
timeline does not hold a menu open, so the result guards unrelated steady-state
work; the changed shell path is bounded by the unchanged 19-sample kernel and
the immediate-release regression test. The single after-run maximum is an
isolated scheduling outlier and is not reflected in P95/P99.

| Runtime guard | Avg FPS | 1% low | P95 | P99 | Max |
| --- | ---: | ---: | ---: | ---: | ---: |
| Before | 60.0 | 55.0 | 17.62 ms | 18.18 ms | 18.62 ms |
| After | 60.0 | 57.0 | 17.28 ms | 17.56 ms | 26.19 ms |

### 2026-08-23 composed camera optical effects

Directional camera blur (`radial`, `zoom`, and `motion`) previously ran inside
every targeted `StageMaterial`. An `all` target therefore sampled the same
camera effect independently for the full-screen background and every character.
It now runs once on the scene camera after stage composition. Scene-only and
character-only effects retain the material path, as do local image blur,
focal-distance blur, and layer-dependent post effects. The same composited pass
now also handles camera-wide chromatic aberration, sharpening, and bloom. This
removes up to ten additional source taps from each stage layer without adding a
second fullscreen pass. The render-node shape follows Bevy's official
[`ViewTarget::post_process_write()` example](https://bevy.org/examples-webgpu/shaders/custom-post-processing/).

Stage revisions can still wake background and character synchronization while
a composited effect animates. `StageMaterial` updates now compare GPU-visible
data before mutably borrowing the asset, so an unchanged layer no longer emits
an asset change and redundant prepare/upload work.

The authored `10-04 blur family` workload contains one full-screen background
and one character. Its active center blur uses six source taps. Before this
change both layers selected the optical material and paid those taps over their
respective coverage. Afterwards the layers use their ordinary one-sample path
and one six-tap fullscreen pass processes the composition; additional
characters no longer multiply the camera-blur tap count.

The same Apple M5 Pro Release/LTO binary, 1920x1080 surface, three-second warm-up
and five-second sample was captured immediately before and after the change:

| `10-04 blur family` | Avg FPS | 1% low | P95 | P99 | Max |
| --- | ---: | ---: | ---: | ---: | ---: |
| Before | 60.0 | 54.7 | 17.84 ms | 18.28 ms | 18.96 ms |
| After | 60.0 | 54.2 | 17.82 ms | 18.44 ms | 19.62 ms |

This machine remains refresh-capped and the tail delta is noise, so no local
frame-rate gain is claimed. The structural regression test verifies that an
`all` directional blur no longer promotes each layer to the optical shader,
while selective targets still do. The Intel UHD 620 portable benchmark is the
meaningful follow-up for throughput below the 60 FPS target.

The optical extension and material-write suppression were then measured with
the same build and capture settings. These are single paired runs on a capped
machine, so the stress-tail improvement is encouraging rather than a claimed
cross-device percentage.

| Workload | State | Avg FPS | 1% low | P95 | P99 | Max |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| `10-03 optical effects` | Before | 60.0 | 53.9 | 17.87 ms | 18.56 ms | 19.01 ms |
| `10-03 optical effects` | After | 60.0 | 53.9 | 17.82 ms | 18.54 ms | 18.88 ms |
| `benchmark stress composition` | Before | 60.0 | 47.8 | 17.93 ms | 20.91 ms | 22.60 ms |
| `benchmark stress composition` | After | 60.0 | 54.5 | 17.93 ms | 18.36 ms | 18.65 ms |

The portable report now records up to five sampled frames at or above 33.33 ms
as `SLOW` entries with their elapsed capture time. This makes a first-use shader,
asset, or transition stall visible instead of hiding it behind one `max` value;
the timestamp is diagnostic correlation, not an automatic attribution.

### 2026-08-23 incremental UI Gaussian coefficients

The regional and fullscreen UI blur keeps the same separable kernel, scissor,
sample positions, and 19 texture samples per pass at the common effective
strength of 45. Only coefficient generation changed. Following NVIDIA GPU Gems
3 chapter 40, [Incremental Computation of the Gaussian](https://developer.nvidia.com/gpugems/gpugems3/part-vi-gpu-computing/chapter-40-incremental-computation-gaussian),
regularly spaced coefficients are advanced with multiplicative quotients. At
this radius the two-pass shader therefore evaluates two `exp()` operations
instead of 34, with multiplication-only updates for the remaining weights.

A CPU regression test compares every incremental coefficient through support
18 against direct Gaussian evaluation within `2e-6`. Texture bandwidth remains
unchanged, so this is principally an ALU/SFU reduction for integrated and older
GPUs rather than a promise of proportional frame-time improvement. The
quarter-resolution bloom/depth-of-field designs described in GPU Gems were not
adopted because they would change the current visual result and add render
targets/passes.

### 2026-08-23 exact stage-shader sampling variants

The remaining Intel UHD 620 workloads exposed two avoidable shader supersets.
Local/focal blur selected the complete optical path even when directional blur,
chromatic aberration, sharpening, and bloom were inactive. Outline similarly
selected that path despite needing only its own four edge samples. The material
pipeline now has bounded `blur` and `outline` variants between `basic` and the
combined `optical` superset. This is deliberately not a feature-bit Cartesian
product: a blur + outline combination falls back to the optical superset, so
the number of cached pipelines remains small.

The fragment calculations are unchanged. The blur variant executes the same
17-tap kernel and the outline variant executes the same four neighbouring
samples; only unrelated uniform branches and shader code are omitted. This
follows Intel's guidance to avoid redundant sampler work and NVIDIA's finding
that texture reads dominate post-process depth-of-field cost, while rejecting
lower-resolution buffers or a smaller kernel because those would change the
accepted image:

- [Intel Gen11 API Developer and Optimization Guide](https://cdrdv2-public.intel.com/671309/intel-c2-ae-processor-graphics-gen11-api-developer-and-optimization-guide.pdf)
- [GPU Gems 3: Practical Post-Process Depth of Field](https://developer.nvidia.com/gpugems/gpugems3/part-iv-image-effects/chapter-28-practical-post-process-depth-field)

The pre-change Windows portable report is the throughput baseline for the next
Intel UHD 620 run:

| Workload | Avg FPS | 1% low | P95 | P99 |
| --- | ---: | ---: | ---: | ---: |
| `10-02 classic camera properties` | 45.0 | 40.5 | 23.67 ms | 24.70 ms |
| `10-06 retro and eyelid mask` | 46.1 | 43.6 | 21.85 ms | 22.92 ms |

An Apple M5 Pro Release/LTO surface run validated both specialized pipelines.
The display remains capped, so the paired local samples establish no regression
rather than a speedup claim:

| Workload | State | Avg FPS | 1% low | P95 | P99 | Max |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| `10-02 classic camera properties` | Before | 60.0 | 54.5 | 17.70 ms | 18.36 ms | 19.15 ms |
| `10-02 classic camera properties` | After | 60.0 | 54.7 | 17.86 ms | 18.28 ms | 18.49 ms |
| `10-06 retro and eyelid mask` | Before | 60.0 | 55.4 | 17.67 ms | 18.04 ms | 18.67 ms |
| `10-06 retro and eyelid mask` | After | 60.0 | 56.2 | 17.52 ms | 17.80 ms | 18.31 ms |
