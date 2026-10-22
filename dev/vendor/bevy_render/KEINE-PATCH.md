# Bevy native renderer patches

Source: the official [`bevy_render` 0.20.0 crate](https://crates.io/crates/bevy_render/0.20.0).
Upstream MIT/Apache licenses and source files are retained.

`RenderDisplayHandle` owns the native display connection. `RenderPlugin` passes
it through automatic initialization and GPU recovery to `InstanceDescriptor.display`.
Kēne inserts winit's owned display before `RenderPlugin`, in both the game and
startup error view. Public `initialize_renderer` and `WgpuSettings` stay unchanged.

Upstream uses `display: None`; wgpu 30's Mesa EGL initialization then chooses
the surfaceless platform, which cannot present Wayland or X11 windows.
Changing the present mode cannot fix an empty surface capability list.
The display patch uses the safe owned display API without borrowed raw pointers.

Android automatic initialization (without `raw_vulkan_init`) tries Vulkan first,
then retries a fresh GLES instance when surface, adapter or device initialization
returns an error. Single-backend selections remain authoritative. The private
initialization helper now returns errors so a failed Vulkan driver can be
released before GLES initialization; desktop selection policy is unchanged.
This does not catch driver crashes or recover from later shader/runtime errors.

`view/window/mod.rs` removes closing views from extraction but retains their
surfaces/native handles until render cleanup, after the final submission.
The 0.20 ECS window components are taken into `RetiringWindows`; the synced
window entity survives Android suspend/resume.
Cleanup waits for submitted GPU work before retiring those surfaces/handles;
native GL must not lose its surface/context before pending submission fences
complete. The wait is bounded to ten seconds and logs device/timeout errors.
This addresses shutdown ordering, not a recovery guarantee for a hung driver.

`camera.rs::extract_cameras` also removes `ViewTarget` and `ViewDepthStencilTexture`
when a camera becomes inactive or its target becomes invalid. Otherwise the
render entity retains color/depth attachments and an output texture view even
after its `ExtractedCamera` is removed; that view can pin an old swapchain
buffer across a resize. Active cameras and texture formats are unchanged.
Kēne's `inactive_camera_releases_render_targets_on_gpu` regression exercises
sleep, target resize, release and wake with a real Metal renderer.

`MaterialSlab::non_bindless_bindings` exposes a borrowed, read-only view of an
already prepared non-bindless group. Background crossfade checks the actual
textures/samplers before suppressing fallback sprites; the new erased material
API no longer exposes those bindings directly. Pending/bindless groups return
`None`, leaving the ordinary sprite path active.

`gpu_readback.rs` borrows the source and destination queues separately and
drains directly, retaining source capacity and command order. The old
`sparse_buffer_vec` and command-encoder Clippy patches are no longer needed.
`rustfmt.toml` preserves upstream's 2021 formatting style.
The crate is a workspace member so its regression tests use the same lockfile.
This includes upstream test dependencies, not new shipping dependencies.

On upgrades, compare the modified source files to upstream and
remove the override when it forwards the real display connection.
The `renderer::display_tests` exercise descriptor ownership, headless descriptor
creation and a recoverable failed adapter request without a GUI; native backend
rendering and Android automatic fallback still require hardware acceptance.
