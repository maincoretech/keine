# T01 — WebGAL flow and input semantics

**Execution:** `parallel-safe`

## Goal

Close the high-impact deterministic WebGAL 4.6.2 gaps around `-continue`, flow completion,
duplicate parameters, conditional/value fallback, and typed user-input validation.

## Scope

- Verify behavior against pinned WebGAL 4.6.2 primary sources before changing semantics.
- Define one core representation for presentation completion versus click wait/`-next`.
- Make parser parameter precedence, `setVar` safe string fallback, `-when` diagnostics, and
  `getUserInput` default/rule behavior explicit and tested.
- Carry the behavior through `StepResult`, script-driver policy, and the existing input overlay.

## Non-goals

Advanced animation/filter payloads, renderer changes, theme/style execution, Steam integration,
Live2D/Spine/GIF, or broad JavaScript evaluation.

## Relevant files / modules

`crates/core/src/model/`, `crates/core/src/runtime/`,
`crates/loader/src/adapter/script/webgal.rs`, `src/runtime/script_driver.rs`,
`src/ui/overlays/user_input.rs`, and WebGAL compatibility tests/docs.

## Interfaces it may depend on

`Action`, `Flow`, `State`, `StepResult`, `ScriptDriveOutcome`, `SystemUiSlot`, and
`ProjectInitialState`. Preserve deterministic execution limits and save/rollback behavior.

## Ownership

- Owns the relevant files under `crates/core/src/model/` and `crates/core/src/runtime/`.
- Owns `crates/loader/src/adapter/script/webgal.rs`, `src/runtime/script_driver.rs`, and
  `src/ui/overlays/user_input.rs` for this task.
- Owns `dev/docs/webgal-compatibility/semantic-matrix.md` and `unsupported.md` only for evidence
  produced by this task.

## Avoid modifying

Rendering/scene systems, LetsGal adapter files, `src/runtime/tick.rs`, shared manifests,
`projects/test-project/`, and integration-owned files listed in `AGENTS.md`.

## Required behavior

Unsupported syntax remains bounded and diagnosable; no arbitrary JavaScript is executed. New
blocking/completion state must round-trip or be explicitly rejected by persistence safety. Existing
`-next`, Auto, Skip, execution-limit, and editor-seek behavior must not regress.

## Acceptance criteria

- Pinned upstream examples have parser and core tests.
- Flow completion is represented once and consumed by every resume source.
- Invalid/default user input has typed, deterministic behavior.
- Compatibility docs state exactly what became equivalent and what remains partial.

## Tests / validation

```text
cargo test -p keine-core
cargo test -p keine-loader adapter::script::webgal
cargo test -p keine runtime::script_driver
cargo test -p keine ui::overlays::user_input
cargo fmt --all --check
cargo clippy --workspace --all-targets
```

## Dependencies on other tasks

None. Its integrated core interface is a prerequisite for T02, T03, and T07.

## Completion report

Report changed semantics, files, upstream evidence, tests, persistence implications, remaining
compatibility gaps, and every public type/field changed.

## Worker startup prompt

Read `AGENTS.md`, `docs/PROJECT_STATE.md`, and `docs/tasks/T01-webgal-flow.md`.

Then inspect only the code relevant to this task. Implement within its ownership boundaries; do
not redesign unrelated systems. Run the specified validation. Report what changed, files changed,
tests run, remaining risks, and interface changes other threads must know about.
