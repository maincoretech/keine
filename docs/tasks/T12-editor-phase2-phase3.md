# T12 — Editor Phase 2 document safety and Phase 3 authoring host

Status: complete in the integration thread on 2026-09-20.

## Scheduling

- `integration-only`
- Depends on integrated T11 / Editor Phase 1.
- The user explicitly requested Phase 2 and Phase 3 together. Implement them as two sequential
  gates: Phase 2 must pass its focused acceptance before Phase 3 product code begins.
- Stop after the Phase 3 gate. Do not implement Phase 4 frame transport or Preview rendering.

## Product scope

### Phase 2

Provide a source-preserving writable path for Kēne Native Project `config.yaml` and
`scripts/*.txt` documents:

- one editor-owned `DocumentManager` and authoritative source buffer per document;
- revision, saved revision, dirty state, selection, text input, IME, undo, and redo;
- external-change detection that never silently overwrites disk changes;
- app-data-only recovery drafts and atomic source saves;
- source diagnostics and basic Inspector selection state;
- no-op open/close must leave source bytes unchanged, including unknown commands.

Do not add Card View, a LetsGal writer, a new script format, or runtime-derived writeback.

### Phase 3

Promote the Phase 0 control spike into the internal Editor/Engine authoring boundary:

- a small shared, Bevy-free versioned protocol with bounded messages;
- Engine authoring-host launch mode with handshake, capabilities, open/close project, validate,
  lifecycle status, ping, and graceful shutdown;
- Editor `EngineLocator`, owned child process/connection, compatibility diagnostics, and a
  no-frame `PreviewSession` state machine;
- per-project isolation, crash/disconnect handling, and bounded shutdown with no orphan child;
- the Editor launches a prebuilt Engine executable directly and never invokes Cargo.

Do not add frames, shared-memory mapping, GPUI texture upload, Engine download/update, a public
SDK, or production Preview controls beyond the minimal compatibility/session surface needed to
exercise the host.

## Ownership

This integration task may modify:

- `crates/editor/**`;
- one focused shared authoring-protocol crate under `crates/` if using a single shared wire schema
  is smaller and clearer than duplicated parsers;
- `src/runtime/authoring.rs`, `src/runtime.rs`, `src/runtime/cli.rs`,
  `src/runtime/bootstrap.rs`, and narrowly required root entry-point wiring;
- workspace manifests/lockfile only for the concrete shared protocol or already-used libraries;
- focused editor/authoring fixtures under `projects/` and integration tests;
- `docs/editor-phase2.md`, `docs/editor-phase3.md`, `docs/PROJECT_STATE.md`, README command help,
  and this task record.

Do not modify loader adapters to make them writable, change core execution semantics, alter
`cargo dev`/`cargo bundle`, or broaden shipping media/package features.

## Phase 2 acceptance

- Opening and closing an unchanged native fixture preserves byte hashes.
- Editing, saving, and reopening preserves the exact intended text and unknown commands.
- Save uses same-directory temporary replacement; external disk changes produce a conflict
  instead of being overwritten.
- Recovery data lives only below editor app data and is removed after a confirmed save.
- Undo/redo is document-scoped and unaffected by Dock movement.
- Real macOS UI acceptance covers typing plus Chinese/Japanese IME, undo/redo, dirty indication,
  save, and external-change conflict.

## Phase 3 acceptance

- A separately built Engine executable completes handshake and validation without Cargo in the
  Editor launch path.
- Protocol/capability mismatch returns an explicit bounded diagnostic.
- Two project sessions use separate Engine children; one crash/disconnect does not terminate the
  other Editor window/session.
- Stop, project close, and Editor close terminate their owned Engine child within a bounded wait;
  focused process tests find no orphan.
- Existing normal Engine CLI and release feature checks remain unchanged.

## Validation

```text
cargo fmt --all --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo validate projects/test-project
```

Also run the focused editor document tests, authoring protocol/process tests, a direct prebuilt
Engine handshake/validate run, and Computer Use acceptance against the real editor. Close all test
windows and child Engine processes afterward.
