#![warn(unused_crate_dependencies)]

#[cfg(feature = "publisher")]
mod compiler;
#[cfg(feature = "publisher")]
mod package;
mod render;
mod runtime;
mod scene;
mod storage;
mod ui;

pub use runtime::host::{HostCapabilityRegistry, HostCommandMessage};
pub use runtime::{build_app_with_loader, run, run_cli, run_with_loader};

// Criterion is a bench-only dev-dependency; the lib-test build sees it as
// available and the crate-level lint would otherwise report it as unused.
#[cfg(test)]
use criterion as _;

#[doc(hidden)]
#[cfg(all(feature = "video-native", target_os = "macos"))]
pub fn validate_native_video(
    mounts: &[keine_loader::ContentMount],
    path: &std::path::Path,
) -> Result<(), String> {
    scene::validate_native_video(mounts, path)
}
