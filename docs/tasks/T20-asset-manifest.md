# T20 — Asset manifest contract

**Execution:** `implementation`

**Status:** complete on 2026-09-22

## Goal

Fix the source and manifest contract needed by later File and Asset work without adding UI,
import conversion, drag and drop, or a second source model.

## Scope and ownership

- `crates/core/src/config.rs`: accept legacy string entries and `{ path, tags? }` entries.
- `crates/loader/src/loader.rs`: consume both forms and reject duplicate physical files within one
  resource type while allowing explicit sharing across different types.
- `crates/editor/src/authoring.rs`: expose tags and deterministic diagnostics for invalid paths,
  missing files, empty IDs, and same-type duplicate files.
- `crates/editor/src/document.rs`: prove manifest open/save remains byte preserving.
- `docs/PROJECT_STATE.md`: record the integrated contract.

## Validation

```text
cargo fmt --all --check
cargo check --workspace
cargo clippy --workspace --all-targets
cargo test --workspace
cargo validate projects/test-project
```

## Completion boundary

Legacy manifests still load without migration, object entries round-trip with tags, invalid or
escaping records fail closed, and opening or saving source does not rewrite unrelated bytes.

All required validation passed. The workspace test gate needed an unsandboxed rerun because its
existing authoring-process tests launch local Engine child processes.
