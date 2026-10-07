//! NativeActivity entry point for the standalone Android Engine test package.

use anyhow::{Context, Result};
use bevy::prelude::{App, bevy_main};
use bevy::render::{
    RenderPlugin,
    settings::{Backends, WgpuSettings},
};
use keine_loader::LoaderRegistry;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

pub(super) mod backup;

static BACK_PENDING: AtomicBool = AtomicBool::new(false);
static BACK_WAKE: Mutex<Option<winit::event_loop::EventLoopProxy<bevy::winit::WinitUserEvent>>> =
    Mutex::new(None);

pub(super) fn install_back_wakeup(app: &App) {
    if let Some(proxy) = app
        .world()
        .get_resource::<bevy::winit::EventLoopProxyWrapper>()
        && let Ok(mut wake) = BACK_WAKE.lock()
    {
        *wake = Some(std::ops::Deref::deref(proxy).clone());
        BACK_PENDING.store(false, Ordering::Relaxed);
    }
}

pub(super) fn take_back() -> bool {
    BACK_PENDING.swap(false, Ordering::Relaxed)
}

/// No borrowed JVM data; Bevy remains on its event-loop thread.
#[unsafe(no_mangle)]
pub extern "system" fn Java_moe_maincore_keine_EngineActivity_nativeBack(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
) {
    BACK_PENDING.store(true, Ordering::Relaxed);
    wake();
}

fn wake() {
    if let Ok(wake) = BACK_WAKE.lock()
        && let Some(proxy) = wake.as_ref()
    {
        let _ = proxy.send_event(bevy::winit::WinitUserEvent::WakeUp);
    }
}

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
