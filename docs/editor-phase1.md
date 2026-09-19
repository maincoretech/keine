# Editor Phase 1 technical record

Status: **complete** on 2026-09-20. Phase 1 establishes the real editor workbench and remains a
read-only authoring shell. It does not launch the Engine and does not contain Preview.

## Workbench and project windows

`keine-editor` now owns one application process and a registry of project windows. A no-argument
launch opens one Empty Workbench with Open Folder and Open Recent. Opening a project creates one
`WorkspaceSession` and one native window; equivalent relative, canonical, and symlink paths use the
Phase 0 physical `ProjectKey` and focus the existing window instead of opening a duplicate. Separate
projects remain independent windows, and closing the final window terminates the process.

The initial project layout is:

- a fixed activity rail with Explorer selected, Open Folder, and Reset Layout;
- an upstream GPUI Dock host containing Explorer, two real read-only document tabs, Inspector, and
  Output;
- a compact top-left brand tile and an overlay host; the full tile is blue while saved and is
  reserved to turn red when a later writable phase has dirty documents;
- no repeated in-app project-title strip—the native title bar owns project identity.

The Dock renderer is a narrow presentation adapter around the upstream model. Singleton Explorer,
Inspector, and Output views use plain integrated card titles rather than fake tabs or close buttons.
Document groups and genuinely multi-view groups retain tabs with real close controls. The adapter
owns the editor-specific drop-zone geometry and borderless pale-blue indicator while retaining upstream
selection, layout mutation, panel lifecycle, and host-item drop callbacks. Tab insertion and
content merge/split targets are mutually exclusive, and content-target changes use a 140 ms
ease-out transition that follows the system reduced-motion preference. Closing a document view
does not delete or rewrite its source file.

Only a document tab or the text of a singleton View title starts a panel drag. Empty title-bar
space remains inert so it cannot duplicate content-area placement semantics.

Tab selection interpolates its fill and text colour over the same 140 ms policy. Closing a document
tab fades it out before removal from the Dock model; reduced-motion mode skips both delays. Explorer
keeps long paths on one line in a two-axis scroll viewport, with a low-contrast scrollbar and a
subtle right-edge fade indicating additional horizontal content.

Workspace discovery is bounded to 2,000 supported text files and 1 MiB per opened document. Hidden
directories plus `.git`, `target`, `node_modules`, `.keine`, and `saves` are skipped. The first two
documents are selected in deterministic authoring order, preferring `scripts/`, then `chapters/`,
then the project configuration.

## Persistence and secondary launches

All editor state remains below the platform app-data root recorded in the Phase 0 document:

```text
<keine-editor app-data>/
├─ recent-projects.json
├─ app-instance/
│  ├─ primary.lock
│  └─ endpoint.json
└─ projects/<workspace-id>/
   ├─ identity.json
   ├─ layout.json
   └─ recovery/
```

Recent projects, workspace identity, and schema-v1 Dock layout use temporary-file replacement.
Malformed, mismatched, or unsupported layout state falls back to the default composition. Reset
Layout removes only the editor-owned layout file and reinstalls the default Dock tree; project
files and document content are never written.

A process lock elects the primary editor instance. A secondary launch reads a bounded endpoint
envelope from app data, sends its requested paths over authenticated loopback TCP, waits for an
acknowledgement, and exits. The primary receives requests on a blocking event-driven listener and
routes them through the same project-window registry; there is no polling loop or resident daemon.

## Visual contract

The accepted Phase 1 surface is fully dark and compact. Lifted neutral blue-black surfaces carry
layout, normal text is grey rather than pure white, and `#BAEBFF` plus restrained derivatives near
the neutral white/black axis are the only brand accent. Green, amber, and red are reserved for
semantic state. Each major Dock view is a borderless 9 px radius tonal card. Every assigned slot
reserves a 2 px inset, producing an even 4 px gap between adjacent views without a painted divider;
the activity rail uses the same inset so all outer edges align. Tabs and the narrower activity rail
use the same borderless filled hierarchy. A tab target inserts at that tab, empty tab-bar space
appends, the broad content centre merges, and only the narrow nearest edge creates a split. Moving
between the tab bar and content always retires the previous target before showing the next one.
The result follows the supplied
current VS Code view geometry and Muspector information density without white cards, pink, repeated
headings, decorative prompts, an extra app toolbar, or a redundant bottom status strip.

## Boundaries

This phase intentionally does not implement writable documents, IME editing, undo/redo, recovery
drafts, structured VN cards, Engine discovery or launch, frame transport, Preview, asset management,
production build UI, themes, plugins, or extensions. The root Engine package still does not depend
on `keine-editor`, and plain `cargo build` continues to select only the root package.

## Acceptance evidence

Focused tests cover deterministic document discovery, app-data-only atomic persistence, corrupted
layout fallback, recent-project ordering and deduplication, Reset Layout source preservation,
physical project identity, duplicate routing, bounded instance messages, and a real secondary
process forwarding paths to the primary.

macOS visual acceptance opened `projects/test-project` and the local `projects/letsgal` fixture in
independent windows. The inspected real-project window showed Explorer files, two source-backed
document tabs with close controls, Inspector, Output, the compact borderless shell, and no Preview.
The test application was terminated after inspection and checked for residue.

The integrated validation gate is:

```text
cargo fmt --all --check
cargo build
cargo build -p keine-editor
cargo check --workspace
cargo clippy --workspace --all-targets
cargo test --workspace
```
