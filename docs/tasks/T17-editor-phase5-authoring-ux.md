# T17 — Editor Phase 5 core VN authoring UX

## Status

Complete on `codex/T17-editor-phase5-authoring-ux`. The bounded Phase 5 surface defined by the final
Editor architecture is implemented and verified. It does not include the explicitly deferred visual
UI designer, complex keyframe editor, localization/voice workflow, narrative blueprint, Live2D, or
Spine.

## Depends on

- T15 Eiyashou source-first Editor projection.
- T16 embedded runtime Preview and source/runtime cursor exchange.

## Goal

Turn the source-safe Document and real Preview into a practical visual-novel authoring loop without
introducing a second document model. Text, cards, inspectors, palettes, indexes, and managers must
produce bounded source edits through the authoritative `SourceDocument` and its existing undo/save/
recovery path.

## Required behavior

1. Keep Text and Script Card modes over one source document and make card selection explicit.
2. Connect Document/Card selection to Inspector and Preview source cursor.
3. Provide a continuous-dialogue authoring surface for adjacent dialogue/narration blocks.
4. Provide a context-aware Insert Palette that inserts valid Eiyashou source at a stable boundary.
5. Index project resource references and expose an Asset Browser using confined project paths.
6. Provide bounded Character and Scene management over the existing manifests/source declarations.
7. Provide a Problems view combining parse/validation/runtime diagnostics with source navigation.
8. Provide a minimal Performance Timeline based only on measured Preview frame/control statistics;
   do not invent profiler precision that the protocol does not expose.
9. Preserve singleton tool-view titles, existing Dock/layout persistence, dark `#BAEBFF` visual
   language, source round-tripping, multi-project isolation, and adaptive idle behavior.
10. Highlight `.shou` source from the authoritative Eiyashou token stream, keep the line-number
    gutter compact, and close document tabs with either the close affordance or middle click.
11. Keep Preview live while its panel is rendered and pause it only after the panel or window is
    actually hidden.

## Ownership

This task may modify:

- `crates/editor/**` for selection, projections, indexes, source-edit commands, Dock views, and
  focused tests;
- `crates/authoring-protocol/**` only if a measured existing message cannot carry required
  diagnostics/timeline data;
- focused Editor fixtures and this task record.

Integration-owned files are updated only by the root integration pass after the worker commit.
No Engine/core/loader schema or Eiyashou grammar expansion is authorized.

## Acceptance

- Existing Text/Card edits still round-trip unknown source without loss.
- Selection updates Inspector and Edit-mode Preview cursor without starting Preview implicitly.
- Dialogue and palette operations are undoable bounded source edits and preserve UTF-8 boundaries.
- Asset/reference indexing is deterministic, confined, and reports missing references.
- Character/Scene operations reject duplicates and invalid identifiers before editing source.
- Problems entries navigate to the authoritative document span.
- Timeline clearly distinguishes transport counters from frame-time measurements.
- Eiyashou keywords, calls, labels, strings, numbers, comments, operators, and annotations are
  distinguishable without rewriting source.
- The compact gutter preserves readable line numbers, and middle-click follows the existing tab
  close transition.
- Live Computer Use verifies the real Editor at normal scale and every test process is closed.

## Validation

```text
cargo fmt --all --check
cargo check --workspace
cargo clippy --workspace --all-targets
cargo test --workspace
```

Run the focused Editor tests and the development command
`cargo editor projects/test-project`.

## Evidence

- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy --workspace --all-targets`
- `cargo test --workspace` (all workspace and documentation tests passed)
- Computer Use at normal scale verified Text, Cards, five-row continuous Dialogue, Assets,
  Characters, Scenes, Problems, and measured Preview transport Performance views; the test app was
  closed after acceptance.
- Computer Use also verified Eiyashou highlighting, the compact gutter, middle-click tab closing,
  and sustained embedded Preview frames from the real ignored `projects/letsgal` project. The
  temporary review bundle, fixture, Editor process, and Preview child were removed or closed after
  acceptance.
