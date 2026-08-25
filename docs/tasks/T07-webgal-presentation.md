# T07 — WebGAL authored presentation payloads

**Execution:** `depends-on: T01, T04`

## Goal

Implement the next coherent WebGAL presentation subset: authored animation tables/keyframes and
the documented complex curves, while preserving current bounded native effects.

## Scope

- Freeze the typed payload after T01's flow schema and T04's rendering evidence are integrated.
- Compile `setAnimation`/`setTempAnimation` authored data and supported complex curves into typed,
  immutable payloads with resource diagnostics.
- Carry duration/easing/inheritance/interruption semantics through core and scene projection.

## Non-goals

Arbitrary JavaScript/CSS, Live2D/Spine/GIF, theme runtime, every WebGAL filter, or speculative GPU
effects not required by a pinned example.

## Relevant files / modules

WebGAL parser, core Action/state/step animation types, stage/effect projection, showcase fixtures,
and compatibility evidence.

## Interfaces it may depend on

Integrated T01 flow/completion schema, T04 stage-material/blur interfaces, `TransformPatch`,
`StageAnimation`, persistence safety, resource reports, and elapsed-time sampling.

## Ownership

After dependencies integrate, this task exclusively owns the required WebGAL parser, core
animation model/runtime, `src/scene/effects/`, related showcase fixtures, and compatibility docs.

## Avoid modifying

UI shell/screens, video, storage, LetsGal adapter, publisher, shared manifests, test project, and
integration-owned files. Do not begin while T01 or T04 still owns overlapping files.

## Required behavior

Payloads are typed, bounded, deterministic, frame-rate independent, and safe to interrupt. Save or
rollback either resumes exact logical state or rejects active unsafe native timelines explicitly.
Unknown files/effects produce source-located diagnostics rather than generic fallback success.

## Acceptance criteria

- A pinned upstream fixture covers parsing, defaults, inheritance, interruption, and final state.
- Core tests are renderer-independent; scene tests prove projection and visual key states.
- Compatibility status changes only for the exact verified subset.

## Tests / validation

```text
cargo test -p keine-core
cargo test -p keine-loader adapter::script::webgal
cargo test -p keine scene::effects
cargo test --test showcase_coverage
cargo validate projects/test-project
```

## Dependencies on other tasks

T01 and T04 must be integrated first. It may run in parallel with T02 and T03 afterward.

## Completion report

Report supported upstream subset, payload/API changes, files, tests, visual evidence, persistence
behavior, rejected inputs, and remaining presentation gaps.

## Worker startup prompt

Read `AGENTS.md`, `docs/PROJECT_STATE.md`, and `docs/tasks/T07-webgal-presentation.md`.

Then inspect only the code relevant to this task. Implement within its ownership boundaries; do
not redesign unrelated systems. Run the specified validation. Report what changed, files changed,
tests run, remaining risks, and interface changes other threads must know about.
