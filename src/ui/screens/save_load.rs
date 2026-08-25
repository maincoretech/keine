use std::collections::HashMap;
use std::time::SystemTime;

use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, block_on, poll_once};
use bevy::text::FontWeight;
use bevy::ui::FocusPolicy;

use crate::render::blur::{DialogCamera, UiBlurCamera};
use crate::runtime::resources::PersistenceRoot;
use crate::ui::control_bar::{BlurStrength, ButtonAction, ControlInput, HoverAlpha, UiBlurSource};
use crate::ui::dialog::{DialogAction, DialogRequest};
use crate::ui::foundation::{
    SHELL_TRANSITION_WIDTH, SURFACE_ACTIVE_ALPHA, SURFACE_HOVER_ALPHA, UiFonts, UiSoundStyle,
    button_surface, ease_in_out_cubic, empty_slot_surface, exp_lerp, fill_node, logical_node_width,
    smoothstep, text, text_weight,
};
use crate::ui::menu::{
    MenuBack, MenuBlur, MenuFade, MenuRouteTransition, MenuSurface, MenuSurfaceState,
    PersistentMenu, active_route, begin_route_change, root_node, spawn_header_slot,
};

pub(crate) const PAGE_COUNT: u32 = 20;
pub(crate) const SLOTS_PER_PAGE: u32 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SaveLoadMode {
    Save,
    Load,
}

impl SaveLoadMode {
    fn watermark(self) -> &'static str {
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

pub(crate) fn save_load_open(ui: Res<SaveLoadUi>) -> bool {
    ui.mode.is_some()
}

#[derive(Component)]
pub(crate) struct SaveLoadRoot;

#[derive(Component)]
pub(crate) struct SaveLoadContent;

#[derive(Component)]
pub(crate) struct SaveLoadBlurProxy;

#[derive(Resource, Default)]
pub(crate) struct SavePreviewCache {
    ready: HashMap<u32, CachedPreview>,
    pending: HashMap<u32, Task<Option<LoadedPreview>>>,
}

struct CachedPreview {
    modified: Option<SystemTime>,
    handle: Handle<Image>,
}

struct LoadedPreview {
    modified: SystemTime,
    image: Image,
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

struct SaveContentContext<'a> {
    font: &'a Handle<Font>,
    project_root: &'a PersistenceRoot,
    store: &'a dyn keine_loader::StoreAdapter,
    program_fingerprint: u64,
    preview_cache: &'a mut SavePreviewCache,
}

#[derive(Resource, Default)]
pub(crate) struct SaveLoadPageTransition {
    active: bool,
    elapsed: f32,
    direction: f32,
}

impl SaveLoadPageTransition {
    const SECONDS: f32 = 0.22;

    fn begin(&mut self, direction: f32) {
        self.active = true;
        self.elapsed = 0.0;
        self.direction = direction.signum();
    }

    pub(crate) fn is_animating(&self) -> bool {
        self.active
    }
}

#[derive(Component)]
pub(crate) struct SaveLoadPreviewImage(u32);

#[derive(Component)]
pub(crate) struct SaveLoadGridViewport;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SaveLoadGridPhase {
    Incoming,
    Outgoing,
    Settled,
}

#[derive(Component)]
pub(crate) struct SaveLoadSlotGrid {
    phase: SaveLoadGridPhase,
}

#[derive(Default)]
pub(crate) struct SaveLoadContentFadeCache {
    active: bool,
    last_alpha: f32,
    text: HashMap<Entity, f32>,
    background: HashMap<Entity, f32>,
    border: HashMap<Entity, [f32; 4]>,
    image: HashMap<Entity, f32>,
}

#[derive(Component)]
pub(crate) struct SaveLoadSlot(pub(crate) u32);

#[derive(Component)]
pub(crate) struct SaveLoadPage(pub(crate) u32);

#[derive(Component)]
pub(crate) struct SaveLoadPageVisual {
    selected: bool,
    text: f32,
    press: f32,
}

const PAGE_HIGHLIGHT_TEXT_ALPHA: f32 = 0.67;

fn page_highlight_alpha(selected: bool, hovered: bool) -> f32 {
    if selected || hovered {
        SURFACE_ACTIVE_ALPHA
    } else {
        0.0
    }
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
    scale: f32,
    x: f32,
    y: f32,
}

impl SaveLoadSlotMotion {
    pub(crate) fn is_animating(&self, interaction: Interaction) -> bool {
        let target_scale = match interaction {
            Interaction::Pressed => 0.97,
            Interaction::Hovered => 0.985,
            Interaction::None => 1.0,
        };
        let (target_x, target_y) = match interaction {
            Interaction::Hovered | Interaction::Pressed => (0.0, 0.0),
            Interaction::None => (0.0, 0.0),
        };
        (self.scale - target_scale).abs() > 0.001
            || (self.x - target_x).abs() > 0.001
            || (self.y - target_y).abs() > 0.001
    }
}

type SettingsRootVisibilityQuery<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static mut Visibility),
    (
        With<crate::ui::settings_panel::SettingsRoot>,
        Without<crate::ui::settings_panel::SettingsBlurProxy>,
        Without<SaveLoadRoot>,
    ),
>;
type SettingsProxyVisibilityQuery<'w, 's> = Query<
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
type SaveLoadRootVisibilityQuery<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static mut Visibility),
    (With<SaveLoadRoot>, Without<SaveLoadBlurProxy>),
>;
type SaveLoadProxyVisibilityQuery<'w, 's> = Query<
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
    commands: Commands<'w, 's>,
    roots: SaveLoadRootVisibilityQuery<'w, 's>,
    grids: Query<'w, 's, (Entity, &'static mut SaveLoadSlotGrid)>,
    grid_viewports: Query<'w, 's, Entity, With<SaveLoadGridViewport>>,
    proxies: SaveLoadProxyVisibilityQuery<'w, 's>,
    camera: Query<'w, 's, Entity, With<DialogCamera>>,
    blur_camera: Query<'w, 's, Entity, With<UiBlurCamera>>,
    fonts: Res<'w, UiFonts>,
    project_root: Res<'w, PersistenceRoot>,
    store: Res<'w, crate::runtime::resources::StoreCodec>,
    state: Res<'w, crate::runtime::resources::GameState>,
    preview_cache: ResMut<'w, SavePreviewCache>,
    fades: Query<'w, 's, &'static mut MenuFade>,
    watermarks: Query<
        'w,
        's,
        (
            &'static mut crate::ui::settings_panel::SettingsWatermark,
            &'static mut Text,
        ),
    >,
    settings_roots: SettingsRootVisibilityQuery<'w, 's>,
    settings_proxies: SettingsProxyVisibilityQuery<'w, 's>,
    route_transition: Res<'w, MenuRouteTransition>,
}

#[derive(SystemParam)]
pub(crate) struct SaveLoadFadeContext<'w, 's> {
    roots: Query<'w, 's, (Entity, &'static MenuFade), With<SaveLoadRoot>>,
    contents: Query<'w, 's, Entity, With<SaveLoadContent>>,
    parents: Query<'w, 's, &'static ChildOf>,
    texts: Query<'w, 's, (Entity, &'static mut TextColor)>,
    backgrounds: Query<'w, 's, (Entity, &'static mut BackgroundColor)>,
    borders: Query<'w, 's, (Entity, &'static mut BorderColor)>,
    images: Query<'w, 's, (Entity, &'static mut ImageNode)>,
}

#[derive(SystemParam)]
pub(crate) struct SaveSlotContext<'w, 's> {
    project_root: Res<'w, PersistenceRoot>,
    store: Res<'w, crate::runtime::resources::StoreCodec>,
    state: Res<'w, crate::runtime::resources::GameState>,
    windows: Query<'w, 's, &'static Window>,
    images: ResMut<'w, Assets<Image>>,
    commands: Commands<'w, 's>,
    settings: Res<'w, crate::storage::settings::RuntimeSettings>,
    preview_coordinator: Res<'w, crate::storage::save::SavePreviewCoordinator>,
    save_previews: ResMut<'w, SavePreviewCache>,
}

#[derive(SystemParam)]
pub(crate) struct SaveDeleteContext<'w, 's> {
    slots: Query<'w, 's, (&'static Interaction, &'static SaveLoadSlot)>,
    ui: Res<'w, SaveLoadUi>,
    request: Option<Res<'w, DialogRequest>>,
    project_root: Res<'w, PersistenceRoot>,
    store: Res<'w, crate::runtime::resources::StoreCodec>,
    commands: Commands<'w, 's>,
    settings: Res<'w, crate::storage::settings::RuntimeSettings>,
}

pub fn toggle_save_load(
    keys: Res<ButtonInput<KeyCode>>,
    input: ControlInput,
    back: Query<&Interaction, (With<MenuBack>, Changed<Interaction>)>,
    mut ui: ResMut<SaveLoadUi>,
    mut settings: ResMut<crate::ui::settings_panel::SettingsUi>,
    mut transition: ResMut<SaveLoadPageTransition>,
    mut route_transition: ResMut<MenuRouteTransition>,
) {
    let requested = [ButtonAction::Save, ButtonAction::Load]
        .into_iter()
        .find(|action| input.pressed(*action));
    if let Some(action) = requested {
        let previous_route = active_route(&ui, &settings);
        ui.mode = match action {
            ButtonAction::Save => {
                if settings.open {
                    settings.open = false;
                }
                Some(SaveLoadMode::Save)
            }
            ButtonAction::Load => {
                if settings.open {
                    settings.open = false;
                }
                Some(SaveLoadMode::Load)
            }
            _ => ui.mode,
        };
        // SAVE and LOAD share one slot surface. Switching mode only changes the
        // operation semantics and selected tab; page motion belongs exclusively
        // to page-number navigation.
        transition.active = false;
        transition.elapsed = 0.0;
        transition.direction = 0.0;
        let next_route = active_route(&ui, &settings);
        begin_route_change(&mut route_transition, previous_route, next_route);
    }
    if ui.mode.is_some()
        && (keys.just_pressed(KeyCode::Escape)
            || back
                .iter()
                .any(|interaction| *interaction == Interaction::Pressed))
    {
        ui.mode = None;
    }
}

pub fn handle_save_load_page(
    interactions: Query<(&Interaction, &SaveLoadPage), Changed<Interaction>>,
    mut ui: ResMut<SaveLoadUi>,
    mut transition: ResMut<SaveLoadPageTransition>,
) {
    for (interaction, page) in &interactions {
        if *interaction == Interaction::Pressed && ui.page != page.0 {
            let direction = if page.0 > ui.page { 1.0 } else { -1.0 };
            ui.page = page.0;
            transition.begin(direction);
        }
    }
}

pub fn animate_page_transition(
    time: Res<Time>,
    ui: Res<SaveLoadUi>,
    mut transition: ResMut<SaveLoadPageTransition>,
) {
    if ui.mode.is_none() {
        transition.active = false;
        transition.elapsed = 0.0;
        transition.direction = 0.0;
    }
    if transition.active {
        transition.elapsed += time.delta_secs();
        if transition.elapsed >= SaveLoadPageTransition::SECONDS {
            transition.active = false;
            transition.elapsed = 0.0;
            transition.direction = 0.0;
        }
    }
}

pub fn sync_save_load(
    ui: Res<SaveLoadUi>,
    settings: Res<crate::ui::settings_panel::SettingsUi>,
    page_transition: Res<SaveLoadPageTransition>,
    mut context: SaveLoadSyncContext,
) {
    if !ui.is_changed() {
        return;
    }
    let Some(mode) = ui.mode else {
        for (entity, mut visibility) in &mut context.roots {
            if context.route_transition.is_animating() && settings.open {
                continue;
            }
            if let Ok(mut fade) = context.fades.get_mut(entity) {
                if settings.open {
                    fade.target = 0.0;
                } else {
                    fade.release_to_title(&mut visibility);
                }
            }
        }
        for (entity, mut visibility) in &mut context.proxies {
            if settings.open {
                *visibility = Visibility::Inherited;
            }
            if let Ok(mut fade) = context.fades.get_mut(entity) {
                if settings.open {
                    fade.target = 1.0;
                } else {
                    fade.release_to_title(&mut visibility);
                }
            }
        }
        if !settings.open {
            for (mut watermark, _) in &mut context.watermarks {
                watermark.hide();
            }
        }
        return;
    };
    if let Ok((root, mut visibility)) = context.roots.single_mut() {
        *visibility = Visibility::Inherited;
        if let Ok(mut fade) = context.fades.get_mut(root) {
            if context.route_transition.is_animating() {
                fade.current = 1.0;
            }
            fade.target = 1.0;
        }
        for (proxy, mut proxy_visibility) in &mut context.proxies {
            *proxy_visibility = Visibility::Inherited;
            if let Ok(mut fade) = context.fades.get_mut(proxy) {
                fade.target = 1.0;
            }
        }
        for (mut watermark, mut label) in &mut context.watermarks {
            watermark.show_label(&mut label, mode.watermark());
        }
        let Ok(viewport) = context.grid_viewports.single() else {
            return;
        };
        for (entity, mut grid) in &mut context.grids {
            if page_transition.is_animating() && grid.phase != SaveLoadGridPhase::Outgoing {
                grid.phase = SaveLoadGridPhase::Outgoing;
            } else {
                context.commands.entity(entity).despawn();
            }
        }
        context.commands.entity(viewport).with_children(|content| {
            spawn_slot_grid(
                content,
                &ui,
                mode,
                if page_transition.is_animating() {
                    SaveLoadGridPhase::Incoming
                } else {
                    SaveLoadGridPhase::Settled
                },
                &mut SaveContentContext {
                    font: &context.fonts.text,
                    project_root: &context.project_root,
                    store: context.store.0.as_ref(),
                    program_fingerprint: context.state.program_fingerprint,
                    preview_cache: &mut context.preview_cache,
                },
            );
        });
        return;
    }
    let (Ok(camera), Ok(blur_camera)) = (context.camera.single(), context.blur_camera.single())
    else {
        return;
    };
    let switching = context.route_transition.is_animating();
    if !switching {
        for (entity, mut visibility) in &mut context.settings_roots {
            if let Ok(mut fade) = context.fades.get_mut(entity) {
                fade.current = 0.0;
                fade.target = 0.0;
            }
            *visibility = Visibility::Hidden;
        }
    }
    let font = context.fonts.text.clone();
    if context.proxies.is_empty() {
        context
            .commands
            .spawn((
                Name::new("menu_blur"),
                SaveLoadBlurProxy,
                crate::ui::settings_panel::SettingsBlurProxy,
                MenuBlur,
                PersistentMenu,
                if switching {
                    MenuFade::visible()
                } else {
                    MenuFade::entering()
                },
                UiBlurSource,
                BlurStrength(if switching {
                    crate::ui::FULLSCREEN_BLUR_STRENGTH
                } else {
                    0.0
                }),
                fill_node(),
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, crate::ui::MENU_BACKDROP_ALPHA)),
                GlobalZIndex(179),
                UiTargetCamera(blur_camera),
                RenderLayers::layer(1),
            ))
            .with_children(|proxy| {
                crate::ui::settings_panel::spawn_menu_watermark(proxy, mode.watermark(), &font);
            });
    } else {
        for (entity, mut visibility) in &mut context.proxies {
            *visibility = Visibility::Inherited;
            if let Ok(mut fade) = context.fades.get_mut(entity) {
                fade.target = 1.0;
            }
        }
        for (_, mut visibility) in &mut context.settings_proxies {
            *visibility = Visibility::Inherited;
        }
    }
    for (mut watermark, mut label) in &mut context.watermarks {
        watermark.show_label(&mut label, mode.watermark());
    }
    context
        .commands
        .spawn((
            Name::new("save_load"),
            SaveLoadRoot,
            MenuSurfaceState::new(MenuSurface::standard(), switching),
            PersistentMenu,
            root_node(),
            BackgroundColor(Color::NONE),
            FocusPolicy::Block,
            GlobalZIndex(180),
            UiTargetCamera(camera),
            RenderLayers::layer(2),
        ))
        .with_children(|root| {
            spawn_header_slot(root);
            spawn_save_content(
                root,
                &ui,
                mode,
                &mut SaveContentContext {
                    font: &font,
                    project_root: &context.project_root,
                    store: context.store.0.as_ref(),
                    program_fingerprint: context.state.program_fingerprint,
                    preview_cache: &mut context.preview_cache,
                },
            );
        });
}

fn spawn_save_content(
    root: &mut ChildSpawnerCommands,
    ui: &SaveLoadUi,
    mode: SaveLoadMode,
    context: &mut SaveContentContext,
) {
    root.spawn((
        SaveLoadContent,
        UiTransform::default(),
        Node {
            position_type: PositionType::Relative,
            width: Val::Percent(100.0),
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            ..default()
        },
    ))
    .with_children(|content| {
        content
            .spawn((Node {
                width: Val::Percent(100.0),
                height: Val::Percent(7.0),
                // Match Config's 13.5 px content inset without moving the
                // save-slot grid below it.
                margin: UiRect {
                    top: Val::Px(13.5),
                    bottom: Val::Px(19.5),
                    ..default()
                },
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },))
            .with_children(|pages| {
                for page in 1..=PAGE_COUNT {
                    spawn_page_button(pages, page, ui.page, context.font);
                }
            });
        content
            .spawn((
                SaveLoadGridViewport,
                Node {
                    position_type: PositionType::Relative,
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    overflow: Overflow::clip(),
                    ..default()
                },
            ))
            .with_children(|viewport| {
                spawn_slot_grid(viewport, ui, mode, SaveLoadGridPhase::Settled, context)
            });
    });
}

fn spawn_slot_grid(
    content: &mut ChildSpawnerCommands,
    ui: &SaveLoadUi,
    mode: SaveLoadMode,
    phase: SaveLoadGridPhase,
    context: &mut SaveContentContext,
) {
    let first = (ui.page - 1) * SLOTS_PER_PAGE + 1;
    let last = first + SLOTS_PER_PAGE;
    context
        .preview_cache
        .ready
        .retain(|slot, _| (first..last).contains(slot));
    context
        .preview_cache
        .pending
        .retain(|slot, _| (first..last).contains(slot));
    content
        .spawn((
            SaveLoadSlotGrid { phase },
            UiTransform::default(),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                display: Display::Grid,
                grid_template_columns: RepeatedGridTrack::flex(5, 1.0),
                grid_template_rows: RepeatedGridTrack::flex(2, 1.0),
                column_gap: Val::Percent(1.8),
                row_gap: Val::Percent(4.0),
                ..default()
            },
        ))
        .with_children(|grid| {
            for slot in first..first + SLOTS_PER_PAGE {
                let preview = request_preview(context.project_root, slot, context.preview_cache);
                spawn_slot(grid, slot, mode, context, preview);
            }
        });
}

pub fn animate_save_load_content(
    mut context: SaveLoadFadeContext,
    mut cache: Local<SaveLoadContentFadeCache>,
) {
    let Some(_content) = context.contents.iter().next() else {
        cache.active = false;
        cache.text.clear();
        cache.background.clear();
        cache.border.clear();
        cache.image.clear();
        return;
    };
    let Ok((root, root_fade)) = context.roots.single() else {
        return;
    };
    let belongs_to = |entity: Entity, ancestor: Entity| {
        let mut current = entity;
        while let Ok(parent) = context.parents.get(current) {
            current = parent.parent();
            if current == ancestor {
                return true;
            }
        }
        false
    };

    let menu_alpha = smoothstep(root_fade.current);
    if menu_alpha >= 0.999 {
        if !cache.active {
            return;
        }
        restore_save_load_content(root, &mut context, &cache);
        cache.active = false;
        cache.last_alpha = 1.0;
        cache.text.clear();
        cache.background.clear();
        cache.border.clear();
        cache.image.clear();
        return;
    }

    if cache.active && (cache.last_alpha - menu_alpha).abs() < 0.0001 {
        return;
    }

    cache.active = true;
    cache.last_alpha = menu_alpha;
    for (entity, mut color) in &mut context.texts {
        if belongs_to(entity, root) {
            let alpha = menu_alpha;
            let base = *cache.text.entry(entity).or_insert_with(|| color.0.alpha());
            color.0 = color.0.with_alpha(base * alpha);
        }
    }
    for (entity, mut color) in &mut context.backgrounds {
        if belongs_to(entity, root) {
            let alpha = menu_alpha;
            let base = *cache
                .background
                .entry(entity)
                .or_insert_with(|| color.0.alpha());
            color.0 = color.0.with_alpha(base * alpha);
        }
    }
    for (entity, mut border) in &mut context.borders {
        if belongs_to(entity, root) {
            let alpha = menu_alpha;
            let base = *cache.border.entry(entity).or_insert_with(|| {
                [
                    border.top.alpha(),
                    border.right.alpha(),
                    border.bottom.alpha(),
                    border.left.alpha(),
                ]
            });
            border.top = border.top.with_alpha(base[0] * alpha);
            border.right = border.right.with_alpha(base[1] * alpha);
            border.bottom = border.bottom.with_alpha(base[2] * alpha);
            border.left = border.left.with_alpha(base[3] * alpha);
        }
    }
    for (entity, mut image) in &mut context.images {
        if belongs_to(entity, root) {
            let alpha = menu_alpha;
            let base = *cache
                .image
                .entry(entity)
                .or_insert_with(|| image.color.alpha());
            image.color = image.color.with_alpha(base * alpha);
        }
    }
}

pub fn animate_save_load_grid_track(
    transition: Res<SaveLoadPageTransition>,
    mut commands: Commands,
    mut grids: Query<(
        Entity,
        &mut SaveLoadSlotGrid,
        &mut UiTransform,
        &ComputedNode,
    )>,
) {
    if !transition.active {
        for (entity, mut grid, mut transform, _) in &mut grids {
            if grid.phase == SaveLoadGridPhase::Outgoing {
                commands.entity(entity).despawn();
            } else {
                grid.phase = SaveLoadGridPhase::Settled;
                transform.translation = Val2::ZERO;
            }
        }
        return;
    }
    let progress = ease_in_out_cubic(transition.elapsed / SaveLoadPageTransition::SECONDS);
    let width = grids
        .iter()
        .map(|(_, _, _, node)| logical_node_width(node))
        .fold(SHELL_TRANSITION_WIDTH, f32::max);
    for (_, grid, mut transform, _) in &mut grids {
        let x = match grid.phase {
            SaveLoadGridPhase::Incoming => transition.direction * width * (1.0 - progress),
            SaveLoadGridPhase::Outgoing => -transition.direction * width * progress,
            SaveLoadGridPhase::Settled => 0.0,
        };
        transform.translation = Val2::px(x, 0.0);
    }
}

fn restore_save_load_content(
    root: Entity,
    context: &mut SaveLoadFadeContext,
    cache: &SaveLoadContentFadeCache,
) {
    let belongs = |entity: Entity| {
        let mut current = entity;
        while let Ok(parent) = context.parents.get(current) {
            current = parent.parent();
            if current == root {
                return true;
            }
        }
        false
    };
    for (entity, mut color) in &mut context.texts {
        if belongs(entity)
            && let Some(alpha) = cache.text.get(&entity)
        {
            color.0 = color.0.with_alpha(*alpha);
        }
    }
    for (entity, mut color) in &mut context.backgrounds {
        if belongs(entity)
            && let Some(alpha) = cache.background.get(&entity)
        {
            color.0 = color.0.with_alpha(*alpha);
        }
    }
    for (entity, mut border) in &mut context.borders {
        if belongs(entity)
            && let Some(alpha) = cache.border.get(&entity)
        {
            border.top = border.top.with_alpha(alpha[0]);
            border.right = border.right.with_alpha(alpha[1]);
            border.bottom = border.bottom.with_alpha(alpha[2]);
            border.left = border.left.with_alpha(alpha[3]);
        }
    }
    for (entity, mut image) in &mut context.images {
        if belongs(entity)
            && let Some(alpha) = cache.image.get(&entity)
        {
            image.color = image.color.with_alpha(*alpha);
        }
    }
}

fn spawn_page_button(
    pages: &mut ChildSpawnerCommands,
    page: u32,
    selected_page: u32,
    font: &Handle<Font>,
) {
    let selected = page == selected_page;
    pages
        .spawn((
            Button,
            UiSoundStyle::Switch,
            SaveLoadPage(page),
            SaveLoadPageVisual {
                selected,
                text: if selected {
                    PAGE_HIGHLIGHT_TEXT_ALPHA
                } else {
                    0.2
                },
                press: 0.0,
            },
            HoverAlpha {
                target: page_highlight_alpha(selected, false),
                current: page_highlight_alpha(selected, false),
                active: selected,
                active_alpha: SURFACE_ACTIVE_ALPHA,
                hover_alpha: SURFACE_HOVER_ALPHA,
                ..default()
            },
            UiTransform::default(),
            Node {
                width: Val::Px(60.0),
                height: Val::Px(58.5),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(button_surface(page_highlight_alpha(selected, false))),
        ))
        .with_child((
            SaveLoadPageLabel,
            text_weight(
                page.to_string(),
                font,
                24.0,
                if selected {
                    PAGE_HIGHLIGHT_TEXT_ALPHA
                } else {
                    0.2
                },
                if selected {
                    FontWeight::BOLD
                } else {
                    FontWeight::NORMAL
                },
            ),
        ));
}

fn spawn_slot(
    grid: &mut ChildSpawnerCommands,
    slot: u32,
    mode: SaveLoadMode,
    context: &SaveContentContext,
    preview: Option<Handle<Image>>,
) {
    use crate::storage::save::SlotStatus;

    let status = crate::storage::save::inspect_slot(context.store, slot, context.project_root);
    let empty = matches!(status, SlotStatus::Empty);
    let compatible = matches!(
        &status,
        SlotStatus::Ready(metadata)
            if metadata.program_fingerprint == context.program_fingerprint
    );
    let preview = compatible.then_some(preview).flatten();
    let enabled = mode == SaveLoadMode::Save || compatible;
    let ready = compatible;
    let primary_text_alpha = if ready {
        0.72
    } else if empty {
        0.34
    } else {
        0.28
    };
    let secondary_text_alpha = if ready {
        0.58
    } else if empty {
        0.28
    } else {
        0.24
    };
    let base_alpha = if enabled { 0.11 } else { 0.06 };
    let detail = match &status {
        SlotStatus::Empty => String::new(),
        SlotStatus::Corrupt => "CORRUPT SLOT\nSave here to replace it".to_owned(),
        SlotStatus::Unsupported(version) => {
            format!("NEWER SAVE · v{version}\nCannot load in this engine")
        }
        SlotStatus::Ready(_) if !compatible => {
            "DIFFERENT SCRIPT BUILD\nSave here to replace it".to_owned()
        }
        SlotStatus::Ready(meta) => meta.text.clone(),
    };
    grid.spawn((
        Button,
        SaveLoadSlot(slot),
        SaveLoadSlotMotion {
            scale: 1.0,
            x: 0.0,
            y: 0.0,
        },
        Interaction::None,
        UiTransform::default(),
        Node {
            flex_direction: FlexDirection::Column,
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(if empty {
            empty_slot_surface()
        } else {
            Color::srgba(0.0, 0.0, 0.0, base_alpha)
        }),
    ))
    .with_children(|slot_node| {
        slot_node.spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(12.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                ..default()
            },
            children![
                (
                    Node {
                        width: Val::Percent(22.0),
                        height: Val::Percent(100.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(if empty {
                        Color::NONE
                    } else {
                        Color::srgba(0.0, 0.0, 0.0, 0.48)
                    }),
                    children![text_weight(
                        slot.to_string(),
                        context.font,
                        21.75,
                        primary_text_alpha,
                        FontWeight::BOLD,
                    )]
                ),
                (
                    Node {
                        width: Val::Percent(78.0),
                        height: Val::Percent(100.0),
                        padding: UiRect::left(Val::Px(7.5)),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(if empty {
                        Color::NONE
                    } else {
                        Color::srgba(0.0, 0.0, 0.0, 0.32)
                    }),
                    children![text(
                        slot_time(&status),
                        context.font,
                        15.75,
                        secondary_text_alpha,
                    )]
                )
            ],
        ));
        slot_node.spawn((
            SaveLoadPreviewImage(slot),
            preview.map_or_else(ImageNode::default, ImageNode::new),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(48.0),
                flex_shrink: 0.0,
                ..default()
            },
        ));
        slot_node.spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(40.0),
                padding: UiRect::axes(Val::Px(10.5), Val::Px(6.75)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(5.25),
                ..default()
            },
            BackgroundColor(if empty {
                Color::NONE
            } else {
                Color::srgba(0.0, 0.0, 0.0, 0.52)
            }),
            children![
                text_weight(
                    slot_speaker(&status),
                    context.font,
                    19.5,
                    primary_text_alpha,
                    FontWeight::BOLD,
                ),
                text(detail, context.font, 17.25, secondary_text_alpha)
            ],
        ));
    });
}

pub fn animate_save_load_slots(
    time: Res<Time>,
    mut slots: Query<(&Interaction, &mut SaveLoadSlotMotion, &mut UiTransform)>,
) {
    let amount = exp_lerp(time.delta_secs(), 14.0);
    for (interaction, mut motion, mut transform) in &mut slots {
        let target_scale = match interaction {
            Interaction::Pressed => 0.97,
            Interaction::Hovered => 0.985,
            Interaction::None => 1.0,
        };
        let (target_x, target_y) = match interaction {
            Interaction::Hovered | Interaction::Pressed => (0.0, 0.0),
            Interaction::None => (0.0, 0.0),
        };
        if (motion.scale - target_scale).abs() < 0.001
            && (motion.x - target_x).abs() < 0.001
            && (motion.y - target_y).abs() < 0.001
        {
            continue;
        }
        motion.scale += (target_scale - motion.scale) * amount;
        motion.x += (target_x - motion.x) * amount;
        motion.y += (target_y - motion.y) * amount;
        transform.scale = Vec2::splat(motion.scale);
        transform.translation = Val2::px(motion.x, motion.y);
    }
}

pub fn animate_save_load_pages(
    time: Res<Time>,
    ui: Res<SaveLoadUi>,
    mut pages: Query<(
        &Interaction,
        &SaveLoadPage,
        &mut SaveLoadPageVisual,
        &mut HoverAlpha,
        &mut UiTransform,
        &Children,
    )>,
    mut labels: Query<(&mut TextColor, &mut TextFont), With<SaveLoadPageLabel>>,
) {
    let amount = exp_lerp(time.delta_secs(), 10.0);
    for (interaction, page, mut visual, mut hover, mut transform, children) in &mut pages {
        let selected = page.0 == ui.page;
        let selection_changed = visual.selected != selected;
        visual.selected = selected;
        let hovered = matches!(interaction, Interaction::Hovered | Interaction::Pressed);
        hover.active = selected;
        hover.active_alpha = SURFACE_ACTIVE_ALPHA;
        hover.hover_alpha = SURFACE_HOVER_ALPHA;
        hover.target = page_highlight_alpha(selected, hovered);
        let target_text = if visual.selected || hovered {
            PAGE_HIGHLIGHT_TEXT_ALPHA
        } else {
            0.2
        };
        if *interaction == Interaction::Pressed {
            visual.press = 1.0;
        } else {
            visual.press *= (-time.delta_secs() * 18.0).exp();
            if visual.press < 0.001 {
                visual.press = 0.0;
            }
        }
        if selection_changed {
            visual.text = target_text;
        }
        if !selection_changed && (visual.text - target_text).abs() < 0.001 && visual.press == 0.0 {
            continue;
        }
        visual.text += (target_text - visual.text) * amount;
        transform.scale = Vec2::splat(1.0 - 0.08 * visual.press);
        for child in children.iter() {
            if let Ok((mut color, mut font)) = labels.get_mut(child) {
                color.0 = Color::srgba(1.0, 1.0, 1.0, visual.text);
                font.weight = if visual.selected || hovered {
                    FontWeight::BOLD
                } else {
                    FontWeight::NORMAL
                };
            }
        }
    }
}

fn slot_time(status: &crate::storage::save::SlotStatus) -> String {
    match status {
        crate::storage::save::SlotStatus::Ready(metadata) => relative_time(metadata.saved_at_unix),
        _ => String::new(),
    }
}

fn slot_speaker(status: &crate::storage::save::SlotStatus) -> String {
    match status {
        crate::storage::save::SlotStatus::Ready(metadata) if !metadata.speaker.is_empty() => {
            metadata.speaker.clone()
        }
        crate::storage::save::SlotStatus::Ready(_) => " ".into(),
        _ => String::new(),
    }
}

fn request_preview(
    project_root: &PersistenceRoot,
    slot: u32,
    cache: &mut SavePreviewCache,
) -> Option<Handle<Image>> {
    if let Some(cached) = cache.ready.get(&slot)
        && cached.modified.is_none()
    {
        return Some(cached.handle.clone());
    }
    let path = crate::storage::save::preview_path(project_root, slot);
    let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
    if let Some(cached) = cache.ready.get(&slot)
        && cached.modified == Some(modified)
    {
        return Some(cached.handle.clone());
    }
    let project_root = project_root.to_path_buf();
    cache.pending.entry(slot).or_insert_with(|| {
        IoTaskPool::get().spawn(async move {
            let bytes = crate::storage::save::read_preview(&project_root, slot).ok()?;
            let image = crate::scene::images::decode_preview(&bytes).ok()?;
            Some(LoadedPreview { modified, image })
        })
    });
    None
}

pub fn poll_preview_tasks(
    mut cache: ResMut<SavePreviewCache>,
    mut images: ResMut<Assets<Image>>,
    mut previews: Query<(&SaveLoadPreviewImage, &mut ImageNode)>,
) {
    let completed = cache
        .pending
        .iter_mut()
        .filter_map(|(slot, task)| block_on(poll_once(task)).map(|result| (*slot, result)))
        .collect::<Vec<_>>();
    for (slot, result) in completed {
        cache.pending.remove(&slot);
        let Some(loaded) = result else { continue };
        let handle = images.add(loaded.image);
        cache.ready.insert(
            slot,
            CachedPreview {
                modified: Some(loaded.modified),
                handle: handle.clone(),
            },
        );
        for (preview, mut image) in &mut previews {
            if preview.0 == slot {
                image.image = handle.clone();
            }
        }
    }
}

fn relative_time(saved_at_unix: u64) -> String {
    if saved_at_unix == 0 {
        return "legacy save".into();
    }
    format_utc(saved_at_unix)
}

fn format_utc(timestamp: u64) -> String {
    let days = (timestamp / 86_400) as i64;
    let seconds = timestamp % 86_400;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_piece = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_piece + 2) / 5 + 1;
    let month = month_piece + if month_piece < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    format!(
        "{year}/{month}/{day} {:02}:{:02}:{:02}",
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60
    )
}

pub fn handle_save_load_slot(
    interactions: Query<(&Interaction, &SaveLoadSlot), Changed<Interaction>>,
    mut ui: ResMut<SaveLoadUi>,
    mut context: SaveSlotContext,
) {
    let Some(mode) = ui.mode else { return };
    let Some(slot) = interactions
        .iter()
        .find_map(|(interaction, slot)| (*interaction == Interaction::Pressed).then_some(slot.0))
    else {
        return;
    };
    let status =
        crate::storage::save::inspect_slot(context.store.0.as_ref(), slot, &context.project_root);
    if mode == SaveLoadMode::Save && context.state.ended {
        return;
    }
    if mode == SaveLoadMode::Load
        && !matches!(
            &status,
            crate::storage::save::SlotStatus::Ready(metadata)
                if metadata.program_fingerprint == context.state.program_fingerprint
        )
    {
        return;
    }
    if mode == SaveLoadMode::Save && !matches!(status, crate::storage::save::SlotStatus::Ready(_)) {
        match crate::storage::save::save_game(
            context.store.0.as_ref(),
            &context.state,
            slot,
            &context.project_root,
        ) {
            Ok(()) => {
                ui.set_changed();
                context.save_previews.invalidate(slot);
                let generation = crate::ui::dialog::replace_preview_generation(
                    &context.preview_coordinator,
                    &context.project_root,
                    slot,
                );
                if let Ok(window) = context.windows.single() {
                    crate::ui::dialog::capture_save_preview(
                        &mut context.commands,
                        &mut context.images,
                        Vec2::new(window.width(), window.height()),
                        slot,
                        generation,
                    );
                }
            }
            Err(error) => {
                log::error!("save slot {slot} failed: {error:#}");
                if !context.state.persistence_safety().is_exact() {
                    context
                        .commands
                        .insert_resource(DialogRequest::confirmation(
                            crate::ui::support::i18n::tr(
                                context.settings.locale,
                                crate::ui::support::i18n::UiText::SaveUnavailableDuringPresentation,
                            ),
                            DialogAction::Noop,
                        ));
                }
            }
        }
        return;
    }
    let (title, action) = match mode {
        SaveLoadMode::Save => (
            crate::ui::support::i18n::overwrite_slot(context.settings.locale, slot),
            DialogAction::SaveSlot(slot),
        ),
        SaveLoadMode::Load => (
            crate::ui::support::i18n::load_slot(context.settings.locale, slot),
            DialogAction::LoadSlot(slot),
        ),
    };
    context
        .commands
        .insert_resource(DialogRequest::confirmation(title, action));
}

pub fn handle_save_delete(mouse: Res<ButtonInput<MouseButton>>, mut context: SaveDeleteContext) {
    if context.ui.mode.is_none()
        || context.request.is_some()
        || !mouse.just_pressed(MouseButton::Right)
    {
        return;
    }
    let Some(slot) = context
        .slots
        .iter()
        .find_map(|(interaction, slot)| (*interaction == Interaction::Hovered).then_some(slot.0))
    else {
        return;
    };
    if !matches!(
        crate::storage::save::inspect_slot(context.store.0.as_ref(), slot, &context.project_root),
        crate::storage::save::SlotStatus::Ready(_)
    ) {
        return;
    }
    context
        .commands
        .insert_resource(DialogRequest::confirmation(
            crate::ui::support::i18n::delete_slot(context.settings.locale, slot),
            DialogAction::DeleteSlot(slot),
        ));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visibility_query_contract(
        _roots: SaveLoadRootVisibilityQuery,
        _proxies: SaveLoadProxyVisibilityQuery,
        _settings_roots: SettingsRootVisibilityQuery,
        _settings_proxies: SettingsProxyVisibilityQuery,
    ) {
    }

    #[test]
    fn menu_visibility_queries_are_disjoint() {
        let mut app = App::new();
        app.add_systems(Update, visibility_query_contract);
        app.update();
    }

    #[test]
    fn page_hover_and_selection_share_color_without_bottom_highlight() {
        fn spawn(mut commands: Commands) {
            let font = Handle::<Font>::default();
            commands.spawn_empty().with_children(|pages| {
                spawn_page_button(pages, 1, 1, &font);
            });
        }

        assert_eq!(
            page_highlight_alpha(true, false),
            page_highlight_alpha(false, true)
        );

        let mut app = App::new();
        app.add_systems(Update, spawn);
        app.update();
        let world = app.world_mut();
        let page = {
            let mut pages = world.query_filtered::<Entity, With<SaveLoadPage>>();
            pages.single(world).expect("one page button")
        };
        assert_eq!(
            world.get::<Node>(page).expect("page button node").border,
            UiRect::ZERO
        );
        assert_eq!(
            world
                .get::<BackgroundColor>(page)
                .expect("page button background")
                .0,
            button_surface(SURFACE_ACTIVE_ALPHA)
        );
        let hover = world
            .get::<HoverAlpha>(page)
            .expect("shared page hover surface");
        assert_eq!(
            (
                hover.current,
                hover.target,
                hover.active_alpha,
                hover.hover_alpha
            ),
            (
                SURFACE_ACTIVE_ALPHA,
                SURFACE_ACTIVE_ALPHA,
                SURFACE_ACTIVE_ALPHA,
                SURFACE_HOVER_ALPHA
            )
        );
    }
}
