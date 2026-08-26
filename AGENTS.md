# Kēne collaboration guide

## Product and stack

Kēne is a native visual-novel engine for WebGAL scripts, native projects, and LetsGal Studio
projects. Rust 2024 is used throughout. `keine-core` owns the Bevy-free typed model and
deterministic execution; `keine-loader` owns content and format adapters; the root `keine`
package owns Bevy 0.19 runtime, rendering, UI, media, storage, and publishing tools.

WebP and Ogg Opus are the canonical production image/audio formats. Hakutaku v1 is the only
packaged-project format. Compatibility formats may remain available for development, but must
not silently expand a shipping build.

WebGAL support is a frozen compatibility layer, not an active parity target. Preserve its current
documented behavior, but add no new WebGAL semantics unless a future product decision explicitly
reopens the scope. Maintenance is limited to security, crashes, data loss, and clear regressions
caused by Kēne changes.

## Architectural constraints

- Dependencies flow `keine-core <- keine-loader <- keine`; core and loader must remain Bevy-free.
- Runtime consumes typed `Program`/`State`, never editor-specific JSON or package-format details.
- Content enters through ordered, read-only `ContentMount`/`ContentFile` sources. Later mounts
  override earlier mounts without escaping their roots.
- Adapters are grouped by capability: `asset`, `editor`, `script`, and `store`. Editor-specific
  behavior stays below its adapter.
- The design space is 1920x1080. Viewport/letterbox conversion has one owner; UI and scene code
  must not introduce independent physical-pixel coordinate systems.
- Scene, normal UI, and dialog cameras have fixed responsibilities. Rendering effects must
  preserve that composition order.
- Save v10 restores only against a matching Program fingerprint. Profile, read history, gallery,
  and settings remain outside slot rollback. Unsupported binary layouts fail closed.
- Shipping persistence lives in the platform user-data directory identified by stable
  `project.id`, never beside a read-only bundle.
- Publisher identities and embedded key material are never logged, committed, cached, or passed
  to child builds beyond the step that needs them.
- Do not add a theme/plugin framework, dynamic backend abstraction, compatibility layer, or new
  dependency without a demonstrated current use.

## Directory ownership

- `crates/core/`: action schema, state, expression evaluator, deterministic step semantics.
- `crates/loader/`: adapters, source confinement, compiled program/store envelopes, diagnostics.
- `src/runtime/`: bootstrap, host boundary, input/lifecycle, script-driving coordination.
- `src/scene/`: asset planning, audio/video, background, sprites, effects.
- `src/render/` and `src/assets/shaders/`: render-world pipelines and WGSL.
- `src/ui/`: fixed MainCore UI, overlays, screens, stage controls, shared UI mechanisms.
- `src/storage/`: persistence roots, saves, backups, profiles, history, gallery, settings.
- `src/compiler.rs`, `src/publisher.rs`, `src/resource_migration.rs`: publisher-only tooling.
- `projects/test-project/`: shared end-to-end acceptance project; integration-owned.

## Code and evidence quality

- Preserve unrelated worktree changes. Inspect `git status` and the relevant diff before editing.
- Prefer one clear owner for schemas, limits, coordinate transforms, and lifecycle decisions.
- Avoid scattered `unwrap`, magic numbers, repeated mappings, speculative abstraction, and tests
  that only restate implementation details.
- For serialization, dependency behavior, unsafe/FFI contracts, security, or performance claims,
  verify official documentation or primary sources before finalizing. Record the resulting
  invariant in code, tests, or the commit message.
- Performance changes require before/after measurements. Workers report raw commands and results;
  the integration thread updates `dev/docs/performance-baseline.md` to avoid parallel conflicts.
- Visual acceptance uses the Computer Use skill directly against the running application. Do not
  add engine screenshot hooks, readiness state, environment-variable protocols, or ad-hoc GUI
  automation solely to collect evidence. Product code changes require a demonstrated visual defect.

## Validation

Every integrated change must pass:

```text
cargo fmt --all --check
cargo check --workspace
cargo clippy --workspace --all-targets
cargo test --workspace
```

Run `cargo validate projects/test-project` for project, loader, adapter, compiler, or publishing
changes. Run the task-specific feature checks and benchmarks declared in `docs/tasks/` when the
affected code is not exercised by the default workspace suite.

## Multi-thread workflow

1. Read `AGENTS.md`, `docs/PROJECT_STATE.md`, and exactly one assigned `docs/tasks/TXX-*.md` before
   editing. Inspect only the task-relevant code after that.
2. Use an isolated worktree/branch named `codex/TXX-short-name`. One thread owns one task.
3. Modify only the task's declared ownership. If a required change crosses that boundary, stop and
   report the proposed interface change to the orchestrator; do not edit around the boundary.
4. `AGENTS.md`, `docs/PROJECT_STATE.md`, `docs/tasks/`, `Cargo.toml`, `Cargo.lock`, `.cargo/`,
   `src/lib.rs`, `src/runtime.rs`, `src/runtime/bootstrap.rs`, `README*`, `dev/docs/TODO.md`,
   `dev/docs/performance-baseline.md`, and `projects/test-project/` are integration-owned unless a
   task explicitly grants ownership.
5. Workers do not merge or push `main`. Commit a focused worker result only when requested, then
   report its commit, files, validation, risks, and interface effects.
6. The orchestrator reviews and integrates worker diffs, resolves shared-file changes, updates
   project state, runs the complete gate, and pushes `origin/main`.
7. Do not start a `depends-on` task before its dependency is integrated. Do not dispatch an
   `integration-only` task to an independent worker.
