# Kēne patch to `bevy_winit` 0.19.0

This directory is the crates.io source for `bevy_winit` 0.19.0, retained
under its upstream MIT/Apache-2.0 licenses. Kēne changes only
`src/state.rs` on macOS:

- `about_to_wait` requests a redraw only when a real event or timer deadline
  requires an application update.
- the synthetic `RedrawRequested` emitted by that bridge is not classified as
  fresh window input while the runner is reactive; continuous animation still
  treats each requested redraw as the driver for its next update.

Without both reactive conditions, Bevy's macOS workaround requests another
redraw from the redraw it just requested. A reactive, otherwise idle Kēne
window therefore loops at the display refresh rate. Suppressing the event in
continuous mode instead stalls animated UI after its first frame. Linux and
Windows retain the upstream event loop behavior unchanged.

Remove the `[patch.crates-io]` entry and this directory once upstream
`bevy_winit` provides equivalent behavior.
