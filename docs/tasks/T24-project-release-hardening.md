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
- The macOS 0.10.1 release Editor and Engine built with locked dependencies. A fresh two-bundle
  package passed plist, version, and binary-hash checks. The packaged Editor opened the native
  `projects/test-project`, and the independently packaged Engine rendered a real scene in Preview.
  Selecting a Block updated the frame and execution selection. Terminating only the Engine child
  produced an explicit failure; Start relaunched it and returned to Live. A process test also
  reopens the same project after a child crash. These checks do not prove installation from a
  distribution archive, 1×/HiDPI, IME, multiple-monitor behavior, or another OS.
- The exact-version and required-capability handshake runs before `OpenProject`. Focused tests
  cover mismatched protocol and missing capabilities, and the Editor now tells the user to update
  both apps together. A mixed-bundle GUI check is still required.
- Native authoring Preview now runs save/load and settings against a child-owned temporary
  `preview-data` root. Studio sync remains read-only and performance capture still disables
  persistence. Bevy's window-only UI focus did not hit controls rendered to Preview's Image
  target; the native Preview now applies the same stack/visibility/clip hit test to that target.
  In the packaged macOS GUI, Q.Save created `slot_0.sav`, Q.Load restored the earlier dialogue
  after advancing, and the normal Save screen created `slot_1.sav`. Both slots appeared only in
  the Engine child's temporary root; the source project's save-file hashes did not change.
  A save/load round-trip test and a real-GPU Preview-root test cover the underlying state.
  The rebuilt 0.10.1 Engine's GPU test passed at 179/170/170 ms to first frame and 86 ms from
  source edit to visible frame. These are not sustained throughput numbers.
- A fresh packaged Editor GUI showed only the K brand on the empty workbench; Explorer appears
  after opening a project. A Recent click originally failed to open a project while opening ran
  synchronously during the row's click update; deferring it to the next app update repaired the
  path. A rebuilt packaged Editor opened `test-project` from its Recent row and showed Explorer.
- Format, workspace check, Clippy with `-D warnings`, all workspace tests, and test-project
  validation passed. The loopback-dependent process tests required unsandboxed local IPC.
- The existing real-GPU Preview integration test passed twice. The cold first run observed
  1338/158/167 ms to first visible frame; the immediate repeat observed 158/174/151 ms, with
  zero overwritten frames in all cycles. Against a byte-identical copy of the packaged 0.10.1
  Engine, a warm three-cycle acceptance observed 165/162/160 ms and zero overwritten frames;
  a source-edit-to-visible-frame check observed 86 ms. On this host, the packaged idle Engine
  sampled 0.0/0.8/1.5% CPU over three one-second `top` samples. See
  `docs/performance-baseline.md` for method and limitations.

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
- Verify the mixed-bundle protocol-mismatch GUI, installation, 1×/HiDPI, IME, window restore,
  multiple monitors, and representative transitions on real targets. Repeat startup and active
  throughput measurements on Windows/Linux and compare an isolated macOS cold start. Exercise
  save/load behavior after source edits, and settle crash-leftover temporary preview data.
