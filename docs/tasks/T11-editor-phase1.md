# T11 — Editor Phase 1 workbench and workspace persistence

**Execution:** `implementation`

## Goal

Turn the Phase 0 GPUI/Dock harness into the real Kēne Editor shell without starting Engine preview
or author-document mutation. The result opens real project folders into independent windows,
restores a compact VS Code-style workbench from editor-owned app data, and keeps project sources
read-only.

## Scope

- Replace dummy startup windows with an empty workbench and explicit Open Folder/Open Recent flows.
- Route command-line and secondary-launch project opens through one application-owned registry.
- Keep one window per physical `ProjectKey`; opening an existing project focuses its window.
- Add `WorkspaceSession`, real project discovery, and two read-only text document tabs sourced from
  the opened project.
- Build a fixed activity rail, Dock host, and overlay host. The native window title owns project
  identity; do not repeat it in an in-app top strip. Put the compact saved/dirty state on the
  activity-rail brand tile instead of reserving a bottom status strip: color the whole tile blue
  while saved and red while dirty rather than adding a badge dot.
- Provide Explorer, Inspector, and Output views alongside the real document tabs.
- Render singleton Explorer, Inspector, and Output names as integrated card titles, not closable
  tabs; reserve tab chrome for document groups and groups that actually contain multiple views.
- Persist schema-versioned per-project layout state and recent projects below editor app data using
  atomic replacement. Corrupt state falls back to the default layout.
- Provide Reset Layout without changing project files, document contents, or process state.
- Use a fully dark, compact tonal system derived from `#BAEBFF`; each Dock view is an independent
  borderless rounded card. Tabs and the narrower activity rail use the same borderless filled
  hierarchy. Use 9 px short radii, a 2 px per-slot inset that produces even 4 px gaps, no painted
  split divider, aligned outer edges and inner padding, real close controls on closable tabs, and
  no pink, pure-white component surfaces, large floating-card layout, or decorative prompts.

## Non-goals

Writable document models, IME editing, undo/redo, recovery drafts, Engine discovery/launch, IPC
authoring sessions, Preview, asset management, production build UI, themes, plugins, or extensions.

## Ownership

- `crates/editor/**`
- `docs/tasks/T11-editor-phase1.md`
- `docs/editor/phase1.md`
- `docs/PROJECT_STATE.md`
- `Cargo.toml` and `Cargo.lock` only for existing workspace dependencies required by persistence

## Required behavior

- No-argument launch opens one empty workbench and never shows a project picker.
- Opening project A and B creates independent windows; reopening A focuses the existing A window.
- Relative, canonical, and symlink paths preserve the Phase 0 identity behavior.
- A second application launch forwards requested folders to the primary instance and exits.
- Explorer and two document tabs display content from the real project without modifying it.
- Tab drops insert at the indicated tab, empty tab-bar space appends, content-centre drops merge,
  and only a narrow nearest-edge zone creates a split. Tab-bar and content targets are mutually
  exclusive, target movement is eased, and invalid drops are inert.
- Drop highlights use pale-blue fill without an outline. Tab selection and close transitions use
  restrained 140 ms ease-out motion and honor the system reduced-motion preference.
- Panel drag initiation is limited to document tabs and singleton title text; empty title-bar rows
  are inert.
- Explorer preserves long path visibility with horizontal scrolling, a low-contrast scrollbar,
  and a subtle right-edge overflow fade.
- Layout and recent-project state are written only below editor app data using temporary-file
  replacement, never inside the project.
- Reopening a project restores its persisted layout selection; a no-argument restart remains an
  empty workbench. Reset Layout restores the default Dock composition without touching source files.
- Closing project A leaves project B alive; closing the final window terminates the application.
- Preview does not exist and no Engine process is launched.

## Validation

```text
cargo fmt --all --check
cargo build
cargo build -p keine-editor
cargo check --workspace
cargo clippy --workspace --all-targets
cargo test --workspace
```

Focused Editor tests must cover state corruption fallback, atomic persistence location, recent
project ordering, real document discovery, duplicate routing, reset semantics, Dock interactions,
and the real secondary-launch control path. Visual acceptance must exercise two real project
folders in the running macOS app and confirm all test processes are closed afterward.

## Completion boundary

Stop after the Phase 1 workbench, persistence, routing, real read-only documents, and acceptance
evidence are complete. Do not implement Phase 2 document mutation or Phase 3/4 Engine/Preview work.
