# T03 — Cross-platform UI visual baseline

**Execution:** `user-acceptance`

## Goal

Establish repeatable evidence for current UI behavior on macOS, Windows, and Linux without turning
normal tests into window-opening or timing-sensitive tests.

## Scope

- The user directly inspects title, stage, dialog, backlog, save/load, config, Extra, and
  representative transitions on available real targets.
- Cover 1x/HiDPI where available, 16:9, ultrawide, and tall windows.
- Fix only demonstrated UI/layout/input defects inside the owned UI files.
- Keep platform/GPU/window/DPI metadata with every reported defect or accepted result.

## Non-goals

Theme systems, parity work for an external engine, renderer/shader optimization, product assets,
engine-native screenshot/capture infrastructure, ad-hoc mouse/keyboard automation, or making the
default workspace test suite open a window.

## Relevant files / modules

`src/ui/`, `src/ui.rs`, `dev/docs/webgal-compatibility/visual-audit.md`, and existing UI unit tests.

## Interfaces it may depend on

`DesignViewport`, camera roles, `UiInputScope`, blur-region requests, elapsed-time animation, and
the existing frozen core/input contract.

## Ownership

- Owns `src/ui/` and `src/ui.rs` only for fixes to defects observed during this task.
- Owns `dev/docs/webgal-compatibility/visual-audit.md` only for concise verified results.

## Avoid modifying

`src/render/`, shaders, scene synchronization, core/loader, `projects/test-project/`, existing CI
workflows, manifests, and integration-owned files.

## Required behavior

Normal gameplay and default tests remain unaffected. Manual acceptance is not a reason to add
runtime capture state. Logical layout and input hit testing remain resolution-independent. If no
defect is observed, make no product-code change.

## Acceptance criteria

- Available screen/state combinations are directly inspected by the user and their environment is
  recorded in the completion report.
- Missing target machines or states are reported, not simulated and not replaced by new tooling.
- Screenshots remain task evidence unless the orchestrator separately approves a golden baseline.
- Existing UI transition and input-scope tests remain green.

## Tests / validation

```text
cargo test -p keine ui::
cargo test -p keine render::blur::tests::transformed_node_bounds_follow_button_scale
cargo fmt --all --check
cargo clippy --workspace --all-targets
# The user inspects each available target directly.
```

## Dependencies on other tasks

None. WebGAL compatibility and renderer performance work are outside this task.

## Completion report

Report platform matrix, capture method, files, tests, accepted differences, missing hardware, and
whether any issue belongs to the renderer rather than UI ownership.

## Current evidence

macOS acceptance completed on 2026-08-27 at `1db8e15` using a shipping `.app` on Apple M5 Pro /
Metal. The title, stage, dialogs, Backlog, Save/Load, Config, Extra, continuation, system Zoom, and
native fullscreen showed no reproducible defect. Windows requires the user's remote credential;
Windows, Linux, 1× DPI, and frame-by-frame transition checks remain for manual coverage.

## User handoff

Report each observed defect with a screenshot, platform, resolution/DPI, the action that exposed
it, and expected behavior. The orchestrator will reproduce and implement only demonstrated fixes.
