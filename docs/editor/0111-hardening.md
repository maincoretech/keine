# Editor 0.11.1 hardening

This increment implements the four P1 and seven P2 findings from the 2026-09-29
Editor review against `main` at `51574d4`. It changes Editor ownership and scheduling;
Eiyashou v1.1 spellings, source-backed Block edits, the native Engine window, protocol
version and save format remain as documented.

```text
P1
├── F1 · Asset import consistency
│   ├── One physical-project manifest writer serializes imports
│   ├── Recheck the disk manifest after copying; roll back the copied file on conflict
│   ├── Busy projects reject overlapping file mutations and saves
│   └── Completion refuses to overwrite a dirty manifest draft or its recovery
├── F2 · Window close lifecycle
│   ├── Both confirmation outcomes stop Preview and audition before window removal
│   ├── Close Without Saving awaits the latest recovery revision
│   └── Final close flushes layout; releasing a workbench removes its global session
├── F3 · Index ownership
│   ├── Immutable Arc index snapshots; 60 ms cancellable debounce
│   ├── Script edits replace only changed contributions on a background worker
│   ├── Manifest/config changes invalidate the full index
│   └── Cross-file asset/scene rewrites wait for a current index
└── F4 · Source analysis and Block rendering
    ├── Shared newline/multibyte indexes replace per-statement prefix scans
    ├── String escape/interpolation errors use that same index
    ├── Diagnostic identity is hashed; report vectors retain lexical/scene order
    ├── SourceDocument caches projection and dialogue rows by revision
    ├── Highlighter/completion lex without lowering a Program
    └── Block inputs/controls render near the viewport; measured heights survive culling
P2
├── F5 · Disk freshness
│   ├── Clean document reopen rereads disk and current configured manifest policy
│   ├── Reopen also refreshes the shared index and Preview source snapshot
│   ├── Dirty documents stay authoritative until save or explicit reload
│   └── Reload button / Cmd-Alt-R (Ctrl-Alt-R) confirms discarding a draft
├── F6 · Source positions
│   └── Parser, Block selection and Editor diagnostics use Unicode scalar columns
│       with explicit conversion to UTF-8 byte offsets
├── F7 · Complete discovery
│   ├── No silent 2,000-entry discovery or 800-row Explorer truncation
│   └── Explorer renders a viewport with spacers over the complete file inventory
├── F8 · Preview backpressure
│   ├── Latest snapshot per path and latest cursor replace pending stale work
│   ├── Stop/Shutdown precede pending snapshots
│   └── A 16 MiB pending-source budget fails explicitly; full restart resynchronizes
├── F9 · Document limits
│   └── Reject >1 MiB replacements before accepting them into authoritative source
├── F10 · Filesystem scheduling
│   ├── Recovery writes and debounced layouts run on background workers
│   ├── Epoch checks and per-document writers prevent stale recovery commits
│   ├── Retiring a document drains its writer and invalidates queued cleanup before reopen
│   ├── Internal copies and file inventory refreshes run in the background
│   └── Initial discovery is shared by panels; read only two initial text documents
└── F11 · Retention
    ├── Clean closed tabs release cached documents; closed projects release workers/indexes
    ├── Only explicit project opening creates sessions; late callbacks cannot recreate them
    ├── Migration replaces inventory, document caches and workers together
    ├── Culled Textarea subscriptions belong to their row states
    └── Source undo/redo shares a 16 MiB budget and 32-transaction limit
```

## Invariants and remaining scope

GPUI input Positions are Unicode scalar columns (`gpui-base` 0.6.4 `RopeExt`),
not UTF-16 LSP columns; native spans follow this same contract. The pinned primary
package source was checked before changing conversions. GPUI `Task` drop cancels
its future, so debounce tasks have explicit owners; background work that already
started also checks its captured epoch before applying results. Recovery workers
serialize with a per-document mutex and check an Acquire/Release clock before and
after writing. Final close performs a durable flush before application exit.

Preview coalescing does not interrupt a transport request already executing; the
existing IPC timeouts still bound that request. External programs do not participate
in the import writer mutex: a changed manifest detected at commit cancels the import,
and an already-dirty editor draft remains available for reconciliation. Explicit
save/reload/close retain their filesystem validation; they are not typing hot paths.

Highlighter lexing remains a bounded full-document pass. Incremental index assembly
still clones the previous index in the background. Viewport layout scans lightweight
Block metadata; it does not create every offscreen input. These are deliberate current
limits, not claims of zero allocation or constant-time rendering.

Offscreen Block heights use estimates until measured; wrapped-dialogue scrolling,
selection and drag/drop require running-app acceptance. Parser/index timings are not
frame or input-latency measurements.

## Evidence

Before/after raw Release probes and gate logs are in the ignored local directory
`target/authoring/editor-audit-20260929/0111/`; the earlier review and baseline logs
remain in its parent directory. Measurements and exact commands are recorded in
`docs/performance-baseline.md`. Regression checks extend existing unit suites and
use disposable temporary directories; no tracked test project was added.

Validation and GUI outcomes are recorded in `docs/PROJECT_STATE.md` after final checks.
Windows/Linux and the previously agreed display/IME/audio/Preview-FPS acceptance remain
with the user.
