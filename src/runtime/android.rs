//! NativeActivity entry point for the standalone Android Engine test package.

use anyhow::{Context, Result};
use bevy::prelude::bevy_main;
use bevy::render::{
    RenderPlugin,
    settings::{Backends, WgpuSettings},
};
use keine_loader::LoaderRegistry;

/// A private test-package override allows both drivers to be exercised with
/// the same APK. Absence means Vulkan first, then GLES on initialization failure.
pub(super) fn render_plugin() -> RenderPlugin {
    let mut settings = WgpuSettings {
        backends: Some(Backends::VULKAN | Backends::GL),
        ..Default::default()
    };
    if let Some(data) = bevy::android::ANDROID_APP
        .get()
        .and_then(|app| app.internal_data_path())
    {
        use std::io::Read;
        if let Ok(file) = std::fs::File::open(data.join("render-backend")) {
            let mut value = String::new();
            if file.take(16).read_to_string(&mut value).is_ok() {
                settings.backends = match value.trim() {
                    "gl" => Some(Backends::GL),
                    "vulkan" => Some(Backends::VULKAN),
                    "auto" => settings.backends,
                    _ => {
                        bevy::log::warn!(
                            "Invalid Android render-backend override; using automatic selection"
                        );
                        settings.backends
                    }
                };
            }
        }
    }
    RenderPlugin {
        render_creation: settings.into(),
        ..Default::default()
    }
}

#[bevy_main]
fn main() {
    if let Err(error) = run() {
        crate::runtime::platform::startup_error("failed to open Android project", &error);
        crate::ui::startup_error::show();
    }
}

fn run() -> Result<()> {
    let activity = bevy::android::ANDROID_APP
        .get()
        .context("Android Activity is not initialized")?;
    let data = activity
        .internal_data_path()
        .context("Android application data directory is unavailable")?;
    let project = data.join("game");
    std::fs::create_dir_all(&project).context("failed to create Android game directory")?;
    let mut app = crate::build_app_with_loader(project, LoaderRegistry::default())?;
    app.run();
    Ok(())
}
