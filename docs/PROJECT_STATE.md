# Kēne project state

> Integration-owned shared context. Update this file when an integrated change alters capability,
> an interface, a compatibility promise, or a known limitation. Do not record task chatter here.

## Project-wide roadmap

P0–P7 are the only phase numbers for the Kēne project delivery roadmap. Component work
(Card Editor, File/Asset, Preview, and so on) uses `TXX` task IDs, never a second P0–P7 sequence.
An implemented task and its remaining visual acceptance are tracked separately; neither creates
another project phase.

| Project phase | Outcome | Status |
|---|---|---|
| P0 | Architecture and technical spike | complete |
| P1 | Workspace, windows, Dock, and app data | complete |
| P2 | Authoring documents and safe writeback | complete |
| P3 | Engine authoring IPC host | complete |
| P4 | Embedded Preview | complete |
| P5 | Core VN authoring UX | complete; T21 visual acceptance pending |
| P6 | Pinned Project CI release chain | complete; private-project Linux runner accepted |
| P7 | Cross-platform packaging, installation, input/DPI, recovery, compatibility, and performance hardening | in progress; T24 and T03 evidence pending |

Asset lifecycle (mapped deletion, recovery, and remapping) is an outstanding feature task, not a
second P6. Media conversion remains deferred. Neither is part of the completed Project CI P6.
Formal signing and notarization are outside the current P7 scope.

## Current capability

- Desktop engine and library entry points run native/WebGAL directories, LetsGal Studio projects,
  and Hakutaku packaged projects on macOS, Windows x64, and Linux.
- `keine-core` provides typed actions, immutable `Program`, deterministic `State` transitions,
  expression evaluation, execution limits, rollback checkpoints, and persistence-safety checks.
- `keine-loader` provides capability-based adapter registration, confined overlay sources,
  WebGAL parsing, LetsGal 1.x/2.0 compilation, compiled Program envelope v1 / IR schema v3,
  save v11, diagnostics, and
  optional development hot reload.
- The Eiyashou v1 `keine` script adapter owns `.shou` lexing, lossless source ranges, typed
  validation, and lowering in Bevy-free Rust. It implements strict expressions/interpolation,
  once-only variables, homogeneous lists, structured branches/loops, choices, scene flow, stable
  source identity, project manifests, character presentation, scene/media actions, non-looping
  blocking video, typed BGM playback mode, and real crossfade state.
- The Bevy runtime provides 1920x1080 design-space rendering, fixed scene/UI/dialog camera
  composition, reactive lifecycle scheduling, background/sprite/effect synchronization, fixed
  MainCore UI, audio, desktop video, and platform persistence roots.
- WebP is decoded through the bounded native media crate. Ogg Opus has the canonical incremental
  runtime path; PNG/JPEG and WAV/MP3/Vorbis/FLAC remain development compatibility inputs.
- `cargo pack` creates only Hakutaku resources. `cargo bundle` builds the matching
  hardened engine and complete release. Production media gates require WebP and Ogg Opus.
- The public developer CLI uses matching Cargo and executable verbs: `validate`, `dev`,
  `bundle`, plus explicit advanced `pack`, `remap`, `migrate`, and `perf` commands. Startup
  measurement is `perf --startup`; the machine-level adapter/video configuration TUI and its
  aliases are removed. Project formats remain selected by the loader and project config.
- Publisher preparation validates and compiles project-owned input before loading or creating an
  identity. Failed release assembly preserves the previous runnable package, and generated
  LetsGal configuration and portrait variants have deterministic ordering.
- Formal Hakutaku release payloads are rebuilt from an allowlisted runtime boundary after script
  compilation. They contain runtime config, `program.bin`, and mounted runtime assets, but no
  Native/WebGAL scripts or LetsGal authoring project. Unknown authoring adapters fail closed.
- A Linux x64 Project CI reusable workflow accepts a separately checked-out game repository only
  from a caller pinned to a full Kēne commit SHA. It selects Rust 1.97.1 and platform dependencies
  on a clean runner, rebuilds the hardened Engine from that same workflow commit, confines the
  stable publisher identity to runner-temporary storage, excludes caller/bundle/identity data from
  caches, verifies the runnable result, and writes non-secret release provenance. Private-repo
  clean-runner acceptance passed at Kēne `0e39761675d9d8e3400ba30fe41364aabd1072ca` in
  [run 35544083557](https://github.com/shiftz300/keine-project-ci-acceptance/actions/runs/35544083557);
  the independently downloaded artifact matched its published digest and contained no source or
  publisher key.
- Save, backup, settings, profile, history, gallery, and preview paths have explicit input limits,
  transactional replacement, and post-commit cleanup warning semantics.
- CI covers Linux, macOS, Windows x64, dependency advisories, platform media feature contracts,
  release feature sets, WebP fuzz smoke, and Linux FFmpeg ASan acceptance. Desktop video fixtures
  cover no-audio, long-GOP, tail-`moov`, damaged-header, rewind, cancellation, FS, and encrypted
  Hakutaku sources.
- `keine-editor` provides the Eiyashou authoring workbench: Empty Workbench and Open Recent/Folder,
  one physical project per native window, secondary-launch routing, real project discovery and
  source-backed document tabs, upstream Dock layout interactions, schema-versioned app-data layout
  persistence, and Reset Layout. Native `config.yaml`, configured manifests, and
  `scripts/**/*.shou` have source-preserving editing, IME/undo, conflict-safe atomic save, and
  app-data recovery; compatibility JSON remains read-only. Text and Card modes share one source,
  Card edits replace bounded source ranges, and unknown syntax remains visible/read-only. An
  Eiyashou project's merged native scene set owns one declaration/type scope, so global variables
  remain typed across `.shou` file boundaries after mount overrides are resolved. An
  explicit preview-before-apply command only renames already-valid legacy Eiyashou `.txt` sources;
  it never translates compatibility input. The bounded binary authoring protocol launches one
  prebuilt Engine child per project for handshake, validation, diagnostics, source snapshots and
  patches, source/runtime cursor exchange, lifecycle, runtime input, and real embedded Preview.
  Preview is an explicit singleton view placed above Inspector in the default right-hand stack;
  Engine Start remains manual. Authoring protocol v4 reports the Engine's executed source location
  while visible: Block selection seeks the Engine, direct Preview input advances it, and the
  corresponding Block/scene/file follows without a separate input mode. The older LetsGal Studio
  sync session remains view-only. Edge-triggered visibility pause and bounded
  Stop/Close cleanup remain independent of layout. Panel visibility comes
  from Dock activation rather than render-frequency inference, so unchanged UI cannot oscillate
  the child and audio between pause/resume. The Engine owns the normal three-camera
  composition in a hidden 1920x1080 target and publishes raw frames through a project/session/
  revision-checked latest-frame-wins shared-memory triple buffer. Production frames are BGRA,
  only one GPU readback may be in flight, and the Editor transfers and uploads each newest complete
  frame once without a per-pixel channel swap. The editor remains excluded from the root Engine's
  default build. `cargo migrate <source-project> <target-project>` opens a compatibility source
  read-only, rejects semantics that Eiyashou cannot preserve, stages confined referenced assets,
  emits `.shou` plus native manifests, validates the result, and only then installs a new target.
- Eiyashou `assets.yaml` keeps legacy string paths and also accepts `{ path, tags? }` entries
  without migrating untouched source. Asset IDs remain namespaced by resource type. Within one
  type, two IDs cannot resolve to the same physical project file; explicit sharing across
  different types remains valid. Loader validation fails closed for empty IDs, incomplete entries,
  escaping paths, missing files, and same-type duplicates, while the Editor exposes tags and the
  matching deterministic diagnostics from the same source-backed manifest.

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
- LetsGal Studio remains a read-only adapter. The checked-in 1.8 fixture and 1.20 acceptance
  project are active compatibility evidence; Studio extensions and bridge injection are excluded.
  New authoring work and the public development command surface are Eiyashou-first. The old
  LetsGal-only Cargo aliases were removed; read-only adapter coverage remains in the workspace
  tests and loader benchmark. Built-in formats are discovered automatically, without a global
  adapter-disable configuration.
  LetsGal 1.20 multi-target character removal and typed `stageMask` overlay/clip state are native;
  2.0 basic blueprint scheduling/chapter preprocessing, session variables, layered differential portraits,
  per-frame portrait timing, scene particle layers, mouse scene parallax, mirror-shatter and scoped
  speed-line effects, and automatic/manual background-or-wait loading strategies are native.
  Desktop parallax reads the mouse; a future mobile host will feed the same normalized axes from
  its gyroscope without changing core or the scene model.
  External-browser and Steam blocks remain explicit platform-boundary errors; dynamic Spine and
  Live2D portraits remain explicitly unsupported.
  A Studio-native ID outside Kēne's path-safe slug grammar is deterministically mapped to a stable
  `letsgal-*` shipping/save ID; `project.json.keine.projectId` is the explicit override.
- Save v11 and compiled Program envelope v1 / IR schema v3 are strict contracts. Other layouts are
  rejected; there is no best-effort legacy decoder.
- Hakutaku v1 is the sole release package. Publisher encryption raises extraction cost but is not
  DRM and does not promise secrecy from a user controlling the client.
- An editable project directory or project package can be authored, previewed, and handed off with
  prebuilt Editor and Engine binaries and does not require a local Rust toolchain. A formal release
  package is different: its build environment, normally project CI, must install the pinned Rust
  toolchain and platform dependencies, check out the pinned Kēne revision, and build the matching
  hardened Engine from source. A project package is never repackaged directly as a release.
- macOS ships AVFoundation/Metal video; Windows/Linux ship the reduced FFmpeg decode feature set.
  Canonical video is MP4/M4V with H.264 + AAC; other FFmpeg containers are compatibility inputs.
- UI layout, input scopes, blur composition, and animations use logical design-space units and
  elapsed time rather than frame-count assumptions. Normal release rendering retains the 60 Hz
  presentation cap while event-driven idle can sleep.

## Incomplete or intentionally deferred

- WebGAL `-continue`, advanced animation tables/keyframes/filters, full input validation, complete
  expression parity, Live2D/Spine/GIF, runtime UI styling, external-browser actions, and
  Steam/debug bridge commands remain known compatibility boundaries. They are not scheduled for closure; see
  `docs/webgal/unsupported.md` for migration facts.
- Automated screenshot/golden coverage is not established across Windows/Linux, 1x DPI,
  ultrawide, and tall windows. Existing semantic tests do not prove pixel equivalence.
- The full LetsGal commercial sample is intentionally untracked. Local acceptance and loader
  benchmarks run when `projects/letsgal` or `KEINE_LETSGAL_PROJECT` is available; clean CI relies
  on tracked fixtures, including `tests/fixtures/letsgal-timeline`, and the native
  `projects/test-project` editor acceptance project.
- Complex multi-route blueprint parity remains deferred. Core already owns the native flow,
  condition, choice, assignment, and scene-call primitives; the adapter does not embed a JS VM or
  Studio extension host to chase editor-specific routing behavior.
- Windows ARM64, Windows Media Foundation, Android/iOS launchers, mobile storage adapters, and
  mobile video backends have no release commitment. Desktop behavior must not be weakened in
  anticipation of them.
- Windows/Linux video remains software-decoded RGBA upload. Hardware decode or zero-copy work
  requires real target hardware, distribution, and device-loss evidence first.
- Project P4 native-window Preview acceptance is complete on macOS. Representative cross-platform
  GPU/GUI, audio/video, DPI, input, and packaging evidence belongs to P7. Native child
  surfaces, frame compression, and GPU sharing remain unapproved unless end-to-end evidence
  identifies the raw triple buffer as the bottleneck.
- Eiyashou v1 intentionally does not expose the deferred advanced runtime surface listed in its
  language reference: camera/post-process/particle timelines, arbitrary code execution, runtime UI
  skinning, import systems, dynamic Live2D/Spine/GIF authoring, SE loop/pan, or non-blocking video.
  These are out of v1 rather than incomplete v1 behavior.
- Compatibility projects that use editor-specific particles, masks, timelines, conditional legacy
  expressions, or other semantics outside Eiyashou v1 are deliberately rejected by `cargo migrate`
  instead of receiving a partial conversion. They require manual Eiyashou redesign.

## Known status

- No confirmed severity-P0–P3 defect is open on current main.
- The main evidence gaps are cross-platform visual acceptance and representative packaged-project
  runs on low-end hardware and slow storage, not missing safety boundaries in the canonical
  WebP/Opus/Hakutaku paths.
- Performance work must start from a repeatable hotspot measurement. Optional ideas in design
  documents are not approved work until a benchmark shows user-visible value.
- T04 closed at `55f5323`. On Intel UHD 620, `f083c57` reduced the isolated classic-sampling GPU
  pass by 42.3% and raised its median from 55.2 to the 60 FPS cap; complete classic improved 6.4%
  and combined stress 1.8%. Visual and non-target workloads showed no material regression. Godray
  and remaining combined costs require a new measured task before any further renderer change.
- Native `stageMask` initially added clip coverage to every stage fragment. X280 evidence at
  `8096fa7` measured the resulting no-mask regression; `eb8d971` moved clipping to an independent
  shader specialization. The isolated classic-sampling pass returned from 54.5 to 60.0 FPS and
  from 3.671 to 2.117 ms median GPU time, matching the pre-mask baseline. Active-mask throughput
  remains unmeasured and must not be inferred from the no-mask result.
- The original T02 production-project pass closed without a product-code change: the local
  representative project compiled 9 scenes and 1020 actions without diagnostics or unresolved
  static resources. Its later Studio 1.20 follow-up added native multi-target character removal
  and typed `stageMask` overlay/clip support, including blocking, rollback, editor replay, and
  explicit Save v11 rejection while a mask is active. The external sample remains untracked.
- Release workflow run 17 completed successfully for `a8175f9`, covering the integrated LetsGal
  release-ID fix and the Linux, macOS, and Windows temporary benchmark bundles.
- T03's available macOS acceptance passed on Apple M5 Pro / Metal at `1db8e15`: title, stage,
  dialogs, Backlog, Save/Load, Config, Extra, continuation, system Zoom, and native fullscreen
  showed no reproducible UI defect. Windows requires the user's remote credential; Windows,
  Linux, 1× DPI, and frame-by-frame transition evidence remain explicitly unverified.
- Project P0 proved 1920x1080 three-camera windowless composition on Apple M5 Pro / Metal.
  The three-frame Bevy Screenshot pipeline completed about 52 captures/s in both dev and release;
  the release 1080p raw slot copy averaged about 0.12 ms. Active preview still targets 60 fps,
  while unchanged and occluded previews must avoid continuous work for portable battery use.
- Project P1 opens the tracked test project and the local LetsGal fixture in independent macOS
  windows, restores editor-owned Dock state, and displays two real read-only documents without
  launching the Engine or Preview. Its accepted shell is fully dark with independent borderless
  9 px tonal Dock cards, borderless filled tabs and activity rail, even 4 px view gaps, aligned outer
  edges, and a `#BAEBFF` accent whose derivatives stay near the neutral axis. The complete top-left
  brand tile is blue for saved state; a future dirty document changes that tile to red rather than
  adding a badge or bottom strip. Tab drops insert, empty tab-bar space appends, the broad content
  centre merges, and only narrow nearest-edge zones split; tab-bar and content targets are mutually
  exclusive and content-target movement is eased. Singleton Explorer, Inspector, and Output views
  use integrated card titles rather than fake tabs. Document tab selection and close use restrained
  reduced-motion-aware transitions. Explorer preserves long paths through horizontal scrolling,
  with a subtle scrollbar and right-edge overflow fade.
- Editor authoring makes Eiyashou `config.yaml`, configured resource/character manifests, and
  `scripts/**/*.shou` writable. One authoritative source document owns text, revisions, dirty
  state, selection, undo/redo, external-change checks, atomic saves, and binary app-data recovery.
  LetsGal, WebGAL compatibility input, and JSON documents remain read-only.
  The line-number gutter omits the unused folding column, and macOS acceptance covered Unicode
  input, dirty/saved indication, save, recovery, selection, and the unsaved-close prompt.
- Project P3 adds a Bevy-free, 256 KiB-bounded, length-prefixed Postcard control protocol and
  a hidden Engine authoring-host mode. The Editor directly launches a prebuilt Engine executable,
  validates version/capabilities, opens and validates one project per child, exposes source
  diagnostics and no-frame lifecycle, and performs bounded shutdown. Process tests cover protocol
  mismatch and two isolated sessions. P4 extends this boundary without moving rendering into
  the Editor process.
- Project P4 implements the real embedded Preview path without a second Engine OS window. The
  file-backed 1920x1080 RGBA triple buffer reserves three fixed slots (about 23.7 MiB), validates
  project/session/revision metadata, and drops stale frames instead of queueing latency. On Apple
  M5 Pro / Metal, three automated Start/Pause/Resume/Stop cycles produced a visible composited frame
  in 142–161 ms, showed no frame overwrite, bounded paused publication to the three in-flight
  captures, removed every mapping, and left no authoring child process. The user accepted the final
  live macOS Editor Preview pass on 2026-09-21.
- Project P5 keeps Text and Blocks as projections of the same authoritative Eiyashou source
  document; there is no separate Dialogue view. Block selection drives Inspector and the Preview source cursor;
  the context-aware Insert Palette, confined Asset Browser, bounded Character/Scene managers, and
  navigable Problems view all edit or inspect that same source path. The Performance view reports
  only measured Preview transport counters, not invented CPU/GPU timing. Normal-scale macOS
  Computer Use acceptance covered the five-row continuous-dialogue fixture and every new tool view,
  then closed the test application. `.shou` Text mode now highlights the authoritative native token
  stream, keeps a compact black line-number gutter with source-text spacing, and supports the same
  restrained close transition from either the close affordance or middle click. A real untracked
  LetsGal project sustained embedded Preview output after the visible-panel lifecycle fix; the
  acceptance bundle and every child process were removed afterward.
  Text tabs share one highlighter and palette for `.shou`, YAML, Markdown, JSON, and TOML;
  `.shou` keeps the authoritative native token source, while plain text remains uncolored.
- T21 Card Editor continuously renders every Scene in the current `.shou` file as a lightweight
  collapsible section. Enter starts a source-free Text draft, then commits and continues even an
  empty block; Shift+Enter adds a line break within the block. Inline Text edits,
  Inspector-owned Speaker/Voice/Stable ID changes, desktop multi-selection, complete-node clipboard,
  source-order movement, and drag reorder all edit the same authoritative document. Text/Blocks
  switching restores the selected source node, unknown syntax stays visible and read-only, and
  Choice/If/Else if/Else/Loop use indentation instead of nested container cards. The full Block
  Picker remains searchable and persists favorites, category/item order, and hidden browse entries
  as global Editor preferences outside project files; hidden commands remain searchable. Blocks
  now provides compact direct Scene create/rename/delete/reorder controls in the current document,
  with exact `goto`/`call` target rewriting on rename. Native commands use distinct short rows,
  and selected non-Text properties edit bounded source ranges in Inspector, including optional
  named arguments. These changes passed code gates but still need live visual acceptance.
- T22 File/Asset work replaces the flat source list with a confined workspace tree. Explorer
  owns ordinary create, rename, move, copy, delete, internal drag, external drop, and Reveal
  operations without becoming a content editor. External resources are accepted only when already
  canonical WebP, Ogg Opus, or H.264 MP4/M4V, are content-validated before publication, derive a
  deterministic ID from the filename, and update the configured asset manifest transactionally.
  Mapped moves and renames preserve manifest comments while updating paths; mapped deletion is
  deferred to an Asset lifecycle task. Batch drops expose one aggregate progress row and one
  summary toast, and failed imports leave neither a destination file nor a dangling manifest entry.
- T23 File/Asset work adds a shared left-slot Asset Browser with deterministic search,
  Type/Folder/`.unmapped` scopes, List/Grid, sorting, and multi-selection. Asset Inspector edits
  ID/Type/Tags only after source and manifest preflight, shows exact references, and rejects
  incompatible changes without partial writes. Typed Asset drag inserts/replaces Block View
  source nodes; Voice is limited to Text blocks. Conversion and mapped-asset deletion remain
  deferred.

## Active task queue

| Task | Scheduling | Boundary |
|---|---|---|
| [T13](tasks/T13-native-dsl-adapter.md) | complete | Eiyashou adapter and typed core boundary |
| [T14](tasks/T14-release-source-exclusion.md) | complete | compiled-only Hakutaku release content |
| [T15](tasks/T15-eiyashou-editor-projection.md) | complete | source-first Editor projection and explicit migration |
| [T16](tasks/T16-editor-phase4-preview.md) | complete | real embedded Preview and bounded raw frame transport |
| [T17](tasks/T17-editor-phase5-authoring-ux.md) | complete | source-safe core VN authoring workflow |
| [T19](tasks/T19-project-ci-release.md) | complete | Project P6 pinned Project CI release chain |
| [T20](tasks/T20-asset-manifest.md) | complete | Asset manifest schema and source-preserving validation |
| [T21](tasks/T21-card-editor.md) | user acceptance | Card Editor source-preserving core interaction |
| [T22](tasks/T22-file-asset-import.md) | complete | confined workspace file tree and canonical asset auto-registration |
| [T23](tasks/T23-asset-browser-inspector-drag.md) | complete | Asset Browser, Inspector, and typed Block View drag |
| [T24](tasks/T24-project-release-hardening.md) | in progress | Project P7 standalone authoring release and cross-platform acceptance |
| [T03](tasks/T03-ui-visual-baseline.md) | user acceptance | UI and cross-platform visual evidence |

## Canonical references

- Project and module map: `docs/PROJECT.md`
- Editor architecture, File/Asset decisions, phase evidence, and questions: `docs/editor/`
- Project/media/package contract: `docs/project-and-assets-spec.md`
- Resource and persistence limits: `docs/resource-limits.md`
- Architecture contracts: `docs/architecture/`
- Compatibility evidence: `docs/webgal/`
- Acceptance procedures: `docs/acceptance/`
- Repeatable measurements: `docs/performance-baseline.md`
