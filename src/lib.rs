#![warn(unused_crate_dependencies)]

#[cfg(feature = "publisher")]
mod compiler;
#[cfg(feature = "publisher")]
mod package;
mod render;
#[cfg(feature = "publisher")]
mod resource_migration;
mod runtime;
mod scene;
mod storage;
mod ui;

pub use runtime::host::{HostCapabilityRegistry, HostCommandMessage};
pub use runtime::{build_app_with_loader, run, run_cli, run_with_loader};

// Bench-only dev-dependencies are also visible to the lib-test build, where
// the crate-level lint would otherwise report them as unused.
#[cfg(test)]
use criterion as _;
#[cfg(test)]
use libwebp_sys as _;

#[doc(hidden)]
#[cfg(all(feature = "video-native", target_os = "macos"))]
pub fn validate_native_video(
    mounts: &[keine_loader::ContentMount],
    path: &std::path::Path,
) -> Result<(), String> {
    scene::validate_native_video(mounts, path)
}

#[doc(hidden)]
#[cfg(all(
    feature = "video-ffmpeg",
    not(all(feature = "video-native", target_os = "macos"))
))]
pub fn validate_ffmpeg_video(
    mounts: &[keine_loader::ContentMount],
    path: &std::path::Path,
) -> Result<(), String> {
    scene::validate_ffmpeg_video(mounts, path)
}
