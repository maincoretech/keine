# Kēne collaboration guide

## Work

- Inspect branch, `git worktree list`, status and relevant diff before editing. Preserve unrelated changes.
- Read [architecture](dev/docs/architecture.md), [testing](dev/docs/testing.md) and the relevant current guide.
- User instructions override workflow defaults. No reset, clean, commit or push without authorization.
- Keep one owner per change; use isolated branches for explicitly delegated work unless the user requests main.
- Workers report files, validation, risks and interface changes; only integration owns shared files and main pushes.
- Shared files: AGENTS, README, dev/docs, Cargo manifests/lock, .cargo, root/bootstrap and native-smoke fixture.
- Do not accumulate dated plans, phase reports or duplicate specs. Update the relevant concise guide.

## Architecture

- Rust 2024 / Bevy 0.19.1; dependency flow `keine-core ← keine-loader ← keine`.
- Core, loader and authoring protocol remain Bevy-free. Runtime consumes typed Program/State.
- Editor-specific behavior stays in its adapter; ordered read-only mounts stay confined to their roots.
- 1920×1080 design space and viewport conversion have one owner; preserve scene/UI/dialog composition.
- Save v11 requires matching Program fingerprint; profile/history/gallery/settings stay outside slot rollback.
- Shipping user data uses stable project.id platform storage, never the read-only bundle directory.
- WebP / Ogg Opus and Hakutaku v1 are production formats; development compatibility cannot expand shipping.
- WebGAL is frozen: maintain safety and clear regressions, add no new semantics or parity goals.
- Keep identities/keys out of logs, commits, caches and unrelated child builds.
- No speculative abstraction, new dependency, theme/plugin/backend framework without demonstrated need.
- Original code/docs use Defold License 1.0; contributions use the same terms. Preserve third-party notices. New dependencies must pass the all-features cargo-deny policy; don't widen the allowlist or ignore private crates just to hide a failure. Assets/native library redistribution needs separate source and license evidence.

## Layout

```text
crates/core       typed schema and deterministic execution
crates/loader     capability adapters, content, compiled/store envelopes, diagnostics
crates/authoring  Editor–Engine protocol
crates/editor     native workbench and source editing
crates/media      bounded WebP decoding
src/runtime      bootstrap, host, input, script driving, Preview
src/scene        assets, media, sprites, effects
src/render       render-world pipelines
src/assets       shaders, audio, fonts, icons, branding
src/ui           fixed MainCore UI
src/storage      persistence
src/migration    publisher-only conversion/remapping
tests            integration, fixtures, bench, fuzz, video
dev              current docs and scripts
```

## Evidence

- Run fmt/check/clippy/workspace tests; run native-smoke validation for loader/project/publisher changes.
- The local macOS sandbox can block Unix socket creation in IPC tests (`Operation not permitted` / `PermissionDenied`). When this occurs, rerun the affected tests with sandbox escalation; do not skip them or treat the sandbox failure as a code regression.
- Check affected optional features. Serialization, unsafe/FFI, dependency and performance claims require primary-source verification.
- Performance changes require raw before/after commands and results; current summary belongs in dev/docs/testing.md.
- Native visual acceptance uses Computer Use directly. Do not add screenshot hooks, readiness protocols or GUI automation just for evidence.
- Report build, test, packaging and runtime acceptance separately; defer untested cases explicitly.
- Prefer necessary regression/boundary tests over implementation mirrors or new disposable test projects.
- Name files by responsibility and directory structure; keep Rust public APIs and compatibility fixture behavior stable.
