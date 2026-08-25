# T03 — Cross-platform UI visual baseline

**Execution:** `parallel-safe`

## Goal

Establish repeatable evidence for current UI behavior on macOS, Windows, and Linux without turning
normal tests into window-opening or timing-sensitive tests.

## Scope

- Define the smallest reliable capture protocol for title, stage, dialog, backlog, save/load,
  config, Extra, and representative transitions.
- Cover 1x/HiDPI where available, 16:9, ultrawide, and tall windows.
- Fix only demonstrated UI/layout/input defects inside the owned UI files.
- Keep platform/GPU/window/DPI metadata with every accepted image or report.

## Non-goals

Theme systems, parity work for an external engine, renderer/shader optimization, product assets,
or making the default workspace test suite open a window.

## Relevant files / modules

`src/ui/`, `src/ui.rs`, `dev/docs/webgal-compatibility/visual-audit.md`, and existing UI unit tests.

## Interfaces it may depend on

`DesignViewport`, camera roles, `UiInputScope`, blur-region requests, elapsed-time animation, and
the existing frozen core/input contract.

## Ownership

- Owns `src/ui/`, `src/ui.rs`, and task-specific visual test support added below `tests/visual/`.
- Owns `dev/docs/webgal-compatibility/visual-audit.md`.
- May add a dedicated `.github/workflows/visual.yml`; it must not edit existing workflows.

## Avoid modifying

`src/render/`, shaders, scene synchronization, core/loader, `projects/test-project/`, existing CI
workflows, manifests, and integration-owned files.

## Required behavior

Normal gameplay and default tests remain unaffected. Capture waits use explicit visual readiness,
not arbitrary long sleeps. Logical layout and input hit testing remain resolution-independent.

## Acceptance criteria

- Every required screen/state has a reproducible capture step and recorded environment.
- At least one target run proves the protocol; missing target machines are reported, not simulated.
- Any checked-in golden has a documented update rule and deterministic dimensions.
- Existing UI transition and input-scope tests remain green.

## Tests / validation

```text
cargo test -p keine ui::
cargo test -p keine render::blur::tests::transformed_node_bounds_follow_button_scale
cargo fmt --all --check
cargo clippy --workspace --all-targets
# Run the documented visual capture on each available target platform.
```

## Dependencies on other tasks

None. WebGAL compatibility work is outside this task; it is independent of T02/T04/T05.

## Completion report

Report platform matrix, capture method, files, tests, accepted differences, missing hardware, and
whether any issue belongs to the renderer rather than UI ownership.

## Worker startup prompt

Read `AGENTS.md`, `docs/PROJECT_STATE.md`, and `docs/tasks/T03-ui-visual-baseline.md`.

Then inspect only the code relevant to this task. Implement within its ownership boundaries; do
not redesign unrelated systems. Run the specified validation. Report what changed, files changed,
tests run, remaining risks, and interface changes other threads must know about.
