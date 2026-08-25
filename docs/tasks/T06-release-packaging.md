# T06 — Publisher and release acceptance

**Execution:** `parallel-safe`

## Goal

Validate the complete resource-only pack and executable bundle workflows with temporary and stable
identity semantics, then fix only publisher/release defects exposed by that acceptance.

## Scope

- Verify `assets --pack`, `assets --remap`, normal bundle, benchmark bundle, incremental segment
  reuse, direct executable launch, runtime-library closure, and transaction recovery.
- Verify secret scope and environment cleanup without placing private material in logs or caches.
- Use temporary copies/output directories; do not dirty source projects.

## Non-goals

Changing Hakutaku v1 wire format, adding an updater/CDN, media decoder work, release marketing, or
making temporary identities update-compatible.

## Relevant files / modules

`src/compiler.rs`, `src/publisher.rs`, `src/resource_migration.rs`, publisher sections of
`src/runtime/cli.rs` and `package_benchmark.rs`, `dev/scripts/`, and release workflow.

## Interfaces it may depend on

`ProjectRoot`, `PersistenceRoot`, `CompiledProgramV1`, Hakutaku `Identity`/`PackOptions`, canonical
media report, shipping feature detection, and benchmark bundle marker/report.

## Ownership

- Owns `src/compiler.rs`, `src/publisher.rs`, `src/resource_migration.rs`, `dev/scripts/`, and the
  publisher-only code in `src/runtime/cli.rs` and `src/runtime/package_benchmark.rs`.
- Owns `.github/workflows/release.yml` and `dev/docs/architecture/06-hakutaku-packaging.md`.

## Avoid modifying

Hakutaku dependency revision, Cargo manifests/lock, setup-video/media-safety, core/loader schemas,
runtime bootstrap, `projects/test-project/`, and integration-owned files.

## Required behavior

`assets --pack` never builds an engine. `bundle` emits a directly runnable complete directory.
Publisher secrets are step-scoped, temporary identities are ephemeral, stable identities are
explicit, and child Cargo builds inherit no publisher secret/path. Failed transactions preserve a
runnable previous release without routine extra disk writes.

## Acceptance criteria

- Resource-only and full-bundle outputs have the documented, non-overlapping layouts.
- Repeated packaging reuses unchanged content and leaves no plaintext/secret artifact.
- Benchmark output is separate and normal bundles are unchanged.
- Acceptance uses disposable project copies and verifies source-tree cleanliness.

## Tests / validation

```text
cargo test --workspace --no-default-features --features publisher
cargo validate projects/test-project
cargo fmt --all --check
cargo clippy --workspace --all-targets
# Run assets/bundle acceptance in a disposable project/output directory.
```

## Dependencies on other tasks

None. T08 depends on the release layout and identity boundary remaining stable.

## Completion report

Report commands, output layouts, identity mode, files, tests, transaction/reuse evidence,
platforms not exercised, and requested shared manifest/CI changes.

## Worker startup prompt

Read `AGENTS.md`, `docs/PROJECT_STATE.md`, and `docs/tasks/T06-release-packaging.md`.

Then inspect only the code relevant to this task. Implement within its ownership boundaries; do
not redesign unrelated systems. Run the specified validation. Report what changed, files changed,
tests run, remaining risks, and interface changes other threads must know about.
