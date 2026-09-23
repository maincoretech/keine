# T16 — Editor Phase 4 embedded runtime preview

## Status

Complete in the integration thread. Automated acceptance passed and the user accepted the live
macOS Preview behavior on 2026-09-21. This task completes Phase 4 only; it does not authorize
Phase 5 authoring features, a release, a tag, a commit, or a push.

## Depends on

- T12 Editor Phase 2/3 source document and bounded authoring host.
- T13 Eiyashou typed adapter and T15 source-first Editor projection.

## Goal

Add one real embedded Preview view per project, backed by the existing prebuilt Engine child. The
Engine owns normal Keine runtime composition in an offscreen 1920x1080 target and publishes raw
latest-frame-wins frames. The Editor uploads only the newest complete frame into GPUI. Active
preview targets the Engine's normal 60 Hz presentation cadence; unchanged, hidden, occluded, or
stopped preview work sleeps for portable battery use.

## Required behavior

- Preview occupies the upper half of the default right-hand stack above Inspector. Showing the
  view does not start the Engine; Start is explicit.
- Moving, splitting, resizing, or restoring the Preview view does not restart a running session.
- Closing Preview stops the owned session and releases runtime, media, frame transport, and child
  process resources within a bounded wait.
- The Engine renders real scene, normal UI, dialog UI, transitions, and post-processing into one
  offscreen composition. No second game OS window is created.
- Transport is a bounded raw triple buffer with latest-frame-wins publication. Slow Editor paint
  may drop stale frames but must not queue latency. Each publication carries project/session,
  revision, sequence, dimensions, stride, and pixel-format metadata.
- Resize preserves the single 1920x1080 design-space owner. The Editor letterboxes the latest
  frame and maps Play input back through that rectangle; resize does not rebuild on every pointer
  event.
- Edit mode reserves pointer/keyboard gestures for the workbench. Play mode forwards only input
  inside Preview content and never leaks Dock layout gestures into the runtime.
- Source cursor and runtime position use explicit messages. Results from an old document revision,
  session, or frame sequence are rejected.
- Hidden/occluded Preview pauses presentation and input forwarding without forgetting the runtime
  position; showing it resumes without creating a new session.
- Project windows and Engine children remain isolated.

## Ownership

This integration task may modify:

- `crates/editor/**` for Preview state, Dock UI, frame import, input mapping, and focused tests;
- `crates/authoring/**` for bounded preview control/metadata messages;
- `src/runtime/authoring.rs`, the narrow runtime/offscreen composition path, and directly required
  render/scene/UI wiring;
- workspace manifests/lockfile only for a demonstrated dependency already required by the chosen
  transport;
- focused fixtures, Phase 4 documentation, `docs/PROJECT_STATE.md`, and this task record.

Do not add a second game window, compression, GPU sharing, an Engine installer/updater, a public
IPC SDK, a theme/plugin abstraction, or new Eiyashou/runtime semantics.

## Acceptance

- P01–P09 in `docs/editor/architecture.md` pass.
- A real tracked fixture shows dialogue, background, sprite, game UI, and a representative
  transition/post-process; audio and supported desktop video are exercised without leaking media
  after Stop/Close.
- Focused tests prove latest-frame-wins behavior, stale revision/session rejection, letterbox and
  input conversion, Edit/Play isolation, visibility pause/resume, and bounded cleanup.
- Measurements record frame queue depth, resize rebuild count, idle CPU observation, repeated
  start/stop resource trend, and text-input responsiveness before any transport optimization.
- Computer Use acceptance uses the real Editor and Engine, then closes every test window and
  verifies no child process remains.

## Validation

```text
cargo fmt --all --check
cargo check --workspace
cargo clippy --workspace --all-targets
cargo test --workspace
cargo validate projects/test-project
```

Also run the focused Phase 4 protocol/session/frame tests, a direct prebuilt Engine preview run,
and live Computer Use acceptance. Stop after the Phase 4 gate.

## 2026-09-20 evidence

- The production path uses one hidden, virtual 1920x1080 Bevy window and redirects the scene,
  normal-UI, and dialog cameras into one `Rgba8UnormSrgb` image. It creates no second OS window.
- Control remains the authenticated loopback protocol. Frames use a file-backed shared-memory
  triple buffer with project/session/revision/frame metadata and latest-frame-wins publication.
- Focused tests cover latest-frame wins, stale revision and session rejection, UTF-8-safe source
  patches, Edit/Play input isolation, letterbox mapping, independent child sessions, and Stop
  isolation.
- The ignored GPU acceptance test completed three Start/Pause/Resume/Stop cycles on Apple M5 Pro /
  Metal. Visible first frames arrived in 142–161 ms, each cycle published 3–4 frames without
  overwrite, paused publication remained bounded by the three in-flight captures, and every Stop
  removed its mapping. The final measured run completed in 4.45 seconds.
- `cargo fmt --all --check`, `cargo check --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
  `cargo validate projects/test-project`, and `cargo build --workspace --bin keine --bin editor`
  pass.
- The user accepted the live macOS Preview pass on 2026-09-21. Cross-platform packaging, DPI,
  input, and representative hardware coverage remain Phase 7 hardening rather than an open
  Phase 4 gate.
