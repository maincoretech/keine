# Editor Phase 2 technical record

Status: **complete** on 2026-09-20. Phase 2 adds the first writable authoring path without
turning compatibility formats or runtime actions into a write model.

## Writable document boundary

`keine-editor::document::DocumentManager` owns one authoritative `SourceDocument` per canonical
project-relative source. The writable set is deliberately narrow:

```text
config.yaml
scripts/*.txt
```

The manager rejects absolute paths, parent traversal, symlink escapes, files above 1 MiB, invalid
UTF-8, and every other extension or location. LetsGal and other JSON inputs remain discoverable and
read-only; JSON is not the native DSL and is not accepted by the writable document layer.

Each source keeps its exact text, disk stamp, revision, saved revision, selection, and recovery
state. Opening and closing an unchanged document does not serialize or rewrite it. Unknown source
commands and Unicode therefore survive a text edit without a model conversion. GPUI's editor owns
IME input plus document-local undo/redo; moving a Dock panel does not replace the editor entity or
its undo history. The gutter omits the unused folding column so line numbers reserve only their
necessary width.

## Save, conflict, and recovery

A save first hashes the current disk file. If it differs from the version opened by the editor,
the save fails with an external-modification conflict and leaves the outside change intact.
Otherwise the editor writes a same-directory temporary file, preserves permissions, flushes it,
renames it over the source, and syncs the containing directory where supported.

Dirty documents write a schema-versioned, bounded binary recovery draft below the editor's
per-workspace app-data recovery directory after a 350 ms debounce. A draft is restored only when
its source path and exact bounded base text still match disk; a changed base is reported as
conflicted rather than merged or overwritten. Confirmed saves remove the draft. Project directories never receive
layout, recovery, or IPC state.

The complete top-left brand tile is blue when all opened writable sources are saved and red while
any is dirty. `Cmd/Ctrl+S` saves all dirty sources. Closing a dirty project window presents Save and
Close, Close Without Saving, and Cancel. Output reports saves, recovery, conflicts, and Engine
diagnostics without adding a status strip. Inspector follows the active source line/column and
shows matching source diagnostics.

## Boundaries

Phase 2 is safe text authoring, not the final native DSL design. It does not add Card View, a
second document body, a LetsGal writer, runtime-derived writeback, a general JSON authoring format,
asset editing, or Preview. A future structured view must remain a projection of the same
source-preserving document and must prove unknown-syntax round trips before it may write.

## Evidence

Focused tests prove unchanged byte preservation, unknown command and Unicode round trips,
external-change rejection, recovery base matching, authoritative document reuse, and rejection of
compatibility JSON as writable input. macOS Computer Use acceptance exercised real text entry,
Unicode paste through the native input path, dirty/saved colour changes, recovery, save, selection,
and the close confirmation. The test window was closed and its process checked for residue.
