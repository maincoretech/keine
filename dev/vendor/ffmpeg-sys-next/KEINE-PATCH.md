# FFmpeg bindings build stack patch

Source: official crates.io `ffmpeg-sys-next` 9.0.1, upstream revision
`ba293c6ffa4a5f2f745df024c96b54b5c94abd84` in
[rust-ffmpeg-sys](https://github.com/zmwangx/rust-ffmpeg-sys/tree/ba293c6ffa4a5f2f745df024c96b54b5c94abd84).
The upstream WTFPL declaration, normalized manifest, sources and headers are retained.
The local build-script addition uses the repository's Defold License 1.0.

`build.rs` runs its original body on a joined worker with a 16 MiB stack.
Windows CI job [119049097304](https://github.com/maincoretech/keine/actions/runs/39547510274/job/119049097304)
failed on the main thread after the SDK/version probes and before binding output,
with `0xc00000fd / STATUS_STACK_OVERFLOW`. The Rust main-thread stack is not
controlled by `RUST_MIN_STACK`; an explicit worker also covers Cargo builds with
`--target`, where target rustflags do not apply to host build scripts.
Worker panics are propagated so Cargo retains the original failure status.

This changes build-time stack allocation only: no dependency versions, enabled
FFmpeg components, generated API policy, native SDK, or runtime stack settings change.
The crate is excluded from workspace membership so `--all-features` does not
enable upstream source-build, GPL, or optional external-codec features.

On upgrade, compare with the official crate and remove this override when upstream
provides an equivalent fix. Native Windows compilation must be verified in CI;
successful macOS binding generation does not replace that check.
