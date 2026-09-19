# T02 — LetsGal Studio 2.0 sync

**Execution:** integration-owned in isolated `codex/T02-letsgal-200` worktree.

## Goal and scope

Compare the official 2.0.0 release, installed SDK, and real project schema with Kēne's current
1.20 adapter. Implement only demonstrated runtime/content deltas, preserving the read-only adapter
and typed core boundary. Studio-only authoring, cloud/community, recorder, build UI, extension
development, Spine, Live2D, and frozen WebGAL semantics remain outside scope.

## Ownership

Owns the LetsGal adapter/model/tests plus narrowly required core/runtime/render support and the
LetsGal architecture/acceptance documentation. Keep Save v10 and Program v1 wire formats stable,
preserve fixed camera composition and no-effect shader specialization, and do not modify the
shared test project or external commercial project sources.

## Validation

Run workspace fmt/check/clippy/tests, targeted LetsGal tests, `cargo letsgal-test`, and
`cargo validate projects/test-project`. Use real Studio/project evidence for schema and visual
behavior; do not add screenshot/readiness hooks. Do not commit or push without explicit request.
