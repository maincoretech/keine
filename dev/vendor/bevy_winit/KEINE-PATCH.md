# Bevy window patch

Source: the official [`bevy_winit` 0.19.1 crate](https://crates.io/crates/bevy_winit/0.19.1).
The upstream MIT and Apache licenses remain in this directory.

Only `src/state.rs::about_to_wait` differs from upstream: on macOS, call
`redraw_requested` only when `should_update` reports pending work or a deadline.
This preserves Kēne's existing reactive idle fix while adopting every upstream
0.19.1 window fix. Remove the override once upstream fixes this idle loop.

On upgrades, copy the official crate and reapply this single hunk. Do not copy
the older patched backend: that would discard upstream window changes.
