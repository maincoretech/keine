# T22 — File and asset import

## Goal

Make Explorer a normal workspace file tree and accept external drops without turning File into a
second asset editor.

## Scope

- Discover ordinary files and folders, with open, create, rename, move, copy, delete, internal
  drag, and Reveal actions.
- Copy ordinary external files into the selected folder.
- Accept only canonical WebP, Ogg Opus, and MP4/M4V media in this task. Validate contents, derive
  an identifier from the filename, and register the result in the configured asset manifest.
- Keep manifest paths synchronized when mapped files or directories move.
- Use one aggregate progress row and one summary toast for a batch.

## Explicit deferrals

- No lossy conversion, source retention, import review, encoder controls, manual Register action,
  or asset search UI.
- Deleting a mapped asset remains owned by the later Asset lifecycle phase and is blocked here.

## Ownership

- `crates/editor/src/workspace.rs`
- `crates/editor/src/file_ops.rs`
- Explorer-specific code in `crates/editor/src/app.rs`
- Focused tests in the files above

## Acceptance

- Invalid or non-canonical media creates neither a destination file nor a manifest entry.
- Canonical resource import produces one file and one manifest entry.
- Mapped move/rename updates the manifest transactionally.
- External batches expose aggregate progress and only one final toast.
- File resources cannot be dropped into the content editor.
