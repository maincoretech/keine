//! Document panel presentation and interaction state. The source owner is crate::document.
mod state;
pub(in crate::app) use state::DocumentState;

mod view;
