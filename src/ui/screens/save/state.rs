//! Save_Load state; registered through the parent facade.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SaveLoadMode {
    Save,
    Load,
}

impl SaveLoadMode {
    pub(super) fn watermark(self) -> &'static str {
        match self {
            Self::Save => "SAVE",
            Self::Load => "LOAD",
        }
    }
}

#[derive(Resource)]
pub(crate) struct SaveLoadUi {
    pub(crate) mode: Option<SaveLoadMode>,
    pub(crate) page: u32,
}

impl Default for SaveLoadUi {
    fn default() -> Self {
        Self {
            mode: None,
            page: 1,
        }
    }
}

#[derive(Component)]
pub(crate) struct SaveLoadRoot;

#[derive(Component)]
pub(crate) struct SaveLoadContent;

#[derive(Component)]
pub(crate) struct SaveLoadBlurProxy;

#[derive(Resource, Default)]
pub(crate) struct SavePreviewCache {
    pub(super) ready: HashMap<u32, CachedPreview>,
    pub(super) pending: HashMap<u32, Task<Option<LoadedPreview>>>,
}

pub(super) struct CachedPreview {
    pub(super) modified: Option<SystemTime>,
    pub(super) handle: Handle<Image>,
}

pub(super) struct LoadedPreview {
    pub(super) modified: SystemTime,
    pub(super) image: Image,
}

impl SavePreviewCache {
    pub(crate) fn insert_live(&mut self, slot: u32, handle: Handle<Image>) {
        self.pending.remove(&slot);
        self.ready.insert(
            slot,
            CachedPreview {
                modified: None,
                handle,
            },
        );
    }

    pub(crate) fn clear(&mut self) {
        self.ready.clear();
        self.pending.clear();
    }

    pub(crate) fn invalidate(&mut self, slot: u32) {
        self.ready.remove(&slot);
        self.pending.remove(&slot);
    }
}

pub(super) struct SaveContentContext<'a> {
    pub(super) font: &'a Handle<Font>,
    pub(super) project_root: &'a PersistenceRoot,
    pub(super) store: &'a dyn keine_loader::StoreAdapter,
    pub(super) program_fingerprint: u64,
    pub(super) preview_cache: &'a mut SavePreviewCache,
}

#[derive(Resource, Default)]
pub(crate) struct SaveLoadPageTransition {
    pub(super) active: bool,
    pub(super) elapsed: f32,
    pub(super) direction: f32,
}

impl SaveLoadPageTransition {
    pub(super) const SECONDS: f32 = 0.22;

    pub(super) fn begin(&mut self, direction: f32) {
        self.active = true;
        self.elapsed = 0.0;
        self.direction = direction.signum();
    }

    pub(crate) fn is_animating(&self) -> bool {
        self.active
    }
}

#[derive(Component)]
pub(crate) struct SaveLoadPreviewImage(pub(super) u32);

#[derive(Component)]
pub(crate) struct SaveLoadGridViewport;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SaveLoadGridPhase {
    Incoming,
    Outgoing,
    Settled,
}

#[derive(Component)]
pub(crate) struct SaveLoadSlotGrid {
    pub(super) phase: SaveLoadGridPhase,
}

#[derive(Default)]
pub(crate) struct SaveLoadContentFadeCache {
    pub(super) active: bool,
    pub(super) last_alpha: f32,
    pub(super) text: HashMap<Entity, f32>,
    pub(super) background: HashMap<Entity, f32>,
    pub(super) border: HashMap<Entity, [f32; 4]>,
    pub(super) image: HashMap<Entity, f32>,
}

#[derive(Component)]
pub(crate) struct SaveLoadSlot(pub(crate) u32);

#[derive(Component)]
pub(crate) struct SaveLoadPage(pub(crate) u32);

#[derive(Component)]
pub(crate) struct SaveLoadPageVisual {
    pub(super) selected: bool,
    pub(super) text: f32,
    pub(super) press: f32,
}

impl SaveLoadPageVisual {
    pub(crate) fn is_animating(
        &self,
        interaction: Interaction,
        page: u32,
        selected_page: u32,
    ) -> bool {
        let selected = page == selected_page;
        let hovered = matches!(interaction, Interaction::Hovered | Interaction::Pressed);
        let target_text = if selected || hovered {
            PAGE_HIGHLIGHT_TEXT_ALPHA
        } else {
            0.2
        };
        (self.text - target_text).abs() > 0.001 || self.press > 0.001
    }
}

#[derive(Component)]
pub(crate) struct SaveLoadPageLabel;

#[derive(Component)]
pub(crate) struct SaveLoadSlotMotion {
    pub(super) scale: f32,
}

impl SaveLoadSlotMotion {
    pub(crate) fn is_animating(&self, interaction: Interaction) -> bool {
        let target_scale = match interaction {
            Interaction::Pressed => 0.97,
            Interaction::Hovered => 0.985,
            Interaction::None => 1.0,
        };
        (self.scale - target_scale).abs() > 0.001
    }
}

pub(super) type SettingsRootVisibilityQuery<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static mut Visibility),
    (
        With<crate::ui::settings_panel::SettingsRoot>,
        Without<crate::ui::settings_panel::SettingsBlurProxy>,
        Without<SaveLoadRoot>,
    ),
>;
pub(super) type SettingsProxyVisibilityQuery<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static mut Visibility),
    (
        With<crate::ui::settings_panel::SettingsBlurProxy>,
        Without<crate::ui::settings_panel::SettingsRoot>,
        Without<SaveLoadRoot>,
        Without<SaveLoadBlurProxy>,
    ),
>;
pub(super) type SaveLoadRootVisibilityQuery<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static mut Visibility),
    (With<SaveLoadRoot>, Without<SaveLoadBlurProxy>),
>;
pub(super) type SaveLoadProxyVisibilityQuery<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static mut Visibility),
    (
        With<SaveLoadBlurProxy>,
        Without<SaveLoadRoot>,
        Without<crate::ui::settings_panel::SettingsRoot>,
    ),
>;

#[derive(SystemParam)]
pub(crate) struct SaveLoadSyncContext<'w, 's> {
    pub(super) commands: Commands<'w, 's>,
    pub(super) roots: SaveLoadRootVisibilityQuery<'w, 's>,
    pub(super) grids: Query<'w, 's, (Entity, &'static mut SaveLoadSlotGrid)>,
    pub(super) grid_viewports: Query<'w, 's, Entity, With<SaveLoadGridViewport>>,
    pub(super) proxies: SaveLoadProxyVisibilityQuery<'w, 's>,
    pub(super) camera: Query<'w, 's, Entity, With<DialogCamera>>,
    pub(super) blur_camera: Query<'w, 's, Entity, With<UiBlurCamera>>,
    pub(super) fonts: Res<'w, UiFonts>,
    pub(super) project_root: Res<'w, PersistenceRoot>,
    pub(super) store: Res<'w, crate::runtime::resources::StoreCodec>,
    pub(super) state: Res<'w, crate::runtime::resources::GameState>,
    pub(super) preview_cache: ResMut<'w, SavePreviewCache>,
    pub(super) fades: Query<'w, 's, &'static mut MenuFade>,
    pub(super) watermarks: Query<
        'w,
        's,
        (
            &'static mut crate::ui::settings_panel::SettingsWatermark,
            &'static Text,
        ),
    >,
    pub(super) settings_roots: SettingsRootVisibilityQuery<'w, 's>,
    pub(super) settings_proxies: SettingsProxyVisibilityQuery<'w, 's>,
    pub(super) route_transition: Res<'w, MenuRouteTransition>,
}

#[derive(SystemParam)]
pub(crate) struct SaveLoadFadeContext<'w, 's> {
    pub(super) roots:
        Query<'w, 's, (Entity, &'static MenuFade, &'static Visibility), With<SaveLoadRoot>>,
    pub(super) contents: Query<'w, 's, Entity, With<SaveLoadContent>>,
    pub(super) parents: Query<'w, 's, &'static ChildOf>,
    pub(super) texts: Query<'w, 's, (Entity, &'static mut TextColor)>,
    pub(super) backgrounds: Query<'w, 's, (Entity, &'static mut BackgroundColor)>,
    pub(super) borders: Query<'w, 's, (Entity, &'static mut BorderColor)>,
    pub(super) images: Query<'w, 's, (Entity, &'static mut ImageNode)>,
}

#[derive(SystemParam)]
pub(crate) struct SaveSlotContext<'w, 's> {
    pub(super) project_root: Res<'w, PersistenceRoot>,
    pub(super) store: Res<'w, crate::runtime::resources::StoreCodec>,
    pub(super) state: Res<'w, crate::runtime::resources::GameState>,
    pub(super) windows: Query<'w, 's, &'static Window>,
    pub(super) images: ResMut<'w, Assets<Image>>,
    pub(super) commands: Commands<'w, 's>,
    pub(super) settings: Res<'w, crate::storage::settings::RuntimeSettings>,
    pub(super) preview_coordinator: Res<'w, crate::storage::save::SavePreviewCoordinator>,
    pub(super) save_previews: ResMut<'w, SavePreviewCache>,
}

#[derive(SystemParam)]
pub(crate) struct SaveDeleteContext<'w, 's> {
    pub(super) slots: Query<'w, 's, (&'static Interaction, &'static SaveLoadSlot)>,
    pub(super) ui: Res<'w, SaveLoadUi>,
    pub(super) request: Option<Res<'w, DialogRequest>>,
    pub(super) project_root: Res<'w, PersistenceRoot>,
    pub(super) store: Res<'w, crate::runtime::resources::StoreCodec>,
    pub(super) commands: Commands<'w, 's>,
    pub(super) settings: Res<'w, crate::storage::settings::RuntimeSettings>,
}
