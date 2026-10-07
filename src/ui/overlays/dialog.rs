//! Confirmation dialog facade; state, layout, input and motion have separate owners.
use crate::ui::save_load::capture::capture_save_preview;

use crate::render::blur::DialogCamera;
use crate::render::blur::UiBlurCamera;
use crate::storage::save::QUICK_SAVE_SLOT;
use crate::storage::settings::RuntimeSettings;
use crate::ui::backlog::BacklogRoot;
use crate::ui::control_bar::QuickSavePreview;
use crate::ui::foundation::{HoverSweep, UiFonts, UiSoundStyle, hover_sweep_fill};
use crate::ui::save_load::SaveLoadRoot;
use crate::ui::settings_panel::SettingsRoot;
use crate::ui::support::i18n::{UiText, tr};
use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use bevy::window::{PrimaryWindow, WindowCloseRequested};

const FADE_DURATION: f32 = 0.2;
const OVERLAY_ALPHA: f32 = 0.16;
const PANEL_ALPHA: f32 = 0.78;

mod state;
pub(crate) use state::*;
mod view;
pub use view::*;
mod actions;
pub use actions::*;
mod motion;
pub use motion::*;
mod sync;
pub use sync::*;
