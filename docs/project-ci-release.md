# Project CI release

An editable Kēne project does not require Rust on the author's computer when it is opened with the
prebuilt Editor and Engine. A formal player release is different: project CI checks out an exact
Kēne revision, installs its pinned Rust toolchain and platform libraries, rebuilds the hardened
Engine from source, and bundles the encrypted Hakutaku payload.

Phase 6 provides this path for GitHub-hosted Linux x64 runners. Additional platforms, installers,
signing, and notarization belong to Phase 7.

## 1. Create the publisher secret

Generate one publisher identity on a trusted machine and keep its original private file backed up
outside the game repository:

```bash
cargo assets --pack /path/to/project
openssl base64 -A -in /path/to/project/.keine/publisher.hakutaku-key
```

Add the printed value to the game repository as an Actions repository secret named
`HAKUTAKU_IDENTITY_BASE64`. Never commit either the private file or its base64 representation. Keep
the same identity for every update in one release lineage; changing it makes the new package a
different publisher lineage.

## 2. Pin the Kēne workflow

Add `.github/workflows/release.yml` to the game repository. Replace
`<40-character-keine-commit>` with the complete reviewed Kēne commit SHA; a branch, tag, or short
SHA is rejected.

```yaml
name: Release

on:
  workflow_dispatch:
  push:
    tags:
      - "v*"

permissions:
  contents: read

jobs:
  release:
    uses: maincoretech/keine/.github/workflows/project-release.yml@<40-character-keine-commit>
    with:
      project-path: .
      artifact-name: my-game-linux-x64
      retention-days: 14
    secrets:
      HAKUTAKU_IDENTITY_BASE64: ${{ secrets.HAKUTAKU_IDENTITY_BASE64 }}
```

The single SHA in `uses:` owns both the reusable workflow and the Engine source. The called job
checks out Kēne at GitHub's resolved `job.workflow_sha` and verifies it against the requested
reference, so a workflow/Engine version mismatch fails before any publisher material is restored.

## 3. Download and verify

Run the workflow manually or push a matching tag. The `my-game-linux-x64` artifact contains the
runnable `keine` executable, `game.haku`, `data/`, and `KEINE-PROVENANCE.txt`. The provenance file
records the game commit, exact Kēne commit, Rust version, runner, selected release features, and
workflow run URL; it contains no publisher secret.

The CI cache contains only Cargo downloads and Kēne build trees. The project checkout, completed
bundle, and publisher identity are outside the cache. The decoded identity is created with private
permissions below `RUNNER_TEMP` and removed by an unconditional cleanup step before artifact
upload.

## Failure contract

The release fails closed when the Kēne reference is mutable, the project path escapes its checkout,
the secret is missing or empty, production media validation fails, the hardened Engine build
fails, or the executable has unresolved Linux libraries. A failed run does not upload a partial
release.

