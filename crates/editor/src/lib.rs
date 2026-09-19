#![warn(unused_crate_dependencies)]

#[cfg(test)]
use bevy as _;

mod app;
pub mod app_data;
pub mod frame_transport;
pub mod ipc;
pub mod project_key;

pub use app::run;
