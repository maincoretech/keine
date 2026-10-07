//! Save_Load motion; registered through the parent facade.
use super::*;

pub(super) fn page_highlight_alpha(selected: bool, hovered: bool) -> f32 {
    if selected || hovered {
        SURFACE_ACTIVE_ALPHA
    } else {
        0.0
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
    let Ok((root, root_fade, visibility)) = context.roots.single() else {
        return;
    };
    if *visibility == Visibility::Hidden {
        return;
    }
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

    // Reopening/page rebuilds can replace the grid while alpha is unchanged.
    // New children start opaque and must receive the fade before extraction.
    if cache.active && (cache.last_alpha - menu_alpha).abs() < 0.0001 {
        let new_visuals = context
            .texts
            .iter_mut()
            .any(|(entity, color)| color.is_added() && belongs_to(entity, root))
            || context
                .backgrounds
                .iter_mut()
                .any(|(entity, color)| color.is_added() && belongs_to(entity, root))
            || context
                .borders
                .iter_mut()
                .any(|(entity, color)| color.is_added() && belongs_to(entity, root))
            || context
                .images
                .iter_mut()
                .any(|(entity, image)| image.is_added() && belongs_to(entity, root));
        if !new_visuals {
            return;
        }
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

pub(super) fn restore_save_load_content(
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
        if (motion.scale - target_scale).abs() < 0.001 {
            continue;
        }
        motion.scale += (target_scale - motion.scale) * amount;
        transform.scale = Vec2::splat(motion.scale);
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
