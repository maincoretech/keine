# Performance baseline

This file records repeatable engine measurements, not visual acceptance results.
All runtime captures disable persistence, warm up for three seconds, use the
1920x1080 design resolution, and sample raw frame intervals in a release build.

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
cargo perf projects/test-project 10 10-04-blur-family
cargo perf projects/test-project 10 10-05-atmosphere
cargo perf projects/test-project 10 10-07-event-track

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

The current equivalent command is `cargo test --no-default-features --features
video-ffmpeg benchmark_hakutaku_video_direct_open_against_legacy_copy --
--ignored --nocapture`. It measures source preparation rather than decode throughput.
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
and cursor-private current block ensure a small caller read does not repeat the
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
