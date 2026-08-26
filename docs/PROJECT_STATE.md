# Kēne project state

> Integration-owned shared context. Update this file when an integrated change alters capability,
> an interface, a compatibility promise, or a known limitation. Do not record task chatter here.

## Current capability

- Desktop engine and library entry points run native/WebGAL directories, LetsGal Studio projects,
  and Hakutaku packaged projects on macOS, Windows x64, and Linux.
- `keine-core` provides typed actions, immutable `Program`, deterministic `State` transitions,
  expression evaluation, execution limits, rollback checkpoints, and persistence-safety checks.
- `keine-loader` provides capability-based adapter registration, confined overlay sources,
  WebGAL parsing, LetsGal 1.x compilation, compiled Program v1, save v10, diagnostics, and optional
  development hot reload.
- The Bevy runtime provides 1920x1080 design-space rendering, fixed scene/UI/dialog camera
  composition, reactive lifecycle scheduling, background/sprite/effect synchronization, fixed
  MainCore UI, audio, desktop video, and platform persistence roots.
- WebP is decoded through the bounded native media crate. Ogg Opus has the canonical incremental
  runtime path; PNG/JPEG and WAV/MP3/Vorbis/FLAC remain development compatibility inputs.
- `cargo assets --pack` creates only Hakutaku resources. `cargo bundle` builds the matching
  hardened engine and complete release. Production media gates require WebP and Ogg Opus.
- Publisher preparation validates and compiles project-owned input before loading or creating an
  identity. Failed release assembly preserves the previous runnable package, and generated
  LetsGal configuration and portrait variants have deterministic ordering.
- Save, backup, settings, profile, history, gallery, and preview paths have explicit input limits,
  transactional replacement, and post-commit cleanup warning semantics.
- CI covers Linux, macOS, Windows x64, dependency advisories, platform media feature contracts,
  release feature sets, WebP fuzz smoke, and Linux FFmpeg ASan acceptance. Desktop video fixtures
  cover no-audio, long-GOP, tail-`moov`, damaged-header, rewind, cancellation, FS, and encrypted
  Hakutaku sources.

## Architecture and interfaces

```text
project / package
      -> LoaderRegistry + ProjectAdapter
      -> ContentMount / ContentFile + compiled Program
      -> State + core::step -> StepResult
      -> runtime::script_driver host policy
      -> Scene / UI / Storage projections
```

- `Program`, `Action`, `State`, and `StepResult` are the adapter/runtime interface. An editor
  adapter may compile to them but may not inject editor objects into core or Bevy systems.
- `ContentMount`/`ContentFile` are the asset/media byte-source interface. Bevy, Opus, FFmpeg, and
  AVFoundation consume the same logical paths without learning Hakutaku internals.
- `GameConfig.adapter` selects asset/editor/script/store capabilities. Release engine features are
  fixed at bundle time; runtime config cannot enable code that was not compiled.
- Video backends are build-selected Bevy plugins. Shared source, visual, clock, cancellation, and
  media-budget behavior lives above FFmpeg and AVFoundation-specific state.
- Storage domains are intentionally separate: slot state, profile globals, read history, gallery,
  settings, and backups do not share rollback semantics.

## Decisions and compatibility contracts

- WebGAL compatibility is frozen at the pinned 4.6.2 evidence boundary: 5 commands implemented,
  23 partially supported, and 3 explicitly unsupported. This is a compatibility record rather
  than a parity roadmap. Existing behavior remains regression-tested; only security, crash, data
  loss, or Kēne-caused regressions justify maintenance without a new product decision.
- LetsGal Studio 1.x remains a read-only adapter. The checked-in 1.8 fixture and 1.11 acceptance
  project are active compatibility evidence; Studio extensions and bridge injection are excluded.
  A Studio-native ID outside Kēne's path-safe slug grammar is deterministically mapped to a stable
  `letsgal-*` shipping/save ID; `project.json.keine.projectId` is the explicit override.
- Save v10 and compiled Program v1 are strict envelopes. Other layouts are rejected; there is no
  best-effort legacy decoder.
- Hakutaku v1 is the sole release package. Publisher encryption raises extraction cost but is not
  DRM and does not promise secrecy from a user controlling the client.
- macOS ships AVFoundation/Metal video; Windows/Linux ship the reduced FFmpeg decode feature set.
  Canonical video is MP4/M4V with H.264 + AAC; other FFmpeg containers are compatibility inputs.
- UI layout, input scopes, blur composition, and animations use logical design-space units and
  elapsed time rather than frame-count assumptions. Normal release rendering retains the 60 Hz
  presentation cap while event-driven idle can sleep.

## Incomplete or intentionally deferred

- WebGAL `-continue`, advanced animation tables/keyframes/filters, full input validation, complete
  expression parity, Live2D/Spine/GIF, runtime UI styling, and Steam/debug bridge commands remain
  known compatibility boundaries. They are not scheduled for closure; see
  `dev/docs/webgal-compatibility/unsupported.md` for migration facts.
- Automated screenshot/golden coverage is not established across Windows/Linux, 1x DPI,
  ultrawide, and tall windows. Existing semantic tests do not prove pixel equivalence.
- The full LetsGal commercial sample is intentionally untracked. Local acceptance and loader
  benchmarks run when `projects/letsgal` or `KEINE_LETSGAL_PROJECT` is available; clean CI relies
  on tracked fixtures and `projects/test-project`.
- Windows ARM64, Windows Media Foundation, Android/iOS launchers, mobile storage adapters, and
  mobile video backends have no release commitment. Desktop behavior must not be weakened in
  anticipation of them.
- Windows/Linux video remains software-decoded RGBA upload. Hardware decode or zero-copy work
  requires real target hardware, distribution, and device-loss evidence first.

## Known status

- No confirmed P0/P1 defect is open on current main.
- The main evidence gaps are cross-platform visual acceptance and representative packaged-project
  runs on low-end hardware and slow storage, not missing safety boundaries in the canonical
  WebP/Opus/Hakutaku paths.
- Performance work must start from a repeatable hotspot measurement. Optional ideas in design
  documents are not approved work until a benchmark shows user-visible value.
- T04 closed at `55f5323`. On Intel UHD 620, `f083c57` reduced the isolated classic-sampling GPU
  pass by 42.3% and raised its median from 55.2 to the 60 FPS cap; complete classic improved 6.4%
  and combined stress 1.8%. Visual and non-target workloads showed no material regression. Godray
  and remaining combined costs require a new measured task before any further renderer change.

## Active task queue

| Task | Scheduling | Boundary |
|---|---|---|
| [T02](tasks/T02-letsgal-production.md) | parallel-safe | LetsGal adapter and external sample evidence |
| [T03](tasks/T03-ui-visual-baseline.md) | parallel-safe | UI and cross-platform visual evidence |
| [T08](tasks/T08-mobile-contract.md) | integration-only | mobile platform contract |

## Canonical references

- Project and module map: `dev/docs/PROJECT.md`
- Current backlog and acceptance status: `dev/docs/TODO.md`
- Project/media/package contract: `docs/project-and-assets-spec.md`
- Resource and persistence limits: `docs/resource-limits.md`
- Architecture contracts: `dev/docs/architecture/`
- Compatibility evidence: `dev/docs/webgal-compatibility/`
- Repeatable measurements: `dev/docs/performance-baseline.md`
