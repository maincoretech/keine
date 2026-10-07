# Bevy display connection patch

Source: the official [`bevy_render` 0.19.1 crate](https://crates.io/crates/bevy_render/0.19.1).
Upstream MIT/Apache licenses and source files are retained.

`RenderDisplayHandle` owns the native display connection. `RenderPlugin` passes
it through automatic initialization and GPU recovery to `InstanceDescriptor.display`.
Kēne inserts winit's owned display before `RenderPlugin`, in both the game and
startup error view. Public `initialize_renderer` and `WgpuSettings` stay unchanged.

Upstream uses `display: None`; wgpu 29's Mesa EGL initialization then chooses
the surfaceless platform, which cannot present Wayland or X11 windows.
Changing the present mode cannot fix an empty surface capability list.
The patch uses the safe owned display API; no borrowed raw pointers or backend
selection/driver installation policy is added.

`sparse_buffer_vec.rs` also removes one redundant formatting borrow so the
official crate passes the workspace's current Clippy gate. `rustfmt.toml`
preserves upstream's 2021 formatting style. Two WGSL files only trim upstream
trailing whitespace; other source files are byte-identical.
The crate is a workspace member so its regression tests use the same lockfile.
This includes upstream test dependencies, not new shipping dependencies.

On upgrades, compare these four modified source files to upstream and
remove the override when it forwards the real display connection.
The two `renderer::display_tests` exercise descriptor ownership and headless
initialization without a GUI; Linux native Wayland/GL and X11/GL still require
actual window acceptance.
