//! Settings motion; registered through the parent facade.
use super::*;

pub fn animate_watermark(
    time: Res<Time>,
    mut watermarks: Query<(&mut SettingsWatermark, &mut Text, &mut TextColor)>,
) {
    let amount = exp_lerp(time.delta_secs(), 34.0);
    for (mut watermark, mut text, mut color) in &mut watermarks {
        let mut changed = false;
        if (watermark.current - watermark.target).abs() >= 0.001 {
            watermark.current += (watermark.target - watermark.current) * amount;
            if (watermark.target - watermark.current).abs() < 0.001 {
                watermark.current = watermark.target;
            }
            changed = true;
        }
        if watermark.current == 0.0
            && watermark.target == 0.0
            && let Some(label) = watermark.pending_label.take()
        {
            text.0 = label;
            watermark.target = 1.0;
            changed = true;
        }
        if !changed {
            continue;
        }
        color.0 = Color::srgba(1.0, 1.0, 1.0, 0.075 * watermark.current);
    }
}

pub fn fade_settings_visuals(
    ui: Res<SettingsUi>,
    route_transition: Res<MenuRouteTransition>,
    mut context: SettingsVisualFadeContext,
    mut cache: Local<SettingsVisualFadeCache>,
) {
    let Ok((root, fade, visibility)) = context.roots.single() else {
        return;
    };
    let belongs_to_settings = |entity: Entity| {
        let mut current = entity;
        while let Ok(parent) = context.parents.get(current) {
            current = parent.parent();
            if current == root {
                return true;
            }
        }
        false
    };

    if *visibility == Visibility::Hidden {
        return;
    }
    let fading =
        (!ui.open && !route_transition.involves(MenuHeaderActive::Config)) || fade.current < 0.999;
    let alpha = smoothstep(fade.current);
    let text_alpha = if fade.target > fade.current {
        smoothstep(fade.current.powf(0.72))
    } else {
        alpha
    };

    if fading {
        if cache.settled {
            cache.text_alpha.clear();
            cache.background_alpha.clear();
            cache.outline_alpha.clear();
        }
        cache.settled = false;
        for (entity, mut color) in &mut context.texts {
            if belongs_to_settings(entity) {
                let base = *cache
                    .text_alpha
                    .entry(entity)
                    .or_insert_with(|| color.0.alpha());
                color.0 = color.0.with_alpha(base * text_alpha);
            }
        }
        for (entity, mut background) in &mut context.backgrounds {
            if belongs_to_settings(entity) {
                let base = *cache
                    .background_alpha
                    .entry(entity)
                    .or_insert_with(|| background.0.alpha());
                background.0 = background.0.with_alpha(base * alpha);
            }
        }
        for (entity, mut outline) in &mut context.outlines {
            if belongs_to_settings(entity) {
                let base = *cache
                    .outline_alpha
                    .entry(entity)
                    .or_insert_with(|| outline.color.alpha());
                outline.color = outline.color.with_alpha(base * alpha);
            }
        }
        return;
    }

    if cache.settled {
        return;
    }

    for (entity, mut color) in &mut context.texts {
        if !belongs_to_settings(entity) {
            continue;
        }
        if let Some(base) = cache.text_alpha.get(&entity) {
            color.0 = color.0.with_alpha(*base);
        }
    }
    for (entity, mut background) in &mut context.backgrounds {
        if !belongs_to_settings(entity) {
            continue;
        }
        if let Some(base) = cache.background_alpha.get(&entity) {
            background.0 = background.0.with_alpha(*base);
        }
    }
    for (entity, mut outline) in &mut context.outlines {
        if !belongs_to_settings(entity) {
            continue;
        }
        if let Some(base) = cache.outline_alpha.get(&entity) {
            outline.color = outline.color.with_alpha(*base);
        }
    }
    cache.text_alpha.clear();
    cache.background_alpha.clear();
    cache.outline_alpha.clear();
    cache.settled = true;
}

pub fn animate_about_repository_link(
    time: Res<Time>,
    mut links: AboutRepositoryAnimationQuery,
    mut labels: Query<&mut TextColor, With<AboutRepositoryLabel>>,
    mut underlines: Query<&mut Node, With<AboutRepositoryUnderline>>,
) {
    let amount = exp_lerp(time.delta_secs(), OPTION_TRANSITION_RATE);
    for (interaction, mut visual, children) in &mut links {
        if !visual.is_animating(*interaction) {
            continue;
        }
        let (target_underline, target_text) = about_repository_targets(*interaction);
        visual.underline_width += (target_underline - visual.underline_width) * amount;
        visual.text_alpha += (target_text - visual.text_alpha) * amount;
        for child in children.iter() {
            if let Ok(mut node) = underlines.get_mut(child) {
                node.width = Val::Percent(visual.underline_width);
            }
            if let Ok(mut color) = labels.get_mut(child) {
                color.0 = Color::srgba(1.0, 1.0, 1.0, visual.text_alpha);
            }
        }
    }
}

pub(super) fn about_repository_targets(interaction: Interaction) -> (f32, f32) {
    match interaction {
        Interaction::None => (0.0, 0.58),
        Interaction::Hovered => (100.0, 0.84),
        Interaction::Pressed => (100.0, 1.0),
    }
}

pub fn advance_language_transition(
    mut commands: Commands,
    mut transition: ResMut<SettingsLocaleTransition>,
    mut settings: ResMut<RuntimeSettings>,
    mut settings_ui: ResMut<SettingsUi>,
    project_root: Res<PersistenceRoot>,
    roots: Query<(Entity, &MenuFade), With<SettingsRoot>>,
) {
    if !transition.is_animating() {
        return;
    }
    if !settings_ui.open {
        transition.finish();
        return;
    }
    let Ok((root, fade)) = roots.single() else {
        return;
    };
    match transition.phase {
        Some(LocaleTransitionPhase::FadeOut) if fade.current <= f32::EPSILON => {
            let Some(locale) = transition.target else {
                transition.finish();
                return;
            };
            settings.locale = locale;
            if let Err(error) = crate::storage::settings::persist(&settings, &project_root) {
                log::error!("failed to persist UI language: {error:#}");
            }
            commands.entity(root).despawn();
            transition.begin_fade_in();
            settings_ui.set_changed();
        }
        Some(LocaleTransitionPhase::FadeIn) if fade.current >= 0.999 => transition.finish(),
        _ => {}
    }
}

pub fn update_setting_visuals(
    resources: SettingsVisualResources,
    sliders: Query<(&Interaction, &SettingSlider)>,
    mut thumbs: Query<
        (
            &SettingSliderThumb,
            &mut SettingSliderThumbVisual,
            &mut Node,
            &mut BackgroundColor,
        ),
        Without<SettingChoice>,
    >,
    mut choices: Query<
        (
            &Interaction,
            &SettingChoice,
            &mut SettingChoiceVisual,
            &Children,
        ),
        Without<SettingSliderThumb>,
    >,
    mut fills: SettingChoiceFillQuery,
    mut choice_text: Query<(&mut TextColor, &mut TextFont)>,
) {
    let amount = exp_lerp(resources.time.delta_secs(), OPTION_TRANSITION_RATE);
    for (kind, mut visual, mut node, mut background) in &mut thumbs {
        let dragging = resources.drag.kind == Some(kind.0);
        let hovered = sliders.iter().any(|(interaction, slider)| {
            slider.0 == kind.0 && matches!(interaction, Interaction::Hovered | Interaction::Pressed)
        });
        let target_width = if hovered || dragging { 12.0 } else { 10.0 };
        if (visual.0 - target_width).abs() < 0.001
            && node.left
                == Val::Percent(
                    kind.0.ratio(&resources.settings).clamp(0.0, 1.0) * (100.0 - visual.0),
                )
        {
            continue;
        }
        if dragging {
            // Pointer motion owns the thumb while captured. Do not interpolate
            // its geometry underneath the cursor.
            visual.0 = target_width;
        } else {
            visual.0 += (target_width - visual.0) * amount;
        }
        node.width = Val::Percent(visual.0);
        node.left =
            Val::Percent(kind.0.ratio(&resources.settings).clamp(0.0, 1.0) * (100.0 - visual.0));
        background.0 = Color::srgba(1.0, 1.0, 1.0, if hovered { 0.67 } else { 0.5 });
    }
    for (interaction, choice, mut visual, children) in &mut choices {
        if !visual.is_animating(*interaction, choice.0, &resources.settings) {
            continue;
        }
        let selected = choice_is_selected(&resources.settings, choice.0);
        let hovered = matches!(interaction, Interaction::Hovered | Interaction::Pressed);
        visual.selected = selected;
        visual.hovered = hovered;
        let target_fill = if selected || hovered { 100.0 } else { 0.0 };
        let target_text = if selected || hovered {
            OPTION_TEXT_ACTIVE
        } else {
            OPTION_TEXT_IDLE
        };
        visual.fill += (target_fill - visual.fill) * amount;
        visual.text_alpha += (target_text - visual.text_alpha) * amount;
        for child in children.iter() {
            if let Ok((mut node, mut background)) = fills.get_mut(child) {
                node.width = Val::Percent(visual.fill);
                background.0 = button_surface(OPTION_FILL_ALPHA);
            }
            if let Ok((mut color, mut font)) = choice_text.get_mut(child) {
                color.0 = Color::srgba(1.0, 1.0, 1.0, visual.text_alpha);
                font.weight = if selected {
                    FontWeight::BOLD
                } else {
                    FontWeight::NORMAL
                };
            }
        }
    }
}

pub fn update_setting_bubbles(
    settings: Res<RuntimeSettings>,
    drag: Res<ActiveSettingSlider>,
    sliders: Query<(&Interaction, &SettingSlider)>,
    mut values: Query<(&SettingValueText, &mut Text)>,
    mut bubbles: Query<(&SettingValueBubble, &mut Node), Without<SettingSliderThumb>>,
) {
    for (kind, mut text) in &mut values {
        let value = kind.0.value_text(kind.0.ratio(&settings));
        if text.0 != value {
            text.0 = value;
        }
    }
    for (kind, mut node) in &mut bubbles {
        // Touch capture owns the slider during a drag; it deliberately does
        // not leave the button Pressed (which would trigger a click).
        let active = drag.kind == Some(kind.0)
            || sliders.iter().any(|(interaction, slider)| {
                slider.0 == kind.0
                    && matches!(interaction, Interaction::Hovered | Interaction::Pressed)
            });
        let display = if active { Display::Flex } else { Display::None };
        let left = Val::Percent(kind.0.ratio(&settings).clamp(0.0, 1.0) * 90.0);
        if node.display != display {
            node.display = display;
        }
        if node.left != left {
            node.left = left;
        }
    }
}

pub fn update_setting_preview(
    settings: Res<RuntimeSettings>,
    mut surfaces: Query<&mut BackgroundColor, With<SettingPreviewSurface>>,
    mut texts: Query<&mut TextFont, With<SettingPreviewText>>,
) {
    if !settings.is_changed() {
        return;
    }
    for mut background in &mut surfaces {
        background.0 = Color::srgba(0.0, 0.0, 0.03, settings.textbox_opacity * 0.72);
    }
    let size = match settings.text_size {
        0 => 23.25,
        2 => 31.5,
        _ => 27.0,
    };
    for mut font in &mut texts {
        font.font_size = FontSize::from(size);
    }
}

pub(super) fn choice_is_selected(settings: &RuntimeSettings, action: SettingAction) -> bool {
    match action {
        SettingAction::SetSkip(value) => settings.skip_all == value,
        SettingAction::SetLanguage(locale) => settings.locale == locale,
        SettingAction::SetFullscreen(value) => settings.fullscreen == value,
        SettingAction::SetTextSize(value) => settings.text_size == value,
        SettingAction::ClearSaves
        | SettingAction::ResetSettings
        | SettingAction::ExportData
        | SettingAction::ImportData => false,
    }
}

pub fn update_settings_pages(
    time: Res<Time>,
    ui: Res<SettingsUi>,
    mut transition: ResMut<SettingsPageTransition>,
    mut buttons: Query<(
        &Interaction,
        &SettingsPageButton,
        &mut SettingsPageButtonVisual,
        &Children,
    )>,
    mut labels: Query<&mut TextColor, With<SettingsPageLabel>>,
    mut panels: Query<(&SettingsPagePanel, &mut Node, &mut UiTransform)>,
) {
    let amount = exp_lerp(time.delta_secs(), OPTION_TRANSITION_RATE);
    for (interaction, page, mut visual, children) in &mut buttons {
        if !visual.is_animating(*interaction, page.0, ui.page) {
            continue;
        }
        let active = ui.page == page.0;
        let hovered = matches!(interaction, Interaction::Hovered | Interaction::Pressed);
        let target = if active {
            PAGE_TEXT_ACTIVE
        } else if hovered {
            PAGE_TEXT_HOVER
        } else {
            PAGE_TEXT_IDLE
        };
        visual.0 += (target - visual.0) * amount;
        for child in children.iter() {
            if let Ok(mut color) = labels.get_mut(child) {
                color.0 = Color::srgba(1.0, 1.0, 1.0, visual.0);
            }
        }
    }

    let (Some(from), Some(to)) = (transition.from, transition.to) else {
        for (panel, mut node, mut transform) in &mut panels {
            let display = if panel.page == ui.page {
                settings_page_display(panel.page)
            } else {
                Display::None
            };
            set_page_presentation(&mut node, &mut transform, display, Val2::ZERO);
        }
        return;
    };

    transition.elapsed =
        (transition.elapsed + time.delta_secs()).min(SettingsPageTransition::SECONDS);
    let progress = transition.elapsed / SettingsPageTransition::SECONDS;
    let direction = (to.index() - from.index()).signum() as f32;

    for (panel, mut node, mut transform) in &mut panels {
        let offset = if panel.page == from {
            Some(page_slide_offset(true, direction, progress))
        } else if panel.page == to {
            Some(page_slide_offset(false, direction, progress))
        } else {
            None
        };
        if let Some(offset) = offset {
            set_page_presentation(
                &mut node,
                &mut transform,
                settings_page_display(panel.page),
                offset,
            );
        } else {
            set_page_presentation(&mut node, &mut transform, Display::None, Val2::ZERO);
        }
    }

    if transition.elapsed >= SettingsPageTransition::SECONDS {
        transition.reset();
        for (panel, mut node, mut transform) in &mut panels {
            let display = if panel.page == ui.page {
                settings_page_display(panel.page)
            } else {
                Display::None
            };
            set_page_presentation(&mut node, &mut transform, display, Val2::ZERO);
        }
    }
}

fn set_page_presentation(
    node: &mut Mut<Node>,
    transform: &mut Mut<UiTransform>,
    display: Display,
    translation: Val2,
) {
    if node.display != display {
        node.display = display;
    }
    if transform.translation != translation {
        transform.translation = translation;
    }
    if transform.scale != Vec2::ONE {
        transform.scale = Vec2::ONE;
    }
}

#[cfg(test)]
mod touch_performance_tests {
    use super::*;

    #[test]
    fn captured_slider_shows_only_its_live_value_until_release() {
        let mut app = App::new();
        app.init_resource::<RuntimeSettings>()
            .init_resource::<ActiveSettingSlider>()
            .add_systems(Update, update_setting_bubbles);
        let mut bubbles = Vec::new();
        for kind in [SettingKind::MasterVolume, SettingKind::BgmVolume] {
            app.world_mut()
                .spawn((SettingSlider(kind), Interaction::None));
            let bubble = app
                .world_mut()
                .spawn((SettingValueBubble(kind), Node::default()))
                .id();
            let text = app
                .world_mut()
                .spawn((SettingValueText(kind), Text::default()))
                .id();
            bubbles.push((bubble, text));
        }
        app.world_mut().resource_mut::<ActiveSettingSlider>().kind =
            Some(SettingKind::MasterVolume);
        app.world_mut()
            .resource_mut::<RuntimeSettings>()
            .master_volume = 0.75;
        app.update();
        assert_eq!(
            app.world().get::<Node>(bubbles[0].0).unwrap().display,
            Display::Flex
        );
        assert_eq!(
            app.world().get::<Node>(bubbles[1].0).unwrap().display,
            Display::None
        );
        assert_eq!(app.world().get::<Text>(bubbles[0].1).unwrap().0, "75");
        app.world_mut()
            .resource_mut::<RuntimeSettings>()
            .master_volume = 0.25;
        app.update();
        assert_eq!(app.world().get::<Text>(bubbles[0].1).unwrap().0, "25");
        app.world_mut().resource_mut::<ActiveSettingSlider>().kind = None;
        app.update();
        assert_eq!(
            app.world().get::<Node>(bubbles[0].0).unwrap().display,
            Display::None
        );
        // Desktop mouse hovering retains its existing hint.
        let mut sliders = app
            .world_mut()
            .query::<(&SettingSlider, &mut Interaction)>();
        for (slider, mut interaction) in sliders.iter_mut(app.world_mut()) {
            if slider.0 == SettingKind::BgmVolume {
                *interaction = Interaction::Hovered;
            }
        }
        app.update();
        assert_eq!(
            app.world().get::<Node>(bubbles[1].0).unwrap().display,
            Display::Flex
        );
    }

    #[derive(Resource, Default)]
    struct LayoutWrites(usize);

    type DirtyPanels = (
        With<SettingsPagePanel>,
        Or<(Changed<Node>, Changed<UiTransform>)>,
    );

    fn count_writes(nodes: Query<Entity, DirtyPanels>, mut count: ResMut<LayoutWrites>) {
        count.0 = nodes.iter().count();
    }

    #[test]
    fn settled_settings_panels_do_not_dirty_layout() {
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<SettingsUi>()
            .init_resource::<SettingsPageTransition>()
            .init_resource::<LayoutWrites>()
            .add_systems(Update, (update_settings_pages, count_writes).chain());
        for page in [
            SettingsPage::System,
            SettingsPage::Display,
            SettingsPage::Audio,
            SettingsPage::About,
        ] {
            app.world_mut().spawn((
                SettingsPagePanel { page },
                Node::default(),
                UiTransform::default(),
            ));
        }
        app.update();
        for _ in 0..60 {
            app.update();
        }
        let writes = app.world().resource::<LayoutWrites>().0;
        eprintln!("settled settings panel layout writes/frame: {writes}");
        assert_eq!(writes, 0);
    }
}
