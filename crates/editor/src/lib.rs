#![warn(unused_crate_dependencies)]

#[cfg(test)]
use bevy as _;

mod app;
pub mod authoring;
pub mod preview;
pub mod workspace;

pub use authoring::{projection, syntax};
pub use preview::{engine, frame_transport, instance};
pub(crate) use workspace::file_ops;
pub use workspace::{app_data, document, migration, persistence, project_key};

pub use app::run;
