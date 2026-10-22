//! Gallery button feedback, fades and horizontal page transitions.
use super::*;

#[derive(Default)]
pub(crate) struct ExtraFadeCache {
    root: Option<Entity>,
    text: HashMap<Entity, f32>,
    background: HashMap<Entity, f32>,
    image: HashMap<Entity, f32>,
}

impl ExtraFadeCache {
    fn clear(&mut self) {
        self.root = None;
        self.text.clear();
        self.background.clear();
        self.image.clear();
    }
}

type ExtraBackgroundQuery<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static mut BackgroundColor),
    (
        Without<ExtraBlurProxy>,
        Without<ExtraRoot>,
        Without<ExtraFullCg>,
    ),
>;

#[derive(SystemParam)]
pub(crate) struct ExtraFadeContext<'w, 's> {
    parents: Query<'w, 's, &'static ChildOf>,
    texts: Query<'w, 's, (Entity, &'static mut TextColor)>,
    choices: Query<'w, 's, &'static SettingChoiceVisual>,
    backgrounds: ExtraBackgroundQuery<'w, 's>,
    images: Query<'w, 's, (Entity, &'static mut ImageNode)>,
}

#[derive(SystemParam)]
pub(crate) struct ExtraAnimationContext<'w, 's> {
    roots: Query<
        'w,
        's,
        (Entity, &'static mut ExtraMotion, &'static mut UiTransform),
        With<ExtraRoot>,
    >,
    proxies: Query<
        'w,
        's,
        (
            Entity,
            &'static mut BlurStrength,
            &'static mut BackgroundColor,
        ),
        With<ExtraBlurProxy>,
    >,
    watermarks: Query<'w, 's, &'static mut SettingsWatermark, With<ExtraWatermark>>,
    players: Query<'w, 's, Entity, With<ExtraBgmPlayer>>,
    stage_bgm: StageBgmQuery<'w, 's>,
    fade: ExtraFadeContext<'w, 's>,
}

type ExtraButtonQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static Interaction,
        Option<&'static ExtraBgm>,
        Option<&'static ExtraTab>,
        &'static mut SettingChoiceVisual,
        &'static Children,
    ),
    With<ExtraButtonVisual>,
>;

pub(crate) fn update_button_visuals(
    time: Res<Time>,
    ui: Res<ExtraUi>,
    mut buttons: ExtraButtonQuery,
    mut fills: SettingChoiceFillQuery,
    mut labels: Query<(&mut TextColor, &mut TextFont)>,
) {
    let amount = exp_lerp(time.delta_secs(), 18.0);
    for (interaction, track, tab, mut visual, children) in &mut buttons {
        let selected = track.is_some_and(|track| ui.selected_bgm.as_deref() == Some(&track.0))
            || tab.is_some_and(|tab| ui.section == tab.section);
        animate_choice_visual(
            *interaction,
            selected,
            &mut visual,
            children,
            &mut fills,
            &mut labels,
            amount,
        );
    }
}

pub(crate) fn animate(
    time: Res<Time>,
    mut context: ExtraAnimationContext,
    mut fade_cache: Local<ExtraFadeCache>,
    mut commands: Commands,
    mut ui: ResMut<ExtraUi>,
) {
    let amount = exp_lerp(time.delta_secs(), UI_MOTION_RATE);
    let mut progress = None;
    for (entity, mut motion, mut transform) in &mut context.roots {
        let mut current = motion.current + (motion.target - motion.current) * amount;
        if motion.target == 0.0 && current <= 0.05 || (motion.target - current).abs() < 0.001 {
            current = motion.target;
        }
        if motion.current != current {
            motion.current = current;
        }
        let eased = smoothstep(current);
        let translation = Val2::px(31.5 * (1.0 - eased), 0.0);
        if transform.translation != translation {
            transform.translation = translation;
        }
        if transform.scale != Vec2::ONE {
            transform.scale = Vec2::ONE;
        }
        progress = Some((entity, eased));
        if motion.current == 0.0 && motion.target == 0.0 {
            commands.entity(entity).despawn();
            for entity in &context.players {
                commands.entity(entity).despawn();
            }
            resume_stage_bgm(&mut ui, &context.stage_bgm);
        }
    }
    if let Some((root, progress)) = progress {
        fade_extra_content(root, progress, &mut context.fade, &mut fade_cache);
        for mut watermark in &mut context.watermarks {
            watermark.set_progress(progress);
        }
        for (entity, mut strength, mut background) in &mut context.proxies {
            let blur = crate::ui::FULLSCREEN_BLUR_STRENGTH * progress;
            if strength.0 != blur {
                strength.0 = blur;
            }
            background.set_if_neq(BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6 * progress)));
            // Zero progress also occurs on the first opening frame. Only
            // tear down the backdrop when the screen has finished closing.
            if !ui.open && progress == 0.0 {
                commands.entity(entity).despawn();
            }
        }
    } else {
        fade_cache.clear();
    }
}

fn fade_extra_content(
    root: Entity,
    alpha: f32,
    context: &mut ExtraFadeContext,
    cache: &mut ExtraFadeCache,
) {
    if cache.root != Some(root) {
        cache.clear();
        cache.root = Some(root);
    }
    if alpha >= 0.999 {
        for (entity, base) in cache.text.drain() {
            if let Ok((_, mut color)) = context.texts.get_mut(entity) {
                let base = context
                    .parents
                    .get(entity)
                    .ok()
                    .and_then(|parent| context.choices.get(parent.parent()).ok())
                    .map_or(base, |visual| visual.text_alpha);
                color.0 = color.0.with_alpha(base);
            }
        }
        for (entity, base) in cache.background.drain() {
            if let Ok((_, mut color)) = context.backgrounds.get_mut(entity) {
                color.0 = color.0.with_alpha(base);
            }
        }
        for (entity, base) in cache.image.drain() {
            if let Ok((_, mut image)) = context.images.get_mut(entity) {
                image.color = image.color.with_alpha(base);
            }
        }
        cache.root = None;
        return;
    }
    let belongs_to_root = |entity: Entity| {
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
        if belongs_to_root(entity) {
            let choice_alpha = context
                .parents
                .get(entity)
                .ok()
                .and_then(|parent| context.choices.get(parent.parent()).ok())
                .map(|visual| visual.text_alpha);
            let base = cache.text.entry(entity).or_insert_with(|| color.0.alpha());
            if let Some(choice_alpha) = choice_alpha {
                *base = choice_alpha;
            }
            color.0 = color.0.with_alpha(*base * alpha);
        }
    }
    for (entity, mut color) in &mut context.backgrounds {
        if belongs_to_root(entity) {
            let base = *cache
                .background
                .entry(entity)
                .or_insert_with(|| color.0.alpha());
            color.0 = color.0.with_alpha(base * alpha);
        }
    }
    for (entity, mut image) in &mut context.images {
        if belongs_to_root(entity) {
            let base = *cache
                .image
                .entry(entity)
                .or_insert_with(|| image.color.alpha());
            image.color = image.color.with_alpha(base * alpha);
        }
    }
}

pub(crate) fn update_sections(
    time: Res<Time>,
    ui: Res<ExtraUi>,
    mut transition: ResMut<ExtraPageTransition>,
    mut panels: Query<(&ExtraPanel, &mut Node, &mut UiTransform), Without<ExtraCgGrid>>,
    mut grids: Query<(Entity, &ExtraCgGrid, &mut UiTransform), Without<ExtraPanel>>,
    mut commands: Commands,
) {
    if !ui.open && (transition.is_animating() || transition.elapsed != 0.0) {
        *transition = ExtraPageTransition::default();
    }
    if transition.is_animating() {
        transition.elapsed = (transition.elapsed + time.delta_secs()).min(PAGE_SLIDE_SECONDS);
    }
    let progress = transition.elapsed / PAGE_SLIDE_SECONDS;
    if progress >= 1.0 && transition.is_animating() {
        transition.from = None;
        transition.cg_direction = 0.0;
    }
    for (panel, mut node, mut transform) in &mut panels {
        let outgoing = transition.from == Some(panel.0);
        let incoming = ui.section == panel.0;
        let display = if incoming || outgoing {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        let translation = if transition.from.is_some() {
            horizontal_page_offset(
                outgoing,
                if ui.section == ExtraSection::Music {
                    1.0
                } else {
                    -1.0
                },
                progress,
            )
        } else {
            Val2::ZERO
        };
        if transform.translation != translation {
            transform.translation = translation;
        }
    }
    for (entity, grid, mut transform) in &mut grids {
        let translation = if transition.cg_direction != 0.0 && progress < 1.0 {
            horizontal_page_offset(grid.outgoing, transition.cg_direction, progress)
        } else if grid.outgoing {
            commands.entity(entity).despawn();
            continue;
        } else {
            Val2::ZERO
        };
        if transform.translation != translation {
            transform.translation = translation;
        }
    }
}

fn horizontal_page_offset(outgoing: bool, direction: f32, progress: f32) -> Val2 {
    let eased = ease_in_out_cubic(progress);
    Val2::percent(
        direction * 100.0 * if outgoing { -eased } else { 1.0 - eased },
        0.0,
    )
}

pub(crate) fn animate_full_cg(
    time: Res<Time>,
    mut full: Query<
        (
            Entity,
            &mut ExtraMotion,
            &mut UiTransform,
            &mut BackgroundColor,
        ),
        With<ExtraFullCg>,
    >,
    mut fade: ExtraFadeContext,
    mut cache: Local<ExtraFadeCache>,
    mut commands: Commands,
) {
    for (entity, mut motion, mut transform, mut background) in &mut full {
        let mut current = motion.current
            + (motion.target - motion.current) * exp_lerp(time.delta_secs(), UI_MOTION_RATE);
        if (current - motion.target).abs() < 0.001 || motion.target == 0.0 && current < 0.05 {
            current = motion.target;
        }
        if motion.current != current {
            motion.current = current;
        }
        let progress = smoothstep(current);
        let scale = Vec2::splat(0.99 + progress * 0.01);
        if transform.scale != scale {
            transform.scale = scale;
        }
        background.set_if_neq(BackgroundColor(Color::srgba(
            0.0,
            0.0,
            0.0,
            0.72 * progress,
        )));
        fade_extra_content(entity, progress, &mut fade, &mut cache);
        if progress == 0.0 && motion.target == 0.0 {
            commands.entity(entity).despawn();
        }
    }
}
