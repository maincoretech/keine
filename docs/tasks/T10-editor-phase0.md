# T10 — Editor Phase 0 architecture and technical spikes

**Execution:** `implementation`

**Status:** complete on 2026-09-20; see `docs/editor-phase0.md` for evidence and deferred boundaries.

## Goal

Freeze the first implementable Kēne Editor boundary and prove the risky technical assumptions
before Phase 1 product work begins. The result is evidence, a minimal GPUI/Dock harness, and
small disposable-quality spikes with production-quality boundaries; it is not an editor shell.

## Scope

- Recheck the current Cargo workspace, default members, aliases, features, app-data behavior,
  runtime bootstrap, render targets, and media lifecycle.
- Add the minimal `keine-editor` workspace package under `crates/editor/` while preserving the
  root Engine as the default Cargo member.
- Pin GPUI and any directly used platform/Dock dependencies to exact, reviewable revisions.
- Prove one process can own two GPUI windows and route a duplicate dummy `ProjectKey` open to the
  existing window.
- Prove typed tab drag/reorder and horizontal/vertical Dock split behavior with a small model and
  focused tests; no production workspace shell is required.
- Prove an app-data directory can be selected without writing layout state into a game project.
- Prove a minimal local control protocol across a real child process: hello, ping/pong, and clean
  shutdown. This is a protocol/process spike, not the Phase 3 Engine authoring host.
- Trace and, where feasible, minimally exercise the existing complete-composition offscreen path;
  record window coupling, camera targets, frame extraction, audio/video lifecycle, and select one
  first-version frame transport.

## Non-goals

Phase 1 editor pages, author document writing, a real VN Preview view, production authoring IPC,
game release CI, GPU zero-copy, extension/plugin systems, Engine installation/update management,
or changes to existing `dev`/`bundle`/`validate`/`assets`/`perf` command semantics.

## Ownership

- `crates/editor/**`
- `docs/tasks/T10-editor-phase0.md`
- `docs/editor-phase0.md`
- `Cargo.toml` and `Cargo.lock`, only for workspace dependency registration and exact pins needed
  by this task
- `.cargo/config.toml`, only if an explicit Editor developer alias is proven necessary (it is not
  required by default)

## Avoid modifying

`crates/core/`, `crates/loader/`, `crates/media/`, Engine runtime/scene/render/UI/storage code,
publisher code, project fixtures, existing command semantics, and Phase 1+ product surfaces. If a
spike proves an Engine-side interface is required, list it in the technical record instead of
implementing Phase 3 early.

## Required evidence

- `cargo build` selects only the root Engine graph and does not build GPUI/editor crates.
- `cargo build -p keine-editor` reaches a minimal GPUI application.
- A repeatable test or demo covers two windows and duplicate `ProjectKey` focus routing.
- Focused tests cover typed Dock reorder, move, and split, including cancelled/invalid operations.
- A real child process completes hello, ping/pong, and graceful shutdown over the selected local
  control channel.
- The offscreen/frame-transport conclusion points to current Kēne code and includes limitations;
  a mock frame is never reported as a real VN preview.
- `docs/editor-phase0.md` records exact dependency pins, commands, platform, pass/fail/not-run,
  interface needs, and the selected first-version frame transport.

## Validation

```text
cargo fmt --all --check
cargo build
cargo build -p keine-editor
cargo check --workspace
cargo clippy --workspace --all-targets
cargo test --workspace
```

Run any narrower Phase 0 tests directly as they are added. `cargo validate
projects/test-project` is not required unless the implementation crosses into project, loader,
adapter, compiler, or publisher behavior.

## Completion boundary

Stop after the Phase 0 evidence and gates pass. Do not continue into Phase 1.
The dummy GPUI/Dock harness is not a UI design deliverable. Record the approved VS Code/Muspector
visual constraints in `docs/editor-phase0.md`, but defer the complete activity rail, Explorer,
editor surface, Inspector, status bar, and production interaction design to Phase 1.
