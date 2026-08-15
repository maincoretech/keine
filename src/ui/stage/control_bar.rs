// WebGAL-style control bar icon definitions and interaction.
// Both top and bottom bars spawn as children of TextBoxRoot in textbox.rs.
use bevy::{
    ecs::system::SystemParam,
    prelude::*,
    tasks::{IoTaskPool, Task, futures_lite::future},
};
use keine_core::State;
use keine_loader::StoreMetadata;
use std::{path::Path, time::Duration};

use crate::runtime::resources::ProjectRoot;
use crate::storage::save::QUICK_SAVE_SLOT;
use crate::ui::dialog::{DialogAction, DialogRequest};
use crate::ui::foundation::{SURFACE_HOVER_ALPHA, button_surface, exp_lerp};
use crate::ui::input_scope::UiInputScope;
use crate::ui::textbox::ContentRoot;

const QSAVE_LABEL: &str = "Q\u{00b7}SAVE";
const QLOAD_LABEL: &str = "Q\u{00b7}LOAD";

#[derive(Component)]
pub(crate) struct ControlBarTop;
#[derive(Component)]
pub(crate) struct ControlBarBot;

/// Identifies which button was clicked, attached to each Button entity at spawn.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ButtonAction {
    // Top row
    Backlog,
    Replay,
    Auto,
    Skip,
    Hide,
    Lock,
    // Bottom row
    QuickSave,
    QuickLoad,
    Save,
    Load,
    System,
    Title,
}

#[derive(SystemParam)]
pub(crate) struct ControlInput<'w, 's> {
    controls: Query<'w, 's, (&'static Interaction, &'static ButtonAction), Changed<Interaction>>,
    actions: Res<'w, crate::runtime::platform::InputActions>,
    scope: Res<'w, UiInputScope>,
}

impl ControlInput<'_, '_> {
    pub(crate) fn pressed(&self, action: ButtonAction) -> bool {
        self.actions.shortcut == Some(action)
            || self.controls.iter().any(|(interaction, candidate)| {
                *interaction == Interaction::Pressed && *candidate == action
            })
    }

    pub(crate) fn pressed_on_stage(&self, action: ButtonAction) -> bool {
        *self.scope == UiInputScope::Stage && self.pressed(action)
    }
}

#[derive(Resource, Default)]
pub(crate) struct QuickSavePreview {
    pub(crate) state: Option<QuickSaveSnapshot>,
    pub(crate) image: Option<Handle<Image>>,
}

#[derive(Resource, Default)]
pub(crate) struct QuickSavePreviewLoad {
    task: Option<Task<QuickSaveDetails>>,
    resolved: bool,
}

pub(crate) struct QuickSaveDetails {
    pub(crate) image: Option<Image>,
}

impl QuickSavePreviewLoad {
    pub(crate) fn request(&mut self, project_root: &Path) {
        if self.task.is_some() || self.resolved {
            return;
        }
        let project_root = project_root.to_owned();
        self.task = Some(IoTaskPool::get().spawn(async move {
            let image = crate::storage::save::read_preview(&project_root, QUICK_SAVE_SLOT)
                .ok()
                .and_then(|bytes| crate::scene::images::decode_preview(&bytes).ok());
            QuickSaveDetails { image }
        }));
    }

    pub(crate) fn poll(&mut self) -> Option<QuickSaveDetails> {
        let result = self
            .task
            .as_mut()
            .and_then(|task| future::block_on(future::poll_once(task)));
        if result.is_some() {
            self.task = None;
            self.resolved = true;
        }
        result
    }
}

impl QuickSavePreview {
    pub(crate) fn can_continue(&self, program_fingerprint: u64) -> bool {
        self.state
            .as_ref()
            .is_some_and(|state| state.playable && state.program_fingerprint == program_fingerprint)
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct QuickSaveSnapshot {
    program_fingerprint: u64,
    playable: bool,
    pub(crate) background: Option<String>,
    pub(crate) speaker: String,
    pub(crate) dialogue: String,
}

impl From<&State> for QuickSaveSnapshot {
    fn from(state: &State) -> Self {
        let (speaker, dialogue) = state.dialogue.as_ref().map_or_else(
            || (String::new(), String::new()),
            |dialogue| (dialogue.speaker.clone(), dialogue.text.clone()),
        );
        Self {
            program_fingerprint: state.program_fingerprint,
            playable: !state.ended,
            background: state.bg.clone(),
            speaker,
            dialogue,
        }
    }
}

impl From<StoreMetadata> for QuickSaveSnapshot {
    fn from(metadata: StoreMetadata) -> Self {
        // Slot 0 is only written by quick_save_on_exit while gameplay is active.
        // Full state validation still occurs when the player actually continues.
        Self {
            program_fingerprint: metadata.program_fingerprint,
            playable: true,
            background: None,
            speaker: metadata.speaker,
            dialogue: metadata.text,
        }
    }
}

#[derive(Component)]
pub(crate) struct QuickPreviewPanel {
    pub(crate) owner: ButtonAction,
}

#[derive(Component, Default)]
pub(crate) struct QuickPreviewFade {
    pub(crate) target: f32,
    pub(crate) current: f32,
}

#[derive(Component)]
pub(crate) struct QuickPreviewSurface;

#[derive(Component)]
pub(crate) struct QuickPreviewVisual {
    pub(crate) owner: ButtonAction,
    pub(crate) base_alpha: f32,
}

#[derive(Component)]
pub(crate) struct QuickPreviewImage;

#[derive(Component)]
pub(crate) struct QuickPreviewContent;

#[derive(Component)]
pub(crate) struct QuickPreviewSpeaker;

#[derive(Component)]
pub(crate) struct QuickPreviewDialogue;

#[derive(Component)]
pub(crate) struct QuickPreviewEmpty;

type EmptyPreviewQuery<'w, 's> = Query<
    'w,
    's,
    &'static mut Node,
    (
        With<QuickPreviewEmpty>,
        Without<QuickPreviewContent>,
        Without<QuickPreviewImage>,
    ),
>;

#[derive(SystemParam)]
pub(crate) struct QuickPreviewContentQueries<'w, 's> {
    images: Query<'w, 's, (&'static mut ImageNode, &'static mut Node), With<QuickPreviewImage>>,
    contents:
        Query<'w, 's, &'static mut Node, (With<QuickPreviewContent>, Without<QuickPreviewImage>)>,
    speakers: Query<'w, 's, &'static mut Text, With<QuickPreviewSpeaker>>,
    dialogues: Query<
        'w,
        's,
        &'static mut Text,
        (With<QuickPreviewDialogue>, Without<QuickPreviewSpeaker>),
    >,
    empty: EmptyPreviewQuery<'w, 's>,
}

type PreviewAnimationQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static QuickPreviewPanel,
        &'static mut QuickPreviewFade,
        &'static mut Node,
        Option<&'static QuickPreviewSurface>,
        Option<&'static mut BackgroundColor>,
        Option<&'static mut BlurStrength>,
    ),
>;

/// Per-button hover alpha state for CSS-like transition animation.
#[derive(Component)]
pub(crate) struct HoverAlpha {
    pub(crate) target: f32,
    pub(crate) current: f32,
    /// Resting fill for neutral cards and compact controls.
    pub(crate) idle_alpha: f32,
    /// When true (toggle is on), the surface rests at `active_alpha`.
    pub(crate) active: bool,
    pub(crate) active_alpha: f32,
    pub(crate) hover_alpha: f32,
}

impl Default for HoverAlpha {
    fn default() -> Self {
        Self {
            target: 0.0,
            current: 0.0,
            idle_alpha: 0.0,
            active: false,
            active_alpha: SURFACE_HOVER_ALPHA,
            hover_alpha: SURFACE_HOVER_ALPHA,
        }
    }
}

/// Toggle state for binary buttons in the top control bar.
#[derive(Resource)]
pub(crate) struct ToggleStates {
    pub auto: bool,
    pub skip: bool,
    pub skip_mode: SkipMode,
    pub hide: bool,
    /// Default: locked (control bars always visible).
    pub lock: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SkipMode {
    #[default]
    Read,
    All,
}

impl Default for ToggleStates {
    fn default() -> Self {
        Self {
            auto: false,
            skip: false,
            skip_mode: SkipMode::Read,
            hide: false,
            lock: true,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ControlItem {
    pub icon: char,
    pub label: &'static str,
    pub action: ButtonAction,
}

pub(crate) const TOP_ITEMS: &[ControlItem] = &[
    ControlItem {
        icon: '\u{f3b9}',
        label: "file-text",
        action: ButtonAction::Backlog,
    },
    ControlItem {
        icon: '\u{f116}',
        label: "arrow-clockwise",
        action: ButtonAction::Replay,
    },
    ControlItem {
        icon: '\u{f4f5}',
        label: "play",
        action: ButtonAction::Auto,
    },
    ControlItem {
        icon: '\u{f7f4}',
        label: "fast-forward",
        action: ButtonAction::Skip,
    },
    ControlItem {
        icon: '\u{f340}',
        label: "eye-slash",
        action: ButtonAction::Hide,
    },
    ControlItem {
        icon: '\u{f47b}',
        label: "lock",
        action: ButtonAction::Lock,
    },
];

pub(crate) const BOTTOM_ITEMS: &[ControlItem] = &[
    ControlItem {
        icon: '\u{f27e}',
        label: QSAVE_LABEL,
        action: ButtonAction::QuickSave,
    },
    ControlItem {
        icon: '\u{f281}',
        label: QLOAD_LABEL,
        action: ButtonAction::QuickLoad,
    },
    ControlItem {
        icon: '\u{f7e4}',
        label: "SAVE",
        action: ButtonAction::Save,
    },
    ControlItem {
        icon: '\u{f3d8}',
        label: "LOAD",
        action: ButtonAction::Load,
    },
    ControlItem {
        icon: '\u{f789}',
        label: "SYSTEM",
        action: ButtonAction::System,
    },
    ControlItem {
        icon: '\u{f425}',
        label: "TITLE",
        action: ButtonAction::Title,
    },
];

// ── Interaction systems ──

/// Sets the target hover alpha on interaction change, respecting active toggle state.
/// Suppresses hover on all buttons except Hide when hide mode is active.
pub fn set_hover_target(
    toggles: Res<ToggleStates>,
    mut q: Query<(&Interaction, &mut HoverAlpha, Option<&ButtonAction>), Changed<Interaction>>,
) {
    for (interaction, mut ha, action) in q.iter_mut() {
        // In hide mode, only the hide button responds to hover
        if toggles.hide && !matches!(action, Some(ButtonAction::Hide)) {
            ha.target = 0.0;
            continue;
        }
        let hover = matches!(interaction, Interaction::Hovered | Interaction::Pressed);
        ha.target = if hover {
            ha.hover_alpha
        } else if ha.active {
            ha.active_alpha
        } else {
            ha.idle_alpha
        };
    }
}

/// Smoothly lerps hover alpha towards target, applying to BackgroundColor.
pub fn animate_hover(time: Res<Time>, mut q: Query<(&mut HoverAlpha, &mut BackgroundColor)>) {
    let amount = exp_lerp(time.delta_secs(), 12.0);
    for (mut ha, mut bg) in q.iter_mut() {
        if (ha.current - ha.target).abs() < 0.001 {
            continue;
        }
        ha.current += (ha.target - ha.current) * amount;
        if ha.current < 0.002 {
            bg.0 = Color::NONE;
        } else {
            bg.0 = button_surface(ha.current);
        }
    }
}

/// Dispatches button clicks: toggle state for binary buttons,
/// save/load for bottom row, log for the rest.
pub fn handle_button_click(
    mut q: Query<(&Interaction, &ButtonAction, &mut HoverAlpha), Changed<Interaction>>,
    actions: Res<crate::runtime::platform::InputActions>,
    scope: Res<UiInputScope>,
    mut toggles: ResMut<ToggleStates>,
    mut commands: Commands,
    settings: Res<crate::storage::settings::RuntimeSettings>,
) {
    let shortcut = match (actions.shortcut, *scope) {
        (
            Some(action @ (ButtonAction::Hide | ButtonAction::QuickSave | ButtonAction::QuickLoad)),
            UiInputScope::Stage,
        ) => Some(action),
        (Some(ButtonAction::Title), UiInputScope::Stage | UiInputScope::Menu) => {
            Some(ButtonAction::Title)
        }
        _ => None,
    };
    if let Some(action) = shortcut {
        perform_button_action(action, &mut toggles, &mut commands, &settings, None);
    }

    for (interaction, action, mut ha) in q.iter_mut() {
        if !matches!(interaction, Interaction::Pressed) {
            continue;
        }
        // In hide mode, only the hide button responds to clicks
        if toggles.hide && !matches!(action, ButtonAction::Hide) {
            continue;
        }
        perform_button_action(
            *action,
            &mut toggles,
            &mut commands,
            &settings,
            Some(&mut ha),
        );
    }
}

fn perform_button_action(
    action: ButtonAction,
    toggles: &mut ToggleStates,
    commands: &mut Commands,
    settings: &crate::storage::settings::RuntimeSettings,
    hover: Option<&mut HoverAlpha>,
) {
    match action {
        ButtonAction::Auto => {
            toggles.auto = !toggles.auto;
            if let Some(hover) = hover {
                hover.active = toggles.auto;
                hover.target = if toggles.auto {
                    hover.active_alpha
                } else {
                    0.0
                };
            }
        }
        ButtonAction::Skip => {
            toggles.skip = !toggles.skip;
            if let Some(hover) = hover {
                hover.active = toggles.skip;
                hover.target = if toggles.skip {
                    hover.active_alpha
                } else {
                    0.0
                };
            }
        }
        ButtonAction::Hide => toggles.hide = !toggles.hide,
        ButtonAction::Lock => toggles.lock = !toggles.lock,
        ButtonAction::QuickSave => {
            commands.insert_resource(DialogRequest::confirmation(
                crate::ui::support::i18n::tr(
                    settings.locale,
                    crate::ui::support::i18n::UiText::ConfirmQuickSave,
                ),
                DialogAction::QuickSave,
            ));
        }
        ButtonAction::QuickLoad => {
            commands.insert_resource(DialogRequest::confirmation(
                crate::ui::support::i18n::tr(
                    settings.locale,
                    crate::ui::support::i18n::UiText::ConfirmQuickLoad,
                ),
                DialogAction::QuickLoad,
            ));
        }
        ButtonAction::Title => {
            commands.insert_resource(DialogRequest::confirmation(
                crate::ui::support::i18n::tr(
                    settings.locale,
                    crate::ui::support::i18n::UiText::ConfirmReturnTitle,
                ),
                DialogAction::BackToTitle,
            ));
        }
        ButtonAction::Save | ButtonAction::Load | ButtonAction::System => {}
        ButtonAction::Backlog | ButtonAction::Replay => {
            log::info!("[click] {action:?}");
        }
    }
}

pub fn load_quick_save_preview(
    project_root: Res<ProjectRoot>,
    store: Res<crate::runtime::resources::StoreCodec>,
    mut preview: ResMut<QuickSavePreview>,
) {
    preview.state = match crate::storage::save::inspect_slot(
        store.0.as_ref(),
        QUICK_SAVE_SLOT,
        &project_root,
    ) {
        crate::storage::save::SlotStatus::Ready(metadata) => Some(metadata.into()),
        _ => None,
    };
    preview.image = None;
}

pub fn show_quick_preview(
    buttons: Query<(&Interaction, &ButtonAction), Changed<Interaction>>,
    mut previews: Query<(&QuickPreviewPanel, &mut QuickPreviewFade, &mut Node)>,
) {
    for (interaction, action) in &buttons {
        if !matches!(action, ButtonAction::QuickSave | ButtonAction::QuickLoad) {
            continue;
        }
        for (preview, mut fade, mut node) in &mut previews {
            if preview.owner == *action {
                fade.target = if matches!(interaction, Interaction::Hovered | Interaction::Pressed)
                {
                    node.display = Display::Flex;
                    1.0
                } else {
                    0.0
                };
            }
        }
    }
}

/// Runs a short CSS-like transition for the preview surface, its content, and
/// the matching regional blur proxy. Keeping the proxy alive until fade-out is
/// complete avoids a sharp blur pop at either end of the animation.
pub fn animate_quick_previews(
    time: Res<Time>,
    mut panels: PreviewAnimationQuery,
    mut text: Query<(&QuickPreviewVisual, &mut TextColor)>,
    mut images: Query<(&QuickPreviewVisual, &mut ImageNode)>,
) {
    const TRANSITION_SECONDS: f32 = 0.2;
    const SURFACE_ALPHA: f32 = 0.68;
    const BLUR_STRENGTH: f32 = 36.0;

    let amount = (time.delta_secs() / TRANSITION_SECONDS).min(1.0);
    let mut visual_alpha = [0.0; 2];
    let mut changed = false;
    for (panel, mut fade, mut node, surface, background, strength) in &mut panels {
        let next = if fade.current < fade.target {
            (fade.current + amount).min(fade.target)
        } else {
            (fade.current - amount).max(fade.target)
        };
        let panel_changed = fade.current != next;
        if panel_changed {
            fade.current = next;
            changed = true;
        }
        let eased = fade.current * fade.current * (3.0 - 2.0 * fade.current);

        if panel_changed {
            if let Some(mut background) = background {
                background.0 = Color::srgba(0.0, 0.0, 0.0, SURFACE_ALPHA * eased);
            }
            if let Some(mut strength) = strength {
                strength.0 = BLUR_STRENGTH * eased;
            }
        }
        if surface.is_some() {
            visual_alpha[preview_index(panel.owner)] = eased;
        }
        if fade.target == 0.0 && fade.current == 0.0 && node.display != Display::None {
            node.display = Display::None;
        }
    }

    if !changed {
        return;
    }

    for (visual, mut color) in &mut text {
        color.0 = color
            .0
            .with_alpha(visual.base_alpha * visual_alpha[preview_index(visual.owner)]);
    }
    for (visual, mut image) in &mut images {
        image.color = image
            .color
            .with_alpha(visual.base_alpha * visual_alpha[preview_index(visual.owner)]);
    }
}

fn preview_index(owner: ButtonAction) -> usize {
    usize::from(matches!(owner, ButtonAction::QuickLoad))
}

/// Anchors both the UI-layer blur proxy and the Dialog-camera preview to the
/// corresponding control button. All inputs are physical layout values, then
/// converted back into the shared 1920×1080 design canvas.
pub fn position_quick_previews(
    buttons: Query<(&ButtonAction, &ComputedNode, &UiGlobalTransform), With<Button>>,
    content_root: Query<(&ComputedNode, &UiGlobalTransform), With<ContentRoot>>,
    mut previews: Query<(&QuickPreviewPanel, &mut Node)>,
) {
    let Ok((root_node, root_transform)) = content_root.single() else {
        return;
    };
    let root_center = root_transform.translation;
    let to_design = root_node.inverse_scale_factor();
    let root_size = root_node.size() * to_design;

    for (action, button_node, button_transform) in &buttons {
        if !matches!(action, ButtonAction::QuickSave | ButtonAction::QuickLoad) {
            continue;
        }
        let button_center =
            root_size * 0.5 + (button_transform.translation - root_center) * to_design;
        let button_size = button_node.size() * to_design;
        let left = button_center.x + button_size.x * 0.5 - 787.5;
        let top = button_center.y - button_size.y * 0.5 - 202.5 - 6.0;

        for (preview, mut node) in &mut previews {
            if preview.owner == *action {
                node.left = Val::Px(left);
                node.top = Val::Px(top);
                node.right = Val::Auto;
                node.bottom = Val::Auto;
            }
        }
    }
}

pub fn sync_quick_preview(
    preview: Res<QuickSavePreview>,
    game_state: Res<crate::runtime::resources::GameState>,
    asset_server: Res<AssetServer>,
    mut last_program_fingerprint: Local<Option<u64>>,
    mut content: QuickPreviewContentQueries,
) {
    let program_fingerprint = game_state.program_fingerprint;
    if !preview.is_changed() && *last_program_fingerprint == Some(program_fingerprint) {
        return;
    }
    *last_program_fingerprint = Some(program_fingerprint);

    let Some(state) = preview
        .state
        .as_ref()
        .filter(|state| state.program_fingerprint == program_fingerprint)
    else {
        for (_, mut node) in &mut content.images {
            node.display = Display::None;
        }
        for mut node in &mut content.contents {
            node.display = Display::None;
        }
        for mut node in &mut content.empty {
            node.display = Display::Flex;
        }
        return;
    };

    for (mut image, mut node) in &mut content.images {
        if let Some(preview_image) = &preview.image {
            image.image = preview_image.clone();
            node.display = Display::Flex;
        } else if let Some(background) = &state.background {
            image.image = asset_server.load(format!("background/{background}"));
            node.display = Display::Flex;
        } else {
            node.display = Display::None;
        }
    }
    for mut node in &mut content.contents {
        node.display = Display::Flex;
    }
    for mut node in &mut content.empty {
        node.display = Display::None;
    }

    for mut text in &mut content.speakers {
        **text = state.speaker.clone();
    }
    for mut text in &mut content.dialogues {
        **text = state.dialogue.clone();
    }
}

// ── Auto-hide control bars ──

/// Marker component for control bar containers that auto-hide.
#[derive(Component)]
pub(crate) struct AutoHideBar;

/// Marker for text nodes inside auto-hide bars whose TextColor alpha is modulated.
#[derive(Component)]
pub(crate) struct AutoHideText {
    /// Original alpha set at spawn, used as multiplier base.
    pub base_alpha: f32,
}

impl AutoHideText {
    pub fn new(base_alpha: f32) -> Self {
        Self { base_alpha }
    }
}

/// Marks a text node whose alpha is modulated by the Hide toggle (dialogue, speaker name).
#[derive(Component)]
pub(crate) struct HideContentText {
    pub base_alpha: f32,
}

impl HideContentText {
    pub fn new(base_alpha: f32) -> Self {
        Self { base_alpha }
    }
}

/// Marks a background node whose alpha is modulated by the Hide toggle.
#[derive(Component)]
pub(crate) struct HideContentBg {
    pub base_alpha: f32,
}

impl HideContentBg {
    pub fn new(base_alpha: f32) -> Self {
        Self { base_alpha }
    }
}

/// Tracks cursor inactivity and smooth alpha for control bar auto-hide.
#[derive(Resource)]
pub(crate) struct AutoHideTiming {
    /// Time::elapsed_secs() at last cursor move (or startup).
    pub last_move: f32,
    /// Previous cursor position for move detection.
    last_cursor: Option<Vec2>,
    /// Current alpha for control bar auto-hide, lerped toward target each frame.
    pub alpha: f32,
    /// Current alpha for hide-toggle content (dialogue, namebar, bg, blur).
    pub hide_alpha: f32,
    /// Alpha for hide button when hide is ON — follows idle timer regardless of lock.
    pub hide_btn_alpha: f32,
}

impl Default for AutoHideTiming {
    fn default() -> Self {
        Self {
            last_move: 0.0,
            last_cursor: None,
            alpha: 1.0,
            hide_alpha: 1.0,
            hide_btn_alpha: 1.0,
        }
    }
}

impl AutoHideTiming {
    pub(crate) fn lifecycle(&self, now: f32, toggles: &ToggleStates) -> (bool, Duration) {
        let idle = (now - self.last_move).max(0.0);
        let (bar, content, hide_button) = auto_hide_targets(idle, toggles);
        let animating = (self.alpha - bar).abs() > 0.001
            || (self.hide_alpha - content).abs() > 0.001
            || (self.hide_btn_alpha - hide_button).abs() > 0.001;

        let mut wait = Duration::MAX;
        if !toggles.lock && idle < 2.5 {
            wait = wait.min(Duration::from_secs_f32(2.5 - idle));
        }
        if idle < 1.0 {
            wait = wait.min(Duration::from_secs_f32(1.0 - idle));
        }
        (animating, wait)
    }
}

/// Updates last_move from cursor position changes; sets alpha/hide_alpha targets.
pub fn auto_hide_tick(
    time: Res<Time>,
    real_time: Res<Time<Real>>,
    mut timing: ResMut<AutoHideTiming>,
    toggles: Res<ToggleStates>,
    win: Query<&Window>,
) {
    let now = real_time.elapsed_secs();
    if timing.last_move < 0.01 {
        timing.last_move = now;
    }
    if let Ok(w) = win.single()
        && let Some(pos) = w.cursor_position()
    {
        let moved = match timing.last_cursor {
            Some(prev) => (prev - pos).length_squared() > 1.0,
            None => true,
        };
        if timing.last_cursor != Some(pos) {
            timing.last_cursor = Some(pos);
        }
        if moved {
            timing.last_move = now;
        }
    }
    let idle = now - timing.last_move;
    let (bar_target, hide_target, hide_btn_target) = auto_hide_targets(idle, &toggles);
    let amount = exp_lerp(time.delta_secs(), 5.0);
    let alpha = approach_alpha(timing.alpha, bar_target, amount);
    let hide_alpha = approach_alpha(timing.hide_alpha, hide_target, amount);
    let hide_btn_alpha = approach_alpha(timing.hide_btn_alpha, hide_btn_target, amount);
    if timing.alpha != alpha {
        timing.alpha = alpha;
    }
    if timing.hide_alpha != hide_alpha {
        timing.hide_alpha = hide_alpha;
    }
    if timing.hide_btn_alpha != hide_btn_alpha {
        timing.hide_btn_alpha = hide_btn_alpha;
    }
}

fn approach_alpha(current: f32, target: f32, amount: f32) -> f32 {
    if (current - target).abs() <= 0.001 {
        target
    } else {
        current + (target - current) * amount
    }
}

fn auto_hide_targets(idle: f32, toggles: &ToggleStates) -> (f32, f32, f32) {
    (
        if toggles.lock || idle < 2.5 { 1.0 } else { 0.0 },
        if toggles.hide { 0.0 } else { 1.0 },
        if idle < 1.0 { 1.0 } else { 0.0 },
    )
}

#[cfg(test)]
mod auto_hide_tests {
    use super::*;

    #[test]
    fn lifecycle_sleeps_until_the_next_visual_deadline() {
        let timing = AutoHideTiming::default();
        let toggles = ToggleStates::default();

        let (animating, wait) = timing.lifecycle(0.5, &toggles);

        assert!(!animating);
        assert_eq!(wait, Duration::from_secs_f32(0.5));
    }

    #[test]
    fn unlocked_bar_wakes_at_its_deadline_without_polling() {
        let timing = AutoHideTiming {
            last_move: 0.0,
            hide_btn_alpha: 0.0,
            ..default()
        };
        let toggles = ToggleStates {
            lock: false,
            ..default()
        };

        let (animating, wait) = timing.lifecycle(1.5, &toggles);
        assert!(!animating);
        assert_eq!(wait, Duration::from_secs(1));

        let (animating, _) = timing.lifecycle(2.5, &toggles);
        assert!(animating);
    }

    #[test]
    fn settled_auto_hide_alpha_stays_bitwise_stable() {
        assert_eq!(approach_alpha(1.0, 1.0, 0.25), 1.0);
        assert_eq!(approach_alpha(0.0005, 0.0, 0.25), 0.0);
    }

    #[test]
    fn quick_save_metadata_is_enough_to_enable_continue() {
        let preview = QuickSaveSnapshot::from(StoreMetadata {
            saved_at_unix: 1,
            program_fingerprint: 42,
            scene: "main".into(),
            cursor: 7,
            speaker: "A".into(),
            text: "hello".into(),
        });

        assert!(preview.playable);
        assert_eq!(preview.program_fingerprint, 42);
        assert_eq!(preview.speaker, "A");
        assert_eq!(preview.dialogue, "hello");
        assert!(preview.background.is_none());
    }
}

/// Marker for the hide-button text that stays visible during hide mode.
#[derive(Component)]
pub(crate) struct HideButtonText;

/// Marker for the lock-button text whose icon swaps between lock/unlock.
#[derive(Component)]
pub(crate) struct LockIcon;

/// Marker for UI nodes whose behind-the-scene should be blurred.
/// The blur post-process auto-collects all ComputedNode + BlurSource entities.
#[derive(Component)]
pub(crate) struct BlurSource;

#[derive(Component)]
pub(crate) struct BlurStrength(pub f32);

#[derive(Component)]
pub(crate) struct UiBlurSource;

const LOCK_ICON: char = '\u{f47b}';
const UNLOCK_ICON: char = '\u{f600}';

/// Applies the lerped alpha to all AutoHideText nodes.
/// When hide is on: other buttons vanish immediately, hide button follows the 2.5s timer.
pub fn auto_hide_apply(
    timing: Res<AutoHideTiming>,
    toggles: Res<ToggleStates>,
    overlay: Res<crate::ui::textbox::TextboxOverlayFade>,
    initial_fade: Res<crate::ui::textbox::InitialTextboxFade>,
    mut normal_q: Query<
        (&mut TextColor, &AutoHideText, Option<&mut TextShadow>),
        Without<HideButtonText>,
    >,
    mut hide_btn_q: Query<
        (&mut TextColor, &AutoHideText, Option<&mut TextShadow>),
        With<HideButtonText>,
    >,
    mut last: Local<Option<(f32, f32)>>,
) {
    let a = timing.alpha.clamp(0.0, 1.0) * overlay.alpha * initial_fade.alpha;
    // When hide is on, all other buttons vanish instantly; otherwise follow auto-hide timer.
    let normal_a = if toggles.hide { 0.0 } else { a };
    // Hide button: when hide is ON, follows idle timer ignoring lock. Otherwise normal.
    let hide_a = if toggles.hide {
        timing.hide_btn_alpha * initial_fade.alpha
    } else {
        a
    };
    if last.is_some_and(|last| (last.0 - normal_a).abs() < 0.001 && (last.1 - hide_a).abs() < 0.001)
    {
        return;
    }
    *last = Some((normal_a, hide_a));
    for (mut tc, ht, shadow) in &mut normal_q {
        let alpha = ht.base_alpha * normal_a;
        tc.0 = tc.0.with_alpha(alpha);
        if let Some(mut shadow) = shadow {
            shadow.color = shadow.color.with_alpha(0.9 * alpha);
        }
    }
    for (mut tc, ht, shadow) in &mut hide_btn_q {
        let alpha = ht.base_alpha * hide_a;
        tc.0 = tc.0.with_alpha(alpha);
        if let Some(mut shadow) = shadow {
            shadow.color = shadow.color.with_alpha(0.9 * alpha);
        }
    }
}

/// Syncs HoverAlpha active state with ToggleStates (for dialog-driven unhide etc.).
pub fn sync_toggle_highlights(
    toggles: Res<ToggleStates>,
    mut q: Query<(&ButtonAction, &mut HoverAlpha)>,
) {
    if !toggles.is_changed() {
        return;
    }
    for (action, mut ha) in q.iter_mut() {
        let active = match action {
            ButtonAction::Auto => toggles.auto,
            ButtonAction::Skip => toggles.skip,
            _ => continue,
        };
        ha.active = active;
        ha.target = if active { ha.active_alpha } else { 0.0 };
    }
}

/// Swaps the lock icon between locked/unlocked when toggle state changes.
pub fn update_lock_icon(toggles: Res<ToggleStates>, mut q: Query<&mut Text, With<LockIcon>>) {
    if !toggles.is_changed() {
        return;
    }
    let ch = if toggles.lock { LOCK_ICON } else { UNLOCK_ICON };
    let s = ch.to_string();
    for mut text in q.iter_mut() {
        **text = s.clone();
    }
}
