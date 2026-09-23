# T24 — Project P7 standalone authoring release and acceptance

## Status

In progress. Do not mark P7 complete until the platform evidence below exists.

## Boundary

Publish the prebuilt Editor and authoring Engine as independent desktop applications without
moving the Engine into the Editor process. Keep the existing exact-version authoring protocol
handshake and capability gate. Do not add a packaging framework, signing, notarization, or a
cloud-build service.

Compatibility policy: the Editor requires the exact `PROTOCOL_VERSION` and every capability it
uses. The Engine's product version and build ID are diagnostic information, not a promise that
different builds interoperate. An incompatible pair fails at the handshake with no automatic
downgrade, migration, or second transport. This is the existing runtime behavior, not a new
versioning layer.

## Acceptance

- Standalone Editor and Engine artifacts can be installed separately and discovered on macOS,
  Windows x64, and Linux. Each platform has a launch and real-project Preview check.
- The Editor rejects a mismatched protocol version or missing required capability before opening
  project content; a mixed-version pair must fail clearly rather than silently degrading.
- Check 1× and HiDPI, IME, window restore, multiple monitors, crash recovery, and representative
  transitions on available real targets; record unavailable targets explicitly.
- Compare a repeatable packaged-build Preview startup, idle, and active-frame measurement with
  the existing baseline before claiming a performance change.
- Run the complete workspace gate and `cargo validate projects/test-project` after integration.

## Current evidence

- macOS has a minimal two-bundle packager. It publishes `Kēne Editor.app` and `Kēne Engine.app`
  together from prebuilt binaries, with no publisher identity or formal signing step.
- Editor discovery finds an Engine in a sibling app bundle, alongside the executable, in its own
  Resources, on `PATH`, or via `KEINE_ENGINE`. A focused test covers sibling-app discovery.
- The macOS release Editor and Engine built with locked dependencies; their stripped arm64
  executables are about 13 MiB and 45 MiB. The Engine links only system frameworks/libraries.
  A fresh two-bundle release package passed plist and executable checks; the packaged Engine
  reported version 0.9.2, and the packaged Editor launched with a local native project.
  This does not prove full Preview, installation, DPI, or recovery acceptance.
- Format, workspace check, Clippy with `-D warnings`, all workspace tests, and test-project
  validation passed. The loopback-dependent process tests required unsandboxed local IPC.
- The existing real-GPU Preview integration test passed twice. The cold first run observed
  1338/158/167 ms to first visible frame; the immediate repeat observed 158/174/151 ms, with
  zero overwritten frames in all cycles. These are not packaged-build throughput measurements.

Local macOS release packaging uses prebuilt binaries and a fresh output directory:

```text
cargo build --release --locked -p keine-editor --bin editor
cargo build --release --locked -p keine --bin keine --no-default-features --features audio-all,ui-sounds,video-native
bash dev/scripts/package-authoring-macos.sh target/release/editor target/release/keine /path/to/fresh-output
```

## Remaining evidence and work

- Build and install standalone Windows x64 and Linux artifacts; verify Engine discovery and
  project Preview on those systems. The current host has only the macOS Rust target.
- Collect T03's Windows/Linux, 1× DPI, ultrawide/tall-window, and transition evidence; finish
  T21's Card Editor visual acceptance.
- Verify packaged-build crash recovery, protocol-mismatch UX, and a repeatable performance
  baseline on representative hardware.
