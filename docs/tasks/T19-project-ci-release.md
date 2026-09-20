# T19 — Phase 6 project CI release chain

## Status

In progress in the integration tree.

## Depends on

- T14 compiled-only Hakutaku release content.
- T18 Eiyashou project migration and the canonical release command surface.

## Goal

Let a game project repository produce a formal Kēne release on a clean GitHub-hosted Linux runner
without requiring Rust on the author's computer. The build must use the exact Kēne commit that
defines the reusable workflow, install the pinned Rust toolchain and platform dependencies, build
the hardened Engine from source, restore the publisher identity only in runner-temporary storage,
and upload an inspectable artifact with provenance.

## Ownership

- `.github/workflows/project-release.yml`.
- `docs/project-ci-release.md` and the narrow README release documentation.
- `docs/PROJECT_STATE.md` and this task record.
- Focused workflow contract checks under `dev/scripts/` when needed.

No editor behavior, runtime semantics, package format, publisher cryptography, DSL grammar, or new
dependency is authorized. Phase 7 owns additional target platforms, installers, signing,
notarization, and representative cross-platform release acceptance.

## Required behavior

- The caller pins the reusable workflow with a full 40-character Kēne commit SHA. The called job
  checks out its own repository at `job.workflow_sha`, so workflow and Engine source cannot drift.
- The caller project and Kēne source occupy separate confined checkout roots. Project input cannot
  escape the caller checkout.
- Rust `1.97.1` and Linux platform dependencies are selected explicitly on `ubuntu-24.04`; the
  workflow never consumes a prebuilt author-machine Engine.
- `HAKUTAKU_IDENTITY_BASE64` is a required caller secret. Its decoded private file exists only
  below `RUNNER_TEMP`, is never cached or uploaded, and is removed in an `always()` cleanup step.
- Cache paths contain only Cargo downloads and Kēne build trees. Finished bundles, caller source,
  publisher material, and identity-derived files are excluded.
- The artifact name is validated, the bundle is verified as runnable, and provenance records the
  caller commit, exact Kēne commit, toolchain, runner image, release features, and workflow run.
- A real private project repository is the production acceptance caller. Local checks may prove
  the underlying bundle and security contracts but do not replace that clean-runner evidence.

## Evidence

- Validate workflow syntax and security invariants with the focused contract check.
- Reproduce the project/engine split from fresh local clones and run the formal bundle command.
- Run the complete workspace validation gate and `cargo validate projects/test-project`.
- Record any GitHub-hosted clean-runner run URL, Kēne SHA, artifact name, and secret contract here.

## External acceptance record

Pending until the reusable workflow is committed and invoked from a private game repository. Do
not mark this task complete based only on local validation or the same-repository smoke caller.

## Local acceptance record

On 2026-09-21, commit `9181a9a40d969b0feb55e81d37d46b7e4ebd626a` was cloned without
hardlinks into an empty temporary Engine checkout. `projects/test-project` was copied into a
separate Git repository with no publisher key, while its stable identity was copied to a private
temporary directory. The publisher-only runner completed a cold release build in 5m01s; the
content-selected hardened Engine completed its separate cold build in 4m58s. The resulting macOS
artifact ran `keine --version` outside its bundle directory, resolved only system frameworks, and
contained `keine`, `game.haku`, `data/*.taku`, and the icon, with no source/manifests or publisher
key. This proves the existing bundle boundary locally but is not the required Linux private-repo
clean-runner evidence.

Workflow syntax passed Ruby YAML parsing and actionlint 1.7.12. The actionlint invocation ignored
only its stale schema errors for the official `job.workflow_ref`, `job.workflow_repository`, and
`job.workflow_sha` properties documented by GitHub; all other checks ran normally. The complete
workspace format, check, clippy, test, and project-validation gates passed. The first sandboxed
test attempt could not create local authoring IPC; the identical complete test suite passed when
rerun outside that sandbox.
