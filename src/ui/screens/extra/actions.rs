//! Gallery navigation, tabs and CG selection.
use super::*;

pub(crate) fn handle_navigation(
    keys: Res<ButtonInput<KeyCode>>,
    actions: Res<crate::runtime::platform::InputActions>,
    close: Query<&Interaction, (With<ExtraClose>, Changed<Interaction>)>,
    mut full: Query<&mut ExtraMotion, With<ExtraFullCg>>,
    mut ui: ResMut<ExtraUi>,
) {
    if !ui.open {
        return;
    }
    if keys.just_pressed(KeyCode::Escape) || actions.back {
        if !full.is_empty() {
            for mut motion in &mut full {
                motion.target = 0.0;
            }
        } else {
            ui.open = false;
        }
    }
    if close
        .iter()
        .any(|interaction| *interaction == Interaction::Pressed)
    {
        ui.open = false;
    }
}

#[derive(SystemParam)]
pub(crate) struct ExtraPageContext<'w, 's> {
    pages: Query<'w, 's, (&'static Interaction, &'static ExtraPage), Changed<Interaction>>,
    ui: ResMut<'w, ExtraUi>,
    state: Res<'w, GameState>,
    config: Res<'w, GameConfigResource>,
    fonts: Res<'w, UiFonts>,
    assets: Res<'w, AssetServer>,
    image_roles: Res<'w, ImageRoleRegistry>,
    grids: Query<'w, 's, (Entity, &'static ChildOf, &'static ExtraCgGrid)>,
    transition: ResMut<'w, ExtraPageTransition>,
    page_labels: Query<'w, 's, &'static mut Text, With<ExtraPageLabel>>,
    commands: Commands<'w, 's>,
}

pub(crate) fn handle_page(mut context: ExtraPageContext) {
    let Some(page) = context
        .pages
        .iter()
        .find_map(|(interaction, page)| (*interaction == Interaction::Pressed).then_some(page.0))
    else {
        return;
    };
    if !context.ui.open || context.transition.is_animating() {
        return;
    }
    let mut images = ordered_cg(&context.state);
    let count = images.len().div_ceil(CG_PER_PAGE).max(1);
    let page = (context.ui.page as i64 + i64::from(page)).clamp(1, count as i64) as usize;
    if context.ui.page != page {
        context.transition.cg_direction = if page > context.ui.page { 1.0 } else { -1.0 };
        context.transition.elapsed = 0.0;
        context.ui.page = page;
        for mut label in &mut context.page_labels {
            label.0 = format!("{page} / {count}");
        }
        images.sort_unstable_by(|left, right| {
            left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0))
        });
        for (grid, parent, phase) in &context.grids {
            if phase.outgoing {
                continue;
            }
            context
                .commands
                .entity(grid)
                .insert(ExtraCgGrid { outgoing: true });
            context
                .commands
                .entity(parent.parent())
                .with_children(|viewport| {
                    spawn_cg_grid(
                        viewport,
                        &images,
                        page,
                        &context.config,
                        &context.fonts,
                        &context.assets,
                        &context.image_roles,
                    );
                });
        }
    }
}

#[derive(SystemParam)]
pub(crate) struct ExtraCgContext<'w, 's> {
    cards: Query<'w, 's, (&'static Interaction, &'static ExtraCg), Changed<Interaction>>,
    controls: Query<'w, 's, (&'static Interaction, &'static ExtraCgControl), Changed<Interaction>>,
    full: Query<'w, 's, (Entity, &'static ExtraFullCg)>,
    roots: Query<'w, 's, Entity, With<ExtraRoot>>,
    state: Res<'w, GameState>,
    config: Res<'w, GameConfigResource>,
    fonts: Res<'w, UiFonts>,
    assets: Res<'w, AssetServer>,
    image_roles: Res<'w, ImageRoleRegistry>,
    ui: Res<'w, ExtraUi>,
    transition: Res<'w, ExtraPageTransition>,
    motions: Query<'w, 's, &'static ExtraMotion>,
    camera: Query<'w, 's, Entity, With<DialogCamera>>,
    commands: Commands<'w, 's>,
}

pub(crate) fn handle_cg(mut context: ExtraCgContext) {
    if !context.ui.open
        || context.transition.is_animating()
        || context.motions.iter().any(ExtraMotion::is_animating)
    {
        return;
    }
    let card = context.cards.iter().find_map(|(interaction, card)| {
        (*interaction == Interaction::Pressed).then_some(card.0.clone())
    });
    let control = context.controls.iter().find_map(|(interaction, control)| {
        (*interaction == Interaction::Pressed).then_some(*control)
    });
    if card.is_none() && control.is_none() {
        return;
    }
    let Ok(camera) = context.camera.single() else {
        return;
    };
    let images = ordered_cg(&context.state);

    let current = context.full.single().ok();
    let selected = if let Some(file) = card {
        Some(file)
    } else if matches!(control, Some(ExtraCgControl::Close)) {
        for (entity, _) in &context.full {
            context.commands.entity(entity).insert(ExtraMotion {
                current: 1.0,
                target: 0.0,
            });
        }
        return;
    } else {
        let Some((_, current)) = current else { return };
        let Some(index) = images.iter().position(|item| item.0 == current.0) else {
            return;
        };
        let next = if matches!(control, Some(ExtraCgControl::Previous)) {
            (index + images.len() - 1) % images.len()
        } else {
            (index + 1) % images.len()
        };
        Some(images[next].0.clone())
    };
    for (entity, _) in &context.full {
        context.commands.entity(entity).despawn();
    }
    let Some(file) = selected else { return };
    let image = crate::scene::images::load(
        &context.assets,
        &context.image_roles,
        context.config.bg_path(&file),
        ImageRole::BACKGROUND,
    );
    let overlay = spawn_full_cg(
        &mut context.commands,
        camera,
        &file,
        &images,
        &context.fonts,
        image,
    );
    if let Ok(root) = context.roots.single() {
        context.commands.entity(root).add_child(overlay);
    }
}

pub(crate) fn handle_section(
    buttons: Query<(&Interaction, &ExtraTab), Changed<Interaction>>,
    mut ui: ResMut<ExtraUi>,
    mut transition: ResMut<ExtraPageTransition>,
    mut seek: Query<&mut ExtraBgmSeekBar>,
) {
    for (interaction, tab) in &buttons {
        if *interaction == Interaction::Pressed && ui.section != tab.section {
            transition.from = Some(ui.section);
            transition.elapsed = 0.0;
            ui.section = tab.section;
            for mut seek in &mut seek {
                seek.reset();
            }
        }
    }
}
