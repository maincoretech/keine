# T21 — Card Editor source-preserving authoring

**Execution:** `implementation`

**Status:** Scene/Inspector integration implemented; live visual acceptance pending

## Goal

Make the `.shou` Blocks view a complete source-preserving writing surface before File import and
Asset management are introduced. Text remains the authority; Blocks, Inspector, and the Picker
operate on bounded source ranges rather than a second document model.

## Implemented boundary

- One document continuously renders all of its Scenes as animated, collapsible sections.
- Enter continues into a new Text row, committing an empty source-backed block when needed;
  Shift+Enter inserts a line break within the current block. New rows also work inside an empty Scene.
- Text bodies edit inline. Speaker, Voice, and Stable ID are edited in Inspector and explicit Stable
  IDs remain project-unique.
- Tab opens the full searchable Block Picker. Normal browsing uses a compact wrapping two-column
  layout, falling back to one column when narrow; customization keeps one row per item for its
  controls. Every Picker icon is included in the Editor asset source. Arrow/Tab/Enter and mouse
  selection work; favorites, category/item order, and hidden browse items persist as global Editor
  preferences. Hidden items remain reachable by search.
- Click, platform-modifier, Shift range selection, empty-space clearing, deletion, complete-node
  copy/paste, keyboard movement, and direct drag reorder use one selection model. Discrete sibling
  selections move as one group in source order; structurally unsafe moves fail closed.
- Text-to-Blocks selection resolves the deepest source node and scrolls it into view. Blocks-to-Text
  navigation restores the exact source line and column.
- Unknown source remains a compact read-only row at its real source position and navigates to Text.
- Choice, If / Else if / Else, and Loop use lightweight headers, indentation, and guide structure.
  Trailing Voice IDs remain part of their Text Block instead of becoming false Blocks.
- Non-Text cards show only compact summaries. Inspector owns their structured values, and multi-
  selection reports common versus mixed properties.
- Blocks keeps Scene creation and per-Scene rename/delete/move in the current document, using a
  compact header menu instead of a second Scene navigation page. Rename updates exact `goto` and
  `call` targets across indexed `.shou` sources; deletion confirms unresolved references.
- Supported native command rows use command-specific icons and short summaries. Inspector edits
  source-bounded parameters, including optional named arguments; unknown calls remain read-only.

## Performance and persistence

- No new dependency, background worker, duplicated source tree, or continuous animation loop was
  added.
- Picker preferences use one small schema-versioned global app-data file written by atomic replace;
  they never enter project files.
- Block and Inspector edits target bounded source ranges. Scene rename preflights related
  documents, then uses their existing undo, recovery, save, diagnostics, and Preview paths.

## Validation

```text
cargo fmt --all --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo validate projects/test-project
```

The integrated format, check, Clippy, and full workspace tests pass. Live Picker geometry still
needs acceptance against a build containing this revision; the existing QA window is older and
contains a recovery draft, so it was not replaced.
