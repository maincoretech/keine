# T23 — Asset Browser, Inspector, and Block drag

## Goal

Complete the dependent File/Asset capabilities without adding conversion, lifecycle states,
or a second content model. Asset Browser owns browsing and selection, Inspector owns safe asset
properties and references, and typed drag operations insert or replace authoritative `.shou`
source through the existing Block View projection.

## Asset Browser

- Provide Type, Folder, and `.unmapped` scopes derived from the current workspace and manifest.
- Provide List/Grid, search, type filter, and stable sort through one in-memory query model.
- Support desktop range/toggle multi-selection with deterministic source order.
- Keep rows/cells compact and selection-driven; details remain in Inspector.
- Do not decode thumbnails or scan the filesystem from the render path.

## Asset Inspector

- Show ID, Type, Path, Tags, and exact source reference locations for one selected asset.
- Show common/mixed summaries for multiple selected assets.
- Apply ID rename, type change, and tag edits to the authoritative manifest and affected native
  source documents as one preflighted operation.
- Reject incompatible type changes and every partial/stale edit without changing any document.
- Reuse the existing document dirty/undo/save path; do not write project files behind open
  documents or introduce Editor-private content state.

## Asset and Block View drag

- Asset Browser is the only resource drag source for content insertion/replacement.
- Insert Background, Figure, BGM, Effect, and Video commands at stable Block View boundaries.
- Voice may only attach to a Text Block; other incompatible targets reject the drop.
- Multi-asset drops are preflighted and all-or-nothing.
- Reuse the existing stable insertion-line feedback and source-preserving Block reorder path; do
  not perform hover-time source rewrites or live reflow.

## Explicit deferrals

- No media conversion, encoder controls, derived source retention, or import-review state.
- No thumbnail decoder/cache until a measured visual workflow requires it.
- No delete, Trash/Recycle Bin, `.unmapped` mutation, or Remap workflow; those remain a separate
  asset-lifecycle task, not Project P6.
- No Particle/LUT schema or drag behavior.
- No general transaction framework, event bus, plugin layer, or persistent asset database.

## Ownership

- Asset indexing and bounded source edits in `crates/editor/src/authoring.rs`.
- Asset query, selection, Inspector, and drag UI in `crates/editor/src/app.rs`.
- Source-preserving manifest helpers in `crates/editor/src/file_ops.rs` when needed.
- Focused tests in those files.
- This task record and the integrated status line in `docs/PROJECT_STATE.md`.

## Acceptance

- Search/filter/sort/list/grid produce the same deterministic selected asset identities.
- Asset selection reaches Inspector without opening or duplicating a document view.
- Rename/type/tag edits either update every required source range or update nothing.
- Incompatible type changes report the conflicting references and leave sources unchanged.
- Single and multi-asset drops generate valid bounded source edits; incompatible or stale targets
  leave source unchanged.
- Asset rendering performs no filesystem reads, media probes, or full-document parsing.
- `cargo fmt --all --check`, `cargo check --workspace`, `cargo clippy --workspace --all-targets`,
  `cargo test --workspace`, and `cargo validate projects/test-project` pass.

## Completion evidence

- All five acceptance commands passed on the integrated worktree. The workspace suite includes
  67 editor unit tests, the editor process test, and the root/core/loader suites; the graphics
  adapter-dependent preview test remains intentionally ignored and is unrelated to this task.
- In a running macOS Editor at 1145×768, the Asset tab shared Explorer's left slot; search by tag,
  `.unmapped`, selection-to-Inspector, reference scrolling, Asset-to-Block insertion, and ID rename
  updating both `.shou` references and `assets.yaml` were observed. A referenced type change was
  rejected with source unchanged. The activity rail was narrowed from 50 to 44 logical pixels.
  The later filter-control revision places Type, Folder, Sort, View, and Mapped/Unmapped choices
  behind one icon popover; its live visual acceptance is tracked separately from the earlier pass.
- Focused tests cover deterministic query/selection, exact reference ranges, source-preserving
  manifest edits, all-or-nothing rename preflight, Voice target rejection, and bounded Block
  insertion. Cross-platform/alternate-DPI visual acceptance remains tracked by T03, not T23.
