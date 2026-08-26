# Kēne context handoff

Updated: 2026-08-27

## Start here

Use `/Users/shiftz/dev/keine` as the canonical checkout and work directly on `main` unless a new,
explicitly delegated task requires an isolated worktree. Read `AGENTS.md` and
`docs/PROJECT_STATE.md` before changing code. The repository should be clean at handoff; verify
with `git status --short` and inspect the latest commit before continuing.

## Latest integrated work

- Save/Load no longer rebuilds a newly written or overwritten slot before its asynchronous
  screenshot is available. The screenshot callback now refreshes metadata and preview together;
  if no primary window exists, the card still refreshes without a preview.
- Save-slot timestamps use a slightly larger font and an additional three logical pixels of left
  padding.
- Dismissing a modal keeps the reactive UI loop alive for one additional frame so title/menu
  regional blur is rendered before the event loop sleeps.
- T02 production acceptance and T04 rendering-hotspot work are closed. T03 has passed the
  available macOS visual pass; cross-platform visual evidence remains user acceptance rather than
  an autonomous worker task.

## Manual acceptance still needed

Run:

```text
cargo dev projects/test-project
```

Then verify:

1. Save into an empty slot: the old empty card remains briefly, then date, details, and screenshot
   appear together.
2. Overwrite an occupied slot: the old card remains until the replacement screenshot is ready;
   new metadata and image replace it together.
3. On the title screen, open the Exit confirmation and choose No: title-button blur must be present
   immediately after the dialog disappears, without waiting for mouse movement.
4. Confirm the larger timestamp remains vertically centred and does not collide with the slot
   number at the supported window shapes.

The code-level UI suite, workspace check, and Clippy passed before handoff. Manual acceptance is
required because these are timing and visual-composition changes.

## Product boundaries to preserve

- WebGAL support is frozen at its documented compatibility boundary; do not expand it without a
  new product decision.
- WebP, Ogg Opus, and Hakutaku are the canonical shipping media/package paths.
- Do not add screenshot automation or UI-only protocol code as a substitute for direct visual
  acceptance.
- Do not commit or push unrelated small follow-ups automatically. Group related work and ask the
  user when a commit boundary is appropriate, unless the user explicitly requests integration.

## Suggested next-context prompt

Read `AGENTS.md`, `docs/PROJECT_STATE.md`, and `docs/CONTEXT_HANDOFF.md`. Inspect current `main` and
continue from the manual acceptance section. Preserve existing product boundaries and do not reopen
closed WebGAL work.
