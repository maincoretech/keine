# T02 — LetsGal production-project acceptance

**Execution:** `parallel-safe`

## Goal

Validate the read-only LetsGal adapter against a representative full Studio project and close only
adapter-local correctness or loader-performance defects demonstrated by that run.

## Scope

- Run the existing local-sample acceptance and loader benchmark when the external sample exists.
- Check chapter ordering, cross-chapter flow, resource resolution, current 1.x blocks, diagnostics,
  and editor debug cursor behavior.
- Add minimal tracked synthetic fixtures for any demonstrated gap; never copy commercial assets.

## Non-goals

Studio extensions, source-project mutation, new core Actions, physical asset conversion, UI visual
redesign, or changing the canonical WebP/Opus release contract.

## Relevant files / modules

`crates/loader/src/adapter/editor/letsgal/`, `tests/letsgal_sample_acceptance.rs`,
`crates/loader/benches/letsgal_project.rs`, and the tracked LetsGal fixtures.

## Interfaces it may depend on

The integrated `Action`/`Program` schema, `AdaptedProject`, `ProjectDebugCursor`, `ParseReport`,
`ResourceRef`, `ContentMount`, and source-input limits.

## Ownership

- Owns `crates/loader/src/adapter/editor/letsgal/` and its module tests.
- Owns `tests/letsgal_sample_acceptance.rs`, `crates/loader/benches/letsgal_project.rs`, and
  `crates/loader/tests/fixtures/letsgal-1.8.0/` when a smaller fixture is necessary.
- Owns `dev/docs/architecture/08-letsgal-studio.md` and
  `dev/docs/acceptance/18-letsgal-studio-acceptance.md` for verified facts.

## Avoid modifying

Core schemas/runtime, WebGAL parser, Bevy runtime/UI, `projects/test-project/`, external
`projects/letsgal`, manifests, and integration-owned files.

## Required behavior

The adapter remains read-only, deterministic, source-bounded per file, and independent of Studio
extensions. Missing optional local samples skip cleanly; an explicitly configured missing sample
fails. Resource aliases and chapter order remain stable.

## Acceptance criteria

- The representative project compiles without unexplained warnings or missing static resources.
- Every fix has a minimal tracked regression independent of commercial media.
- Loader regressions are measured before implementation; no project-wide source cap is restored.

## Tests / validation

```text
cargo letsgal-test
cargo test -p keine-loader adapter::editor::letsgal
cargo validate projects/test-project
cargo letsgal-perf            # only for measured performance changes
```

## Dependencies on other tasks

None. Treat the existing core Action/flow schema as frozen for this task. If a demonstrated
LetsGal defect requires a new core interface, report it to the orchestrator instead of changing
core files.

## Completion report

Report sample source/version without redistributing it, coverage exercised, files, diagnostics,
tests/benchmarks, remaining unsupported blocks, and any requested core interface change.

## Worker startup prompt

Read `AGENTS.md`, `docs/PROJECT_STATE.md`, and `docs/tasks/T02-letsgal-production.md`.

Then inspect only the code relevant to this task. Implement within its ownership boundaries; do
not redesign unrelated systems. Run the specified validation. Report what changed, files changed,
tests run, remaining risks, and interface changes other threads must know about.
