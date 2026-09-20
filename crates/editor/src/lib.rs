#![warn(unused_crate_dependencies)]

#[cfg(test)]
use bevy as _;

mod app;
pub mod app_data;
pub mod authoring;
pub mod document;
pub mod engine;
pub mod frame_transport;
pub mod instance;
pub mod migration;
pub mod persistence;
pub mod preview;
pub mod project_key;
pub mod projection;
pub mod syntax;
pub mod workspace;

pub use app::run;
