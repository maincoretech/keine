# T04 — Rendering hotspot verification

**Execution:** `parallel-safe`

## Goal

Measure the current blur/stage/sprite pipelines on representative low-end hardware and simplify or
optimize only hotspots that remain visible with the current visual result held constant.

## Scope

- Separate CPU frame preparation, GPU blur, stage material, sprite sync, and idle behavior.
- Use current daily and stress benchmark timelines; add a narrow benchmark only when attribution
  is otherwise impossible.
- Preserve current blur strength, camera composition, effect appearance, and 60 Hz presentation.

## Non-goals

Visual redesign, quality presets, new renderer abstraction, hardware-specific fast paths without
fallback evidence, or unmeasured cache/state rewrites.

## Relevant files / modules

`src/render/`, `src/render.rs`, `src/assets/shaders/`, stage rendering files under `src/scene/`,
and `benches/runtime_hotspots.rs`.

## Interfaces it may depend on

`BlurRegion`, camera render layers, stage revision tokens, Bevy render extraction/prepare/queue,
and benchmark report semantics.

## Ownership

- Owns `src/render/`, `src/render.rs`, and `src/assets/shaders/`.
- Owns `src/scene/background.rs`, `sprites.rs`, `images.rs`, `effects/material.rs`, and
  `benches/runtime_hotspots.rs` for measured rendering changes.

## Avoid modifying

UI files, video, core/loader, runtime bootstrap/tick, `projects/test-project/`, manifests, shared
performance documentation, and integration-owned files.

## Required behavior

No quality reduction relative to the current screenshots. Stable scenes must not regain per-frame
entity/material rebuilds. Benchmark-only diagnostics must not execute in normal builds.

## Acceptance criteria

- Before/after results identify the actual bottleneck and include device/GPU, resolution, profile,
  median, P95/P99, and raw command.
- Each code change removes measurable work or complexity; no-change is acceptable when capped by
  VSync or driver time.
- Visual comparison shows no material regression.

## Tests / validation

```text
cargo test -p keine render::
cargo test -p keine scene::background
cargo test -p keine scene::sprites
cargo test -p keine scene::effects::material
cargo bench --bench runtime_hotspots
cargo perf projects/test-project
cargo fmt --all --check
cargo clippy --workspace --all-targets
```

## Dependencies on other tasks

None. T07 waits for its rendering interfaces and evidence to stabilize.

## Completion report

Report hardware, exact before/after data, hotspot attribution, files, tests, visual evidence,
remaining limits, and any benchmark format changes for the orchestrator to record.

## Worker startup prompt

Read `AGENTS.md`, `docs/PROJECT_STATE.md`, and `docs/tasks/T04-render-hotspots.md`.

Then inspect only the code relevant to this task. Implement within its ownership boundaries; do
not redesign unrelated systems. Run the specified validation. Report what changed, files changed,
tests run, remaining risks, and interface changes other threads must know about.
