# T08 — Mobile platform contract

**Execution:** `integration-only`

## Goal

Freeze a minimal Android/iOS launcher and content-source contract before any mobile implementation
is dispatched.

## Scope

- Decide launcher crate/entry ownership, lifecycle handoff, platform persistence root, packaged
  Hakutaku segment source, input/safe-area behavior, media backend boundary, and release artifact.
- Identify shared API changes and split later Android/iOS implementation into conflict-free tasks.
- Verify decisions against current Bevy/winit and platform primary documentation.

## Non-goals

Implementing launchers in this task, copying the desktop binary model, promising mobile codecs,
adding a universal platform trait, or weakening desktop contracts for hypothetical reuse.

## Relevant files / modules

Root manifests, `src/lib.rs`, runtime bootstrap/platform, storage persistence, loader content source,
video backend selection, publisher/release assembly, and application icon assets.

## Interfaces it may depend on

The integrated T05 `ContentFile`/video backend contract and T06 release/publisher layout, plus
Bevy application lifecycle and platform user-data APIs.

## Ownership

No worker ownership is granted. The orchestrator owns the architecture decision, shared API diff,
`PROJECT_STATE` update, and creation of later platform task files.

## Avoid modifying

All product code until the decision and child-task ownership are approved. Do not bundle Android
and iOS implementation into one cross-cutting commit.

## Required behavior

Desktop builds, read-only packaged content, persistence isolation, key handling, logical design
space, and adapter neutrality remain unchanged. New platform dependencies are target-gated.

## Acceptance criteria

- The decision names one owner for each lifecycle/content/storage/media boundary.
- Android and iOS work can be dispatched without both changing the same shared files.
- Build, signing, packaging, and real-device validation requirements are explicit.

## Tests / validation

No product validation applies until implementation tasks exist. Validate all Markdown links and run
the workspace gate if any shared Rust or manifest file changes during integration.

## Dependencies on other tasks

T05 and T06 must be integrated. This remains in the orchestrator rather than a worker batch.

## Completion report

Report the architecture decision, primary sources, shared API changes, new child tasks, ownership
matrix, platform risks, and required real hardware.

## Worker startup prompt

Read `AGENTS.md`, `docs/PROJECT_STATE.md`, and `docs/tasks/T08-mobile-contract.md`.

This task is integration-only. The orchestrator should inspect the relevant boundaries, make no
product implementation before approval, and report the decision, files, validation, risks, and
child-task interfaces.
