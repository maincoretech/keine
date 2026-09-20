#![warn(unused_crate_dependencies)]

#[cfg(test)]
use bevy as _;

mod app;
pub mod app_data;
pub mod document;
pub mod engine;
pub mod frame_transport;
pub mod instance;
pub mod persistence;
pub mod project_key;
pub mod workspace;

pub use app::run;
