# T21 — Card Editor Phase 1

**Execution:** `implementation`

**Status:** implementation complete on 2026-09-22; live visual acceptance pending

## Goal

Make the `.shou` Blocks view a complete source-preserving writing surface before File import and
Asset management are introduced. Text remains the authority; Blocks, Inspector, and the Picker
operate on bounded source ranges rather than a second document model.

## Implemented boundary

- One document continuously renders all of its Scenes as animated, collapsible sections.
- Enter creates a source-free draft Text row and writes the first valid narration only after input,
  including inside an empty Scene.
- Text bodies edit inline. Speaker, Voice, and Stable ID are edited in Inspector and explicit Stable
  IDs remain project-unique.
- Tab opens the full searchable Block Picker. Arrow/Tab/Enter and mouse selection work; favorites,
  category/item order, and hidden browse items persist as global Editor preferences. Hidden items
  remain reachable by search.
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
- Blocks view does not add Scene create, rename, delete, move, or nested Scene navigation controls.

## Performance and persistence

- No new dependency, background worker, duplicated source tree, or continuous animation loop was
  added.
- Picker preferences use one small schema-versioned global app-data file written by atomic replace;
  they never enter project files.
- Source edits remain single bounded replacements and flow through the existing document undo,
  recovery, save, diagnostics, and Preview paths.

## Validation

```text
cargo fmt --all --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo validate projects/test-project
```

The full workspace gate passes. Live Editor acceptance remains pending.
