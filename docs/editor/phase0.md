# Editor Phase 0 technical record

Status: **complete** on 2026-09-20. This record describes the Phase 0 code and experiments
integrated directly on `main`; it is not a claim that the editor or preview path is ready to ship.

## Dependency and workspace decision

Checked against crates.io on 2026-09-20. The editor uses exactly `gpui-kit = 0.6.4`, the latest
stable GPUI Kit release at the time of the check. GPUI Kit is the application-facing facade and
pins its matching GPUI family; `Cargo.lock` resolves `gpui-pre` and `gpui-pre-platform` to 0.3.5
and the Dock implementation in `gpui-base`/`gpui-component` to 0.6.4.

The choice avoids inventing a version for the unpublished `gpui_platform` crate name or mixing
independently moving GPUI packages. Upstream references:

- <https://crates.io/crates/gpui-kit/0.6.4>
- <https://github.com/longbridge/gpui-kit>
- <https://crates.io/crates/gpui-base/0.6.4>

`crates/editor` is the `keine-editor` package and binary. It is a workspace member through the
existing `crates/*` glob. The root `default-members = ["."]` remains unchanged, and the root
`keine` dependency tree contains no GPUI packages. Therefore plain `cargo build` still selects
only the engine package; the editor is built explicitly with `cargo build -p keine-editor`.

## GPUI and Dock spike

The executable opens two independent project windows and keeps one window per `ProjectKey`.
Opening an already registered project focuses its existing window instead of creating another.
Closing the last project window exits the application on every platform, including macOS, so test
runs and ordinary editor sessions do not leave a background process behind.
Each window contains real GPUI Kit Dock panels with:

- an Explorer pane;
- a tab group with two script tabs;
- an Inspector pane in a vertical split;
- draggable/reorderable tabs and split drop targets owned by the upstream Dock model.

The automated Dock test performs a tab reorder, a split move, and a drop against a stale target.
The stale drop must leave the layout unchanged. Manual macOS inspection confirmed both project
windows render, the second remains after the first closes, and script-tab activation changes the
visible panel.

### Visual direction recorded for Phase 1

The Phase 0 executable is a technical harness, not a visual acceptance candidate. Full workbench
composition belongs to Phase 1 under the final architecture's completion boundary. Phase 1 must
follow `UI_GUIDE.md` and the supplied current VS Code and Muspector references rather than
inventing a separate visual language:

- neutral near-black surfaces carry layout and elevation;
- neutral grey, not pure white, carries normal text;
- `#A3E4FF` is the single brand primary; hover, active, container, selection, and weak accents are
  tonal derivatives of that same blue-cyan hue;
- green, amber, and red appear only for real success, warning, and danger states;
- no white cards or controls and no pink accent are used;
- colour identifies state or role instead of decorating every surface.
- the workbench uses VS Code's compact, short-radius, one-pixel outlined views: activity rail,
  Explorer, document tabs/editor, Inspector, and status bar remain visibly related without being
  turned into floating cards;
- the Muspector reference governs information density, hierarchy, and restrained prompting;
  empty decoration, repeated headings, and tutorial copy are not substitutes for real state.

The general control radius is 6 px and large overlays use 10 px. Motion follows the component
library's semantic 120/180/280 ms fast/normal/slow scale: fade for visibility changes, eased
movement for spatial changes, and no animation for keyboard input or continuous editing. Reduced
motion must collapse decorative fades and movement. Tooltips, banners, and status copy appear only
when they add an action or explain a real state; panels do not repeat their title or show tutorial
copy by default.

Phase 0 may exercise these tokens to ensure GPUI can render them, but it must not present its dummy
panels as the final editor design. Phase 1 screens must reuse the semantic tokens rather than
introducing local colour constants.

## Project identity and persistence

`ProjectKey` first resolves an existing path to its physical filesystem identity. On Unix this is
the device/inode pair, so relative, absolute, canonical, and symlink paths for the same directory
route to the same editor window. Missing paths fall back to a lexically normalized absolute path.
On Windows, canonical path identity is case-folded but physical file identity is not yet queried,
so hard-linked directory aliases are not collapsed. This is a recorded cross-platform limitation,
not a reason to add an unreviewed Win32 dependency during Phase 0; Phase 1 must verify the behavior
on Windows before treating hard-link routing as supported.

Editor persistence never falls back to the project directory:

| Platform | Editor app-data root |
| --- | --- |
| macOS | `~/Library/Application Support/moe.maincore.keine-editor` |
| Windows | `%LOCALAPPDATA%/Kēne/Editor`, then `%APPDATA%` |
| Linux/Unix | `$XDG_DATA_HOME/keine/editor`, then `~/.local/share/keine/editor` |

Relative environment paths fail closed. These paths are editor-owned and separate from the
engine's project-id-based shipping persistence.

## Process boundary and control IPC

The Phase 0 control spike uses a real child process over an authenticated loopback TCP connection.
The bounded, versioned line protocol proves:

1. `HELLO <version> <launch-token>` negotiation;
2. request/response correlation with `PING`/`PONG`;
3. clean `SHUTDOWN`/`BYE` termination;
4. rejection of incompatible hello messages and control lines over 4 KiB.

The current launch token is test-only and is not a production secret generator. The spike does
not yet implement project commands, error events, crash recovery, or a production frame channel.

## Preview and offscreen feasibility audit

Current Kēne rendering has three ordered cameras: scene, normal UI, and dialog. The existing save
thumbnail path creates a separate scene-only camera, so it does **not** prove full-composition
preview capture. The Phase 0 example instead starts Bevy without `WinitPlugin` or a primary window,
targets all three ordered cameras at one `Rgba8UnormSrgb` image, and verifies pixels contributed by
each layer. This proves a hidden native window is not inherently required for complete offscreen
composition. It does not claim that the normal game bootstrap is already an authoring host.

The media audit found the concrete Engine seam that Phase 3 must change: both FFmpeg and
AVFoundation video synchronization currently derive `DesignViewport` from the single `Window` and
return early if it is absent. Decoder/session cleanup is already bounded: FFmpeg uses a two-frame
queue, level-triggered cancellation, explicit visual/audio cleanup, and deferred worker joins;
AVFoundation pauses its player on drop and cleans visual/frame-bridge state when a session is
removed. Offscreen authoring therefore needs one Engine-owned viewport source that can come from
either a window or an authoring render target. It does not need an editor copy of media lifecycle
logic. Audio remains owned by the Engine process and is not transported as frame data.

The selected first-version preview transport is a bounded, latest-frame-wins raw RGBA triple
buffer in shared memory, with frame metadata and readiness notifications separate from control
IPC. It preserves process isolation, never blocks the Engine behind a slow editor, and adds no
compression or extra intermediate image. Each frame is read back once into its reusable slot and
GPUI uploads only the newest completed slot. The Phase 0 in-memory model validates dimensions,
stride, capacity, payload length, revision, generation, and frame identity without adding a
platform shared-memory package before implementation needs one.

A native child surface was rejected for the first version. GPUI does not provide one portable
child-surface contract, so that route would add NSView/HWND/X11/Wayland ownership, resize, focus,
occlusion, device-loss, and teardown code. The measured copy cost does not justify that additional
platform layer. It remains a benchmark-triggered optimization only if the end-to-end release
preview later misses its target because of the copy itself.

### Performance and power evidence

The repeatable `phase0_offscreen` example ran on Apple M5 Pro / Metal at 1920x1080. It keeps at
most three readbacks in flight and submits at most one per authoring-preview frame. Submitting several Bevy
`Screenshot` requests in the same frame was observed to leave requests without callbacks, so the
generic screenshot API is evidence for feasibility and a baseline, not the production ring-buffer
implementation.

| Profile | Warm samples | Median request latency | P95 | Completed throughput |
| --- | ---: | ---: | ---: | ---: |
| dev (`opt-level = 1`) | 120 | 38.317 ms | 40.179 ms | 51.92 fps |
| release/LTO | 120 | 38.506 ms | 40.533 ms | 51.74 fps |

The release CPU copies reached 70.75 GiB/s at 960x540 and 63.52 GiB/s at 1920x1080. One 1080p
slot copy averaged about 0.12 ms; three fixed slots reserve 23.7 MiB. The generic Screenshot path,
not the raw copy, is the current bottleneck. GPUI upload is not isolated by this spike, so Phase 3
must measure the complete Engine-readback/shared-slot/GPUI-upload chain before optimizing it.

The product target remains the Engine's native 60 fps while input, animation, video, or timeline
playback is active. Latest-frame-wins means stale completed frames are discarded; it does not mean
the Engine is intentionally lowered to 15 or 20 fps. For battery-powered authoring, unchanged
previews must use the existing event-driven lifecycle and stop rendering and copying rather than
poll at a reduced rate. Occluded, minimized, and background preview policy must likewise suspend
or reduce work without altering script time. A release benchmark should gate active throughput and
idle wakeups/CPU/GPU activity in CI once the Phase 3 transport exists; the single M5 Pro run is a
baseline, not a universal pass threshold.

## Minimal engine-facing interface for Phase 1

The editor should only require the following process-level concepts from the engine:

- launch with project root, launch token, control endpoint, and frame endpoint;
- negotiate protocol version and capability bits;
- load/reload project, pause/resume, seek to a script location, and stop;
- emit diagnostics, execution position, lifecycle state, and correlated command results;
- publish frame metadata containing session generation, revision, frame id, dimensions, stride,
  pixel format, and active buffer slot;
- accept an Engine-owned authoring viewport independent of a native game window;
- render on demand when stable and continuously at the normal target while time-dependent content
  is active;
- terminate cleanly when the owning editor window closes.

No editor JSON, Dock state, package internals, or GPUI types cross this boundary.

## Evidence so far

```text
cargo check -p keine-editor
cargo test -p keine-editor
cargo run -p keine-editor --example phase0_offscreen
cargo run --release -p keine-editor --example phase0_offscreen
cargo metadata --no-deps --format-version 1
cargo tree -p keine -e normal
cargo fmt --all --check
cargo build
cargo build -p keine-editor
cargo check --workspace
cargo clippy --workspace --all-targets
cargo test --workspace
```

Observed results:

- editor check passed;
- 8 unit tests, 1 Dock interaction test, and 2 process-IPC tests passed;
- workspace default member remained the root `keine` package;
- root `keine` normal dependency tree contained no GPUI package;
- macOS manual inspection confirmed the dark semantic palette, two windows, and tab switching.
- closing both macOS test windows terminated the process; a follow-up `pgrep` found no residue.
- a windowless Bevy process captured the ordered scene/UI/dialog composition at 1920x1080;
- dev and release/LTO three-frame pipelines completed about 52 1080p captures per second on the
  tested M5 Pro, while the bounded raw copy remained below 0.2 ms per frame;
- the complete workspace gate passed in a clean tracked checkout. The working checkout's optional,
  ignored commercial LetsGal sample is independent local evidence and is not part of this task.

Phase 0 is complete. Production authoring commands, shared-memory mapping, end-to-end GPUI upload,
adaptive preview scheduling, and the real workbench remain Phase 1/Phase 3 work rather than being
smuggled into this technical spike.
