//! NativeActivity entry point for Engine tests and packaged games.

use anyhow::{Context, Result};
use bevy::prelude::{App, bevy_main};
use bevy::render::{
    RenderPlugin,
    settings::{Backends, WgpuSettings},
};
use keine_loader::{HakutakuError, LoaderRegistry, PositionedFile, SegmentId, SegmentSource};
use std::sync::Arc;
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
    #[cfg(feature = "hardened")]
    crate::runtime::platform::apply_hardening();
    let activity = bevy::android::ANDROID_APP
        .get()
        .context("Android Activity is not initialized")?;
    let data = activity
        .internal_data_path()
        .context("Android application data directory is unavailable")?;
    let assets = activity.asset_manager();
    let mut app = if let Some(snapshot) = assets.open(c"keine-game/game.haku") {
        let descriptor = snapshot
            .open_file_descriptor()
            .context("APK game snapshot must be uncompressed")?;
        let snapshot = super::android_package::ApkFile::new(
            descriptor.fd.into(),
            descriptor.offset as u64,
            descriptor.size as u64,
        )?;
        let archive = keine_loader::HakutakuArchive::open_packaged_sources(
            data.join("apk-game/game.haku"),
            Arc::new(snapshot),
            Arc::new(ApkSegments(activity.clone())),
        )?;
        let project = keine_loader::open_hakutaku_archive(archive)?;
        super::bootstrap::build_project_app(
            super::bootstrap::OpenedProject {
                root: project.root,
                config: project.config,
                content: project.content,
                packaged: true,
            },
            LoaderRegistry::default(),
        )?
    } else {
        let project = data.join("game");
        std::fs::create_dir_all(&project).context("failed to create Android game directory")?;
        crate::build_app_with_loader(project, LoaderRegistry::default())?
    };
    app.run();
    Ok(())
}

// Asset handles stay on the opening thread; only owned, positioned file
// descriptors cross into Loader/media workers. Signed segment IDs cannot
// address arbitrary APK paths.
struct ApkSegments(bevy::android::android_activity::AndroidApp);

impl SegmentSource for ApkSegments {
    fn open(&self, id: SegmentId) -> Result<Arc<dyn PositionedFile>, HakutakuError> {
        let name = std::ffi::CString::new(format!("keine-game/data/{id}.taku"))
            .expect("segment digest contains only hex digits");
        let asset = self
            .0
            .asset_manager()
            .open(&name)
            .ok_or(HakutakuError::SegmentUnavailable(id))?;
        let descriptor = asset.open_file_descriptor()?;
        Ok(Arc::new(super::android_package::ApkFile::new(
            descriptor.fd.into(),
            descriptor.offset as u64,
            descriptor.size as u64,
        )?))
    }
}
