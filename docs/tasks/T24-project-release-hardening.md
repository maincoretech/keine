# T24 — Project P7 standalone authoring release and acceptance

## Status

macOS delivery ready; user acceptance pending. Windows/Linux acceptance is deferred by the user.
Do not mark the full cross-platform P7 acceptance complete until the platform evidence below exists.

The current authoring Preview uses a separate native Engine window. Older evidence below that
mentions embedded frames or shared transport is historical. The 2026-09-29 installed pair has
current GUI and native-window idle evidence; actual Preview active FPS remains a user acceptance
item. The standalone Engine performance sample does not measure Editor-to-Preview overhead.

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
- Run the complete workspace gate and `cargo validate tests/fixtures/native-smoke` after integration.

## Current evidence

- macOS has a minimal two-bundle packager. It publishes `Kēne Editor.app` and `Kēne Engine.app`
  together from prebuilt binaries, with no publisher identity or formal signing step. The packager
  now ad hoc signs and strictly verifies each assembled bundle: copying a linker-signed Mach-O
  into an app left its resources unsealed and failed `codesign --verify --deep --strict`.
- The 0.11.0 release Editor and Engine built with locked dependencies. Both packaged bundles
  report 0.11.0 and pass strict signature verification before and after ZIP archive extraction.
  With the older QA Editor closed, the packaged Editor opened the Eiyashou v1.1 test project;
  the sibling Engine opened a separate native Preview window and rendered its first dialogue.
  The compact one-line Key card and selected Key Inspector were checked at normal UI scale.
  After ZIP extraction to a separate directory, the Editor opened `native-smoke` and its sibling
  Engine rendered the scene; process paths confirmed both executables came from that extraction.
  The same extracted pair then opened the repository's LetsGal source project and direct Engine
  input advanced from a black narration frame to its illustrated town background and dialogue.
  A warm idle Engine sampled 0.0/0.1/0.1% CPU over three one-second `top` intervals. Active FPS
  remains open.
- The rebuilt 0.11.0 pair was installed in a fresh subdirectory of the user's Applications folder.
  Strict signature verification passed there, and the installed Editor opened an Eiyashou project,
  discovered its installed sibling Engine, and displayed the edited dialogue in a separate window.
  The running process paths confirmed both binaries came from that installation. A Preview quick
  save created only `preview-data/saves/slot_0.sav` and its thumbnail under the child scratch root.
  After the exact Engine child was killed, the root disappeared with the slot; Start opened a new
  Engine window and rendered the dialogue again. The new root had `0700` Unix permissions.
- In the installed Editor, closing the project window and reopening the app showed the empty
  workbench; opening the project from Recent restored its saved document tabs and Dock layout.
  System Zoom expanded the window across the current display and toggled back without clipping
  Explorer, source, Asset Preview, Inspector, or Output. The first geometry-persistence build
  exposed macOS Zoom animation frames and a title-bar size offset; the Editor now saves the last
  settled content bounds and the Zoom state. In the rebuilt installed app, an ordinary window
  reopened at the same 2360×1584 screenshot dimensions. Zoom reopened at 5120×2672, and Unzoom
  returned to 2360×1584. The ordinary window's saved 1180 px content width maps to 2360 physical
  screenshot pixels on this Retina display; this checks the Editor at the current 2× scale, not a
  separate 1× target. An attempted Control-Space switch followed by Pinyin keystrokes inserted
  Latin `nihao`; this run did not establish Chinese IME composition. The disposable project text
  was restored afterward.
- Editor discovery finds an Engine in a sibling app bundle, alongside the executable, in its own
  Resources, on `PATH`, or via `KEINE_ENGINE`. A focused test covers sibling-app discovery.
- The macOS 0.10.1 release Editor and Engine built with locked dependencies. A fresh two-bundle
  package passed plist, version, and binary-hash checks. The packaged Editor opened the native
  former native `projects/test-project`, and the independently packaged Engine rendered a real scene in Preview.
  Selecting a Block updated the frame and execution selection. Terminating only the Engine child
  produced an explicit failure; Start relaunched it and returned to Live. A process test also
  reopens the same project after a child crash. These checks do not prove installation from a
  distribution archive, 1×/HiDPI, IME, multiple-monitor behavior, or another OS.
- The exact-version and required-capability handshake runs before `OpenProject`. Focused tests
  cover mismatched protocol and missing capabilities. A real mixed pair was assembled from the
  0.11.0 Editor and retained 0.9.2 Engine. Its GUI showed `Preview failed · see Output`, while
  Output reported Editor protocol 6 versus Engine protocol 3 and `Update both apps together`.
  No Engine Preview window or project session opened. The initial mixed-pair attempt exposed that
  asynchronous handshake failures were invisible in the Editor; the visible status and Output
  notice were added and then checked in the rebuilt release Editor.
- Native authoring Preview now runs save/load and settings against a child-owned temporary
  `preview-data` root. Studio sync remains read-only and performance capture still disables
  persistence. Bevy's window-only UI focus did not hit controls rendered to Preview's Image
  target; the native Preview now applies the same stack/visibility/clip hit test to that target.
  The Editor and Engine now derive the same scratch directory from the launch token. After reaping
  a crashed Engine child, the Editor removes its exact directory; the process crash test writes a
  marker into that directory and verifies its removal. The Engine creates the root with owner-only
  Unix access, including on systems with a shared temporary parent. A simultaneous Editor and
  Engine crash can still leave data for the operating system's temporary-directory cleanup.
  In the packaged macOS GUI, Q.Save created `slot_0.sav`, Q.Load restored the earlier dialogue
  after advancing, and the normal Save screen created `slot_1.sav`. Both slots appeared only in
  the Engine child's temporary root; the source project's save-file hashes did not change.
  A save/load round-trip test and a real-GPU Preview-root test cover the underlying state.
  The rebuilt 0.10.1 Engine's GPU test passed at 179/170/170 ms to first frame and 86 ms from
  source edit to visible frame. These are not sustained throughput numbers.
- In the packaged 0.11.0 macOS GUI, Q.Save stored a temporary slot before an Editor Text Block
  change. The Engine displayed the edited dialogue immediately. Q.Load then showed the explicit
  script-version mismatch message and left the edited scene running. This is expected fingerprint
  rejection; the earlier slot was not silently loaded against changed source.
- A fresh packaged Editor GUI showed only the K brand on the empty workbench; Explorer appears
  after opening a project. A Recent click originally failed to open a project while opening ran
  synchronously during the row's click update; deferring it to the next app update repaired the
  path. A rebuilt packaged Editor opened the former `test-project` from its Recent row and showed Explorer.
- Format, workspace check, Clippy with `-D warnings`, all workspace tests, and former test-project
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

## 2026-09-29 macOS delivery and acceptance handoff

Work stayed in the main checkout at `535f7db74682` with the existing uncommitted changes preserved.
No new test project, worktree, commit, or push was made. The existing disposable Shou QA source was
restored byte-for-byte after the interactions below.

```text
P7
├── macOS 0.11.0 delivery — ready
│   ├── Locked release builds: Editor; Engine audio-all,ui-sounds,video-native
│   ├── ZIP → separate extraction → fresh Applications installation
│   │   ├── Both bundles pass codesign --verify --deep --strict
│   │   └── Installed Editor discovers and launches its installed sibling Engine
│   ├── Real LetsGal project: narration → illustrated dialogue → scene change
│   ├── Source ↔ Inspector: camera tween/duration and shake randomness
│   │   └── Save, close/reopen document, and compare source plus Inspector
│   ├── Editor selection seeks Engine; direct Engine input updates execution position
│   ├── Saved source revision updates the running native Engine
│   ├── text.retract: short, multiline Chinese, and Unicode-prefix cases advance normally
│   ├── Current Retina display: restored Editor layout and expanded Engine composition
│   └── Complete workspace gate and native-smoke validation pass
├── macOS user acceptance — pending by agreement
│   ├── Chinese IME composition, 1× display, multiple displays, ultrawide/tall windows
│   ├── Actual native Preview active FPS and isolated cold startup
│   ├── Broader T03/T21 visual interactions and subjective audio acceptance
│   └── Unicode case: QA font renders emoji as missing glyphs; emoji appearance unaccepted
└── Windows x64 / Linux — deferred by agreement
    └── Build/install/discovery, real-project Preview, input/DPI/recovery and metrics
```

The current host exposes only its built-in Retina display. Control-Space followed by Pinyin keys
inserted Latin text; this did not establish IME composition. Attempted edge drags did not establish
ultrawide/tall geometry. Opening another directory creates a separate workbench with its own Preview;
two Engine processes belonging to two open workbenches are expected.

The current workspace gate passed `cargo fmt --all --check`, `cargo check --workspace`,
`cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace`
(656 passed, zero failed, one ignored), followed by `cargo validate tests/fixtures/native-smoke`
(one scene, one action, one source, zero warnings). Protocol-7 mismatch/capability rejection tests
passed in that suite. These checks are current-source evidence, separate from the historical GUI
mixed-protocol demonstration above.

The installed Engine's existing `perf` command recorded three fresh-process startup samples and
a ten-second `10-04 blur family` sample: 60.0 FPS average, 55.3 FPS 1% low, P99 17.83 ms.
The real LetsGal native Preview's three idle CPU samples were 0.0/0.4/0.6%, with 451 MB RSS.
See `docs/performance-baseline.md` for raw settings and limits. No Preview FPS parity or performance
improvement is claimed.

Delivery locations:

```text
target/authoring/
├── keine-authoring-0.11.0-macos-aarch64-p7-20260929.zip
└── p7-20260929/                         build, gate, metric logs and hashes
/Users/shiftz/Applications/Kēne 0.11.0 P7 20260929/
├── Kēne Editor.app
└── Kēne Engine.app
```
