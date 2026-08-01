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
and script watchers to explicit `dev` and `studio` sessions; normal binaries and
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
  shipping runtime. Only the explicit `studio-sync` session keeps continuous
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

# Sustained authored timelines (compiled action cursors)
cargo perf projects/test-project 10 22  # blur family
cargo perf projects/test-project 10 25  # atmosphere effects
cargo perf projects/test-project 10 31  # all event types

# Benchmark-only camera composition A/B. A cursor is required before the final
# profile argument; cursor 0 keeps every run on the same initial stage.
cargo perf projects/test-project 5 0 full
cargo perf projects/test-project 5 0 scene-ui
cargo perf projects/test-project 5 0 scene-dialog
cargo perf projects/test-project 5 0 scene

# Project parser/validator
cargo validate projects/test-project
```

The benchmark prints every available timeline cursor before sampling. This
keeps cursor selection explicit when the test project changes.
