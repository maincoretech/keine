# T13 — Eiyashou adapter foundation

## Status

Complete in the current integration tree. Eiyashou now has a dedicated lossless `.shou` adapter,
strict typed expressions and interpolation, variables and homogeneous lists, structured control
flow, project manifests, stable source identities, plain-text dialogue, typed BGM playback mode,
real crossfade state, and strict schema upgrades. Editor projection and explicit migration are
owned separately by T15. This task does not authorize a release, tag, or push.

## Depends on

- Editor Phase 2 source-preserving document state.
- Editor Phase 3 bounded authoring protocol.
- The accepted decisions in `docs/EIYASHOU_LANGUAGE_REFERENCE_v1.md`.

## Goal

Add a dedicated Bevy-free Rust `adapter::script::native` implementation for Eiyashou `.shou`
sources. The adapter must own lexing, lossless source structure, parsing, type/flow validation,
and lowering to typed runtime-neutral core IR. It must not pass Native expressions through the
legacy WebGAL evaluator and must not use JSON as an authoring or sidecar format.

## Ownership

- `crates/core/src/model/` and `crates/core/src/runtime/` for the smallest typed IR and deterministic
  execution additions required by the accepted Native semantics.
- `crates/loader/src/adapter/script/native.rs`, `crates/loader/src/language.rs`, and
  `crates/loader/src/loader/scenes.rs` for the Native parser/adapter and multi-scene source boundary.
- `crates/loader/src/report.rs` and focused tests needed by the adapter.
- `crates/core/src/config.rs` only for a single owner of Native script version/entry configuration.
- `crates/core/src/model/state.rs`, `src/scene.rs`, and the narrow bootstrap/preview entry-selection
  call sites needed to honor `script.entry` without changing compatibility-adapter defaults.
- `src/compiler.rs` only for fail-closed validation that the configured Native entry exists.
- `crates/loader/src/compiled.rs` only when a new typed Native action requires an IR schema bump.
- `src/runtime/script_driver.rs` and the narrow tooling replay match in `src/runtime/tick.rs` only
  to surface typed core runtime errors without silently continuing.
- `docs/EIYASHOU_LANGUAGE_REFERENCE_v1.md`, `docs/PROJECT_STATE.md`, and this task document.

Do not modify Editor UI, Preview frame transport, publisher behavior, release workflows, WebGAL
syntax, or LetsGal source semantics in this task.

## Fixed decisions

- The public language name is Eiyashou. The existing adapter/configuration identifier remains exactly
  `keine`; alternate adapter spellings are invalid.
- Source files use `.shou` and may contain multiple project-global `scene` declarations.
- `config.yaml` keeps `adapter.script: keine`; Native version and entry are subordinate script
  configuration, not a second project-config owner.
- There is no generic `setting(...)` command in v1.
- Video v1 is fullscreen, non-looping, blocking playback with optional `skippable`; author-facing
  `loop` and `wait` parameters are absent.
- `/${` encodes a literal `${` in a string; other `/` and `$` characters remain literal.
- Every possible control-flow cycle must cross a yielding action. The compiler rejects a cycle
  that can spin without dialogue, choice, wait, or blocking video. Core's deterministic forward
  action limit remains a fail-closed runtime backstop.
- Mutating list operations are statements. `pop` writes to an explicit target statement instead
  of being a general expression, so evaluation order and once-only initialization remain clear.

## Acceptance

- A `.shou` file with multiple scenes parses deterministically and retains token/trivia ranges.
- Duplicate scenes, unresolved scenes, recursive calls, type errors, possibly uninitialized reads,
  and non-yielding cycles produce source-located errors and no runnable Program.
- Native boolean conditions are strict and short-circuiting; numeric/list/string behavior does
  not inherit the legacy evaluator's truthiness or coercions.
- Dialogue, choice, control flow, variables/lists, background/sprite/hide/move, wait, audio, and
  blocking video lower through typed core structures or explicit diagnostics.
- `/${` round-trips and displays as literal `${`; ordinary `${expr}` remains interpolation.
- WebGAL and LetsGal tests remain unchanged in behavior.
- Run the repository validation gate plus `cargo validate projects/test-project` when the fixture
  is migrated or extended for this adapter.
