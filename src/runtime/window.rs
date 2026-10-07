//! Normal window bounds belong to project preferences, outside save-slot rollback.

use std::path::Path;

use bevy::app::AppExit;
use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::{Monitor, OnMonitor, PrimaryMonitor, PrimaryWindow, WindowMode, WindowPosition};
use bevy::winit::WINIT_WINDOWS;
use serde::{Deserialize, Serialize};

use super::preview::AuthoringPreviewSession;
use super::resources::{
    EditorSyncSession, PersistenceDisabled, PersistenceRoot, writable_runtime_session,
};

const VERSION: u32 = 1;
const MAX_BYTES: usize = 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct WindowBounds {
    version: u32,
    size: [u32; 2],
    position: Option<[i32; 2]>,
    monitor_origin: [i32; 2],
    scale: f64,
    maximized: bool,
}

impl WindowBounds {
    fn valid(&self) -> bool {
        self.version == VERSION
            && self.size.iter().all(|size| (1..=65536).contains(size))
            && self.scale.is_finite()
            && (0.25..=8.0).contains(&self.scale)
            && self
                .monitor_origin
                .iter()
                .all(|value| value.unsigned_abs() <= 1_000_000)
            && self.position.is_none_or(|position| {
                position
                    .iter()
                    .all(|value| value.unsigned_abs() <= 1_000_000)
            })
    }

    fn load(root: &Path) -> Option<Self> {
        let bytes = crate::storage::read_limited(&root.join("saves/window.bin"), MAX_BYTES).ok()?;
        let bounds: Self = crate::storage::decode_postcard_exact(&bytes).ok()?;
        bounds.valid().then_some(bounds)
    }
}

#[derive(Resource, Default)]
struct WindowMemory {
    initialized: bool,
    bounds: Option<WindowBounds>,
    fullscreen: bool,
}

pub(crate) struct WindowMemoryPlugin;

impl Plugin for WindowMemoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WindowMemory>()
            .add_systems(PostStartup, initialize)
            // Some platforms enumerate monitors after Startup. Keep the window
            // hidden until the first bounds request is ready for winit.
            .add_systems(Update, initialize.run_if(needs_initialization))
            .add_systems(PreUpdate, remember)
            .add_systems(Last, persist_on_exit);
    }
}

fn needs_initialization(memory: Res<WindowMemory>) -> bool {
    !memory.initialized
}

/// Physical desktop coordinates can be negative on secondary monitors. Leave
/// space for decorations and common desktop panels; winit has no portable
/// monitor work-area API. Explicit benchmark sizes bypass this policy entirely.
fn fitted_bounds(
    size: UVec2,
    position: Option<IVec2>,
    origin: IVec2,
    screen: UVec2,
    scale: f64,
    frame: UVec2,
) -> (UVec2, IVec2) {
    let margin = (32.0 * scale) as u32;
    let bottom = (64.0 * scale) as u32;
    let available = UVec2::new(
        screen.x.saturating_sub(margin * 2 + frame.x).max(1),
        screen.y.saturating_sub(margin + bottom + frame.y).max(1),
    );
    let ratio = (available.x as f64 / size.x.max(1) as f64)
        .min(available.y as f64 / size.y.max(1) as f64)
        .min(1.0);
    let size = UVec2::new(
        ((size.x.max(1) as f64 * ratio) as u32).max(1),
        ((size.y.max(1) as f64 * ratio) as u32).max(1),
    );
    let outer = size + frame;
    let minimum = origin + IVec2::splat(margin as i32);
    let maximum =
        (origin + screen.as_ivec2() - outer.as_ivec2() - IVec2::new(margin as i32, bottom as i32))
            .max(minimum);
    let centered = minimum + (maximum - minimum) / 2;
    (size, position.unwrap_or(centered).clamp(minimum, maximum))
}

fn initialize(
    root: Res<PersistenceRoot>,
    mut memory: ResMut<WindowMemory>,
    mut windows: Query<(Entity, &mut Window), With<PrimaryWindow>>,
    monitors: Query<(Entity, &Monitor, Has<PrimaryMonitor>)>,
    _main_thread: NonSendMarker,
) {
    if memory.initialized || monitors.is_empty() {
        return;
    }
    let Ok((entity, mut window)) = windows.single_mut() else {
        return;
    };
    let saved = WindowBounds::load(&root);
    let matching = saved.as_ref().and_then(|saved| {
        monitors
            .iter()
            .find(|(_, monitor, _)| monitor.physical_position.to_array() == saved.monitor_origin)
    });
    let Some((_, monitor, _)) = matching
        .or_else(|| monitors.iter().find(|(_, _, primary)| *primary))
        .or_else(|| monitors.iter().next())
    else {
        return;
    };
    let frame = WINIT_WINDOWS.with_borrow(|windows| {
        windows.get_window(entity).map_or(UVec2::ZERO, |native| {
            let inner = native.inner_size();
            let outer = native.outer_size();
            UVec2::new(
                outer.width.saturating_sub(inner.width),
                outer.height.saturating_sub(inner.height),
            )
        })
    });
    let scale = monitor.scale_factor;
    let size = saved
        .as_ref()
        .map_or(window.resolution.physical_size(), |saved| {
            UVec2::new(
                (saved.size[0] as f64 * scale / saved.scale).round() as u32,
                (saved.size[1] as f64 * scale / saved.scale).round() as u32,
            )
        });
    let position = matching.and_then(|_| {
        let saved = saved.as_ref()?;
        let offset = IVec2::from_array(saved.position?) - IVec2::from_array(saved.monitor_origin);
        Some(monitor.physical_position + (offset.as_dvec2() * (scale / saved.scale)).as_ivec2())
    });
    let (size, position) = fitted_bounds(
        size,
        position,
        monitor.physical_position,
        monitor.physical_size(),
        scale,
        frame,
    );
    window.resolution.set_physical_resolution(size.x, size.y);
    window.position = WindowPosition::At(position);
    if saved.as_ref().is_some_and(|saved| saved.maximized) {
        window.set_maximized(true);
    }
    window.visible = true;
    memory.bounds = Some(WindowBounds {
        version: VERSION,
        size: size.to_array(),
        position: Some(position.to_array()),
        monitor_origin: monitor.physical_position.to_array(),
        scale,
        maximized: saved.is_some_and(|saved| saved.maximized),
    });
    memory.initialized = true;
}

type WindowQuery<'w, 's> =
    Query<'w, 's, (Entity, Ref<'static, Window>, Option<&'static OnMonitor>), With<PrimaryWindow>>;

fn remember(
    mut memory: ResMut<WindowMemory>,
    windows: WindowQuery,
    monitors: Query<&Monitor>,
    _main_thread: NonSendMarker,
) {
    let Ok((entity, window, on_monitor)) = windows.single() else {
        return;
    };
    if !memory.initialized {
        return;
    }
    let leaving_fullscreen =
        std::mem::replace(&mut memory.fullscreen, window.mode != WindowMode::Windowed);
    if memory.fullscreen || leaving_fullscreen || !window.is_changed() {
        return;
    }
    WINIT_WINDOWS.with_borrow(|windows| {
        let Some(native) = windows.get_window(entity) else {
            return;
        };
        // Never replace normal bounds with a fullscreen/maximized/minimized
        // rectangle, or with the hidden native window's pre-restore size.
        if native.fullscreen().is_some()
            || native.is_minimized() == Some(true)
            || native.is_visible() == Some(false)
        {
            return;
        }
        let Some(bounds) = memory.bounds.as_mut() else {
            return;
        };
        bounds.maximized = native.is_maximized();
        if bounds.maximized {
            return;
        }
        let size = native.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        bounds.size = [size.width, size.height];
        bounds.scale = native.scale_factor();
        bounds.position = native.outer_position().ok().map(|p| [p.x, p.y]);
        if let Some(monitor) = on_monitor.and_then(|link| monitors.get(link.0).ok()) {
            bounds.monitor_origin = monitor.physical_position.to_array();
        }
    });
}

fn persist_on_exit(
    mut exits: MessageReader<AppExit>,
    memory: Res<WindowMemory>,
    root: Res<PersistenceRoot>,
    editor: Option<Res<EditorSyncSession>>,
    preview: Option<Res<AuthoringPreviewSession>>,
    disabled: Option<Res<PersistenceDisabled>>,
) {
    if exits.read().next().is_none()
        || !writable_runtime_session(editor.is_some(), preview.is_some(), disabled.is_some())
    {
        return;
    }
    let Some(bounds) = memory.bounds.as_ref().filter(|bounds| bounds.valid()) else {
        return;
    };
    let result = crate::storage::encode_postcard_limited(bounds, MAX_BYTES, "window bounds")
        .and_then(|bytes| crate::storage::write_atomically(&root.join("saves/window.bin"), &bytes));
    if let Err(error) = result {
        log::warn!("failed to remember window bounds: {error:#}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_root() -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("keine-window-{nonce}"))
    }

    #[test]
    fn restore_scales_for_dpi_changes_and_recenters_after_monitor_removal() {
        for monitor_origin in [[-1920, 0], [0, 0]] {
            let root = temporary_root();
            let saved = WindowBounds {
                version: VERSION,
                size: [800, 450],
                position: Some([-1800, 100]),
                monitor_origin: [-1920, 0],
                scale: 1.0,
                maximized: false,
            };
            crate::storage::write_atomically(
                &root.join("saves/window.bin"),
                &postcard::to_stdvec(&saved).unwrap(),
            )
            .unwrap();
            let mut app = App::new();
            app.insert_resource(PersistenceRoot(root.clone()))
                .init_resource::<WindowMemory>()
                .add_systems(Update, initialize);
            let entity = app
                .world_mut()
                .spawn((
                    Window {
                        visible: false,
                        ..default()
                    },
                    PrimaryWindow,
                ))
                .id();
            app.world_mut().spawn((
                Monitor {
                    name: None,
                    physical_height: 1600,
                    physical_width: 2560,
                    physical_position: IVec2::from_array(monitor_origin),
                    scale_factor: 2.0,
                    refresh_rate_millihertz: None,
                    video_modes: vec![],
                },
                PrimaryMonitor,
            ));
            app.update();
            let window = app.world().get::<Window>(entity).unwrap();
            assert!(window.visible);
            assert_eq!(window.physical_size(), UVec2::new(1600, 900));
            let expected = if monitor_origin == [-1920, 0] {
                IVec2::new(-1680, 200)
            } else {
                IVec2::new(480, 318)
            };
            assert_eq!(window.position, WindowPosition::At(expected));
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn exit_persistence_respects_benchmark_and_read_only_session_boundaries() {
        for (disabled, read_only) in [(false, false), (true, false), (false, true)] {
            let root = temporary_root();
            let bounds = WindowBounds {
                version: VERSION,
                size: [900, 600],
                position: Some([80, 90]),
                monitor_origin: [0, 0],
                scale: 1.0,
                maximized: false,
            };
            let mut app = App::new();
            app.insert_resource(PersistenceRoot(root.clone()))
                .insert_resource(WindowMemory {
                    initialized: true,
                    bounds: Some(bounds.clone()),
                    ..default()
                })
                .add_message::<AppExit>()
                .add_systems(Update, persist_on_exit);
            if disabled {
                app.insert_resource(PersistenceDisabled);
            }
            if read_only {
                app.insert_resource(EditorSyncSession);
            }
            app.world_mut().write_message(AppExit::Success);
            app.update();
            assert_eq!(
                WindowBounds::load(&root),
                (!disabled && !read_only).then_some(bounds)
            );
            if root.exists() {
                std::fs::remove_dir_all(root).unwrap();
            }
        }
    }

    #[test]
    fn first_window_fits_small_desktop_including_decorations_and_panels() {
        let (size, position) = fitted_bounds(
            UVec2::new(1920, 1080),
            None,
            IVec2::ZERO,
            UVec2::new(1366, 768),
            1.0,
            UVec2::new(16, 40),
        );
        assert!(position.x >= 32 && position.y >= 32);
        assert!(position.x + size.x as i32 + 16 <= 1366 - 32);
        assert!(position.y + size.y as i32 + 40 <= 768 - 64);
        assert!((size.x as f64 / size.y as f64 - 16.0 / 9.0).abs() < 0.01);
    }

    #[test]
    fn negative_monitor_origin_and_offscreen_saved_position_are_clamped() {
        let (size, position) = fitted_bounds(
            UVec2::new(900, 600),
            Some(IVec2::new(4000, -5000)),
            IVec2::new(-1920, 0),
            UVec2::new(1920, 1080),
            1.0,
            UVec2::new(16, 32),
        );
        assert_eq!(size, UVec2::new(900, 600));
        assert_eq!(position, IVec2::new(-948, 32));
    }

    #[test]
    fn tiny_and_hidpi_desktops_produce_nonzero_bounded_sizes() {
        for screen in [UVec2::new(1, 1), UVec2::new(2560, 1600)] {
            let (size, _) = fitted_bounds(
                UVec2::new(3840, 2160),
                None,
                IVec2::ZERO,
                screen,
                2.0,
                UVec2::new(0, 56),
            );
            assert!(size.x > 0 && size.y > 0);
            assert!(size.x <= screen.x && size.y <= screen.y);
        }
    }

    #[test]
    fn persisted_bounds_reject_unknown_versions_invalid_scales_and_trailing_data() {
        let bounds = WindowBounds {
            version: VERSION,
            size: [900, 600],
            position: Some([-1000, 100]),
            monitor_origin: [-1920, 0],
            scale: 2.0,
            maximized: true,
        };
        let mut bytes =
            crate::storage::encode_postcard_limited(&bounds, MAX_BYTES, "window bounds").unwrap();
        assert_eq!(
            crate::storage::decode_postcard_exact::<WindowBounds>(&bytes).unwrap(),
            bounds
        );
        bytes.push(0);
        assert!(crate::storage::decode_postcard_exact::<WindowBounds>(&bytes).is_err());
        assert!(
            !WindowBounds {
                version: 2,
                ..bounds.clone()
            }
            .valid()
        );
        assert!(
            !WindowBounds {
                scale: f64::NAN,
                ..bounds.clone()
            }
            .valid()
        );
        assert!(
            !WindowBounds {
                size: [0, 600],
                ..bounds.clone()
            }
            .valid()
        );
        assert!(
            !WindowBounds {
                position: Some([i32::MIN, i32::MAX]),
                ..bounds
            }
            .valid()
        );
    }
}
