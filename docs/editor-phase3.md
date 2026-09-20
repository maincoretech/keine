# Editor Phase 3 technical record

Status: **complete** on 2026-09-20. Phase 3 establishes the internal Editor to Engine control
boundary. It intentionally carries no rendered frames.

## Shared protocol

`keine-authoring` is a small Bevy-free crate shared by the Editor and Engine. Messages use a
versioned, 256 KiB-bounded, length-prefixed Postcard binary envelope; the control channel does not
use JSON and cannot carry frame payloads. Every request and response carries a session generation
and request ID. The initial authenticated handshake reports protocol version, Engine version,
build ID, supported project formats, and capabilities.

Protocol v1 contains only the Phase 3 surface:

- handshake and capabilities;
- open/close project and validate;
- source diagnostics;
- start, pause, resume, stop, and lifecycle state;
- ping/pong and graceful shutdown;
- structured bounded errors, including authentication and protocol mismatch.

This is an internal product boundary, not a public plugin ABI or native project DSL.

## Engine authoring host

The standalone `keine` executable accepts a hidden `__authoring-host` launch mode. It is omitted
from normal help and connects only to the loopback endpoint supplied by the Editor. The first
message must present the one-launch token and exact protocol version. The host owns one open
project and returns validation through the same loader and typed diagnostics used by the Engine's
normal `check` path.

The no-frame lifecycle states model an authoring session only; `Running` does not claim that Phase
4 rendering has been embedded. Normal Engine CLI behavior and shipping bundle behavior remain
separate.

## Editor process ownership

`EngineLocator` searches an explicit `KEINE_ENGINE`, a sibling executable, the macOS app Resources
location, then `PATH`. It never invokes Cargo. `EngineProcess` binds a loopback listener, creates a
single-use token, launches the prebuilt executable directly, validates the handshake and required
capabilities, and opens the project. Read, write, connect, and shutdown operations are bounded.

Each project window owns its own child and protocol generation. Closing or dropping the session
requests graceful shutdown, then kills and reaps a child that misses the bounded deadline. A crash
in one project session does not terminate a different session. The activity-rail Engine control
surfaces connecting, ready, and failed state and places validation diagnostics into the existing
Output and Inspector views.

## Boundaries

Phase 3 does not add frames, shared-memory mapping, GPUI texture upload, Preview view controls,
hot document snapshots, runtime input forwarding, Engine download/update, public SDK stability,
or a second repository. Control IPC and future frame transport remain separate; Editor and Engine
continue to be independently runnable programs in this repository.

## Evidence

Unit tests cover binary round trips, bounded/truncated envelopes, and Engine executable discovery.
Process tests launch the already-built `keine` binary, complete handshake/open/validate and the
full no-frame lifecycle, reject a mismatched protocol with a structured error, run two independent
children, kill one, and confirm the other still responds before clean shutdown. The Editor UI was
also exercised against a real child; validation failure was shown as a bounded Output diagnostic.
