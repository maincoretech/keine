#![warn(unused_crate_dependencies)]

mod compiler;
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
