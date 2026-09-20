# T14 — Exclude author sources from release packages

## Status

Complete in the current integration worktree. The publisher compiles from a private source staging
tree, then materializes a separate fail-closed release payload containing only runtime config, the
compiled Program, and mounted runtime assets. Native, WebGAL, and LetsGal archive-content tests
cover source exclusion. This task does not authorize a release, tag, or push.

## Depends on

- Compiled Program envelope v1 / IR schema v3 and the existing Hakutaku publisher pipeline.

## Goal

Make formal Hakutaku release packages contain the compiled Program and runtime-required resources,
but no authoring source. Native, WebGAL, and LetsGal projects must all execute from
`.keine/compiled/program.bin`; packaging must not retain script text, editor project files, source
manifests, extension configuration, or other adapter-owned author documents.

## Ownership

- `src/publisher.rs` and its focused tests.
- This task document for factual status updates.

Do not change parser/compiler semantics, loader APIs, project fixtures, Cargo manifests, release
workflow files, or T13-owned files. If the existing publisher boundary cannot identify runtime
resources without such a change, stop and report the required interface instead of widening scope.

## Acceptance

- A real Hakutaku archive built from each supported source-project shape contains
  `.keine/compiled/program.bin`, `config.yaml`, and every runtime-required asset.
- The archive contains no Native/WebGAL script source and no LetsGal authoring/project source.
- The packaged Program decodes under the current IR schema and its runtime assets remount from the
  generated config; the loader's compiled-scene tests continue to cover packaged scene selection.
- Source exclusion is capability/path driven and fails closed; it is not a filename blacklist that
  silently retains a new author-source family.
- Existing publisher tests and the normal repository validation gates remain green.
