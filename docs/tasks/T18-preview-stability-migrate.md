# T18 — Preview stability and Eiyashou project migration

## Status

Complete in the integration tree; cross-platform visual/audio acceptance remains
part of the broader Editor evidence gap recorded in `docs/PROJECT_STATE.md`.

## Depends on

- T13 Eiyashou typed adapter.
- T16 embedded runtime preview.

## Goal

Keep the embedded Preview responsive at the Engine's native 60 Hz cadence, including projects
with continuous audio, without accumulating frame-upload work. Add a public
`cargo migrate <source-project> <target-project>` command which opens any supported authoring
adapter read-only and materializes an equivalent default Eiyashou project.

## Ownership

- `crates/editor/src/preview.rs` and the narrow Preview presentation path in
  `crates/editor/src/app.rs`.
- `crates/authoring-protocol/src/frame.rs` and `src/runtime/authoring.rs` only when measurements
  prove the frame transport or Engine update loop is responsible.
- `src/project_migration.rs`, the CLI/bootstrap entry points, and focused loader exports required
  to translate an already-adapted project.
- `.cargo/config.toml`, `README.md`, `docs/PROJECT_STATE.md`, and
  `docs/performance-baseline.md` as integration-owned command/evidence surfaces.

No Eiyashou grammar expansion, runtime semantic change, compatibility-semantic expansion, package
format change, or new dependency is authorized.

## Required behavior

- Preview remains latest-frame-wins and bounded; it must not queue image uploads or recreate work
  for a frame id already presented.
- Active Preview keeps the Engine's normal 60 Hz target. Hidden, occluded, unchanged, paused, and
  stopped Preview avoids continuous conversion/upload work.
- Audio must remain continuous during Preview and the control channel must stay responsive.
- Migration accepts exactly a source and target path. Source content is never modified.
- Migration rejects package inputs, unknown adapters, an existing target, invalid source content,
  unsupported/lossy semantics, path escape, and output which cannot pass native validation.
- Successful output uses `adapter.script: keine`, `.shou` source, Eiyashou manifests, and copied
  confined assets. It contains no adapter authoring JSON.
- Native Eiyashou input is rejected as already native rather than silently copied or rewritten.

## Evidence

- Record representative before/after Preview CPU, memory, frame publication/overwrite behavior,
  and audio continuity in `docs/performance-baseline.md`.
- Add focused tests for bounded/latest-only presentation and migration failure/rollback rules.
- Run the workspace validation gate and `cargo validate projects/test-project`.
- Exercise Preview with Computer Use against a real project containing audio, then close every
  process started for acceptance.
