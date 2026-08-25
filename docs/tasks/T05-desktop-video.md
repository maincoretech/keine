# T05 — Desktop video acceptance and hardening

**Execution:** `parallel-safe`

## Goal

Expand realistic desktop video evidence and fix backend-local correctness, cancellation, or memory
issues without introducing another cross-platform video framework.

## Scope

- Exercise FS and encrypted Hakutaku sources with damaged headers, no audio, long GOP, tail `moov`,
  seek/loop, cancellation, and process exit.
- Validate current AVFoundation/Metal behavior on macOS and FFmpeg behavior on Windows/Linux.
- Measure queue/memory behavior before changing buffering or upload strategy.

## Non-goals

Windows Media Foundation, VA-API/DMABUF, mobile backends, transparent video, new codec promises, or
full native-library sanitizer builds.

## Relevant files / modules

`src/scene/video.rs`, `src/scene/video/`, `dev/tools/video_*`, `dev/fixtures/video/`,
`benches/video_queue.rs`, `.github/actions/setup-video/`, and media-safety workflow.

## Interfaces it may depend on

`ContentFile`/`AssetCursor`, shared video state, cancellation channel, global media budget, stable
Bevy image handle, platform-selected backend plugin, and Hakutaku random access.

## Ownership

- Owns `src/scene/video.rs`, `src/scene/video/`, `dev/tools/video_*`, `dev/fixtures/video/`, and
  `benches/video_queue.rs`.
- Owns `.github/actions/setup-video/`, `.github/workflows/media-safety.yml`, and
  `dev/docs/architecture/09-native-video.md`.

## Avoid modifying

Core video Action semantics, publisher/release workflow, Cargo manifests/lock, general render/UI,
and integration-owned files. Request integration for feature or CI-matrix changes.

## Required behavior

Queues and reads remain bounded and cancellable. Hakutaku playback creates no plaintext temporary
file or full compressed copy. EOF and decode errors remain distinguishable. Backend selection stays
compile-time/target-based.

## Acceptance criteria

- Available desktop backends pass real FS and Hakutaku decode acceptance.
- New edge fixtures are minimal, redistributable, and generated/documented reproducibly.
- Any unsafe/FFI change states ownership, thread, lifetime, and buffer-size invariants.
- Memory/queue changes include before/after evidence.

## Tests / validation

```text
cargo test -p keine --no-default-features --features video-native scene::video::       # macOS
cargo run --locked --no-default-features --features publisher,video-native --bin keine-video-acceptance -- dev/fixtures/video/playback.mp4
cargo test -p keine --no-default-features --features publisher,video-ffmpeg scene::video::
cargo run --locked --no-default-features --features publisher,video-ffmpeg --bin keine-video-acceptance -- dev/fixtures/video/playback.mp4
cargo bench --bench video_queue
```

Run only commands supported by the current target and report the untested matrix explicitly.

## Dependencies on other tasks

None. T08 requires this desktop source/backend contract to be stable.

## Completion report

Report platform/native versions, fixtures, FS/Hakutaku results, files, tests/benchmarks, unsafe
invariants, untested targets, and requested manifest/workflow integration.

## Worker startup prompt

Read `AGENTS.md`, `docs/PROJECT_STATE.md`, and `docs/tasks/T05-desktop-video.md`.

Then inspect only the code relevant to this task. Implement within its ownership boundaries; do
not redesign unrelated systems. Run the specified validation. Report what changed, files changed,
tests run, remaining risks, and interface changes other threads must know about.
