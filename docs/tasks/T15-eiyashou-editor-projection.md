# T15 — Eiyashou Editor projection and migration

## Status

Complete in the current integration tree. The Editor now classifies the configured Eiyashou source
boundary from `config.yaml`, keeps Text/Card on one source document, applies Card changes through
bounded ranges, preserves unknown syntax as read-only diagnostics, and requires a visible preview
plus confirmation before the only automatic migration: renaming already-valid Eiyashou `.txt`
sources to `.shou`. It never translates WebGAL/LetsGal input. This task does not authorize a
release, tag, commit, or push.

## Depends on

- T12 Editor Phase 2/3 source document and bounded authoring protocol.
- T13 Eiyashou typed adapter and lossless source inventory.
- T14 source-free Hakutaku release payload.

## Goal

Make `.shou`, `assets.yaml`, and `characters.yaml` first-class source-preserving documents in the
Editor, expose a Text/Card projection over one authoritative source document, surface Engine
validation diagnostics, and provide an explicit preview-before-apply migration command. Ordinary
open/save must never migrate or rewrite untouched source.

## Ownership

- `crates/editor/src/` for document classification, Text/Card projection, commands, diagnostics,
  and migration preview/application.
- `crates/authoring-protocol/` and the narrow Engine authoring host only if the existing typed
  diagnostics cannot express Eiyashou validation output.
- Focused editor/authoring tests and Eiyashou/project-state documentation.
- No publisher, renderer, compatibility-adapter, or package-format changes.

## Acceptance

- `.shou`, configured `assets.yaml`, and configured `characters.yaml` share the Phase 2 exact-text,
  revision, undo/redo, conflict-safe save, and recovery path.
- Text and Card modes reference the same `SourceDocument`; Card edits apply bounded source ranges
  and do not reconstruct a file from runtime actions or hidden JSON.
- Unknown syntax remains visible and read-only in Card mode with a Text-mode escape hatch.
- Eiyashou validation diagnostics reach the existing bounded authoring protocol and source view.
- Ordinary open/save is byte-preserving when unmodified.
- Migration is an explicit command with a diff preview and a separate apply action.
- LetsGal and compatibility JSON remain read-only; WebGAL semantics do not change.
