//! Gallery lifecycle and image readiness.
use super::*;

pub(crate) fn active(ui: Res<ExtraUi>, roots: Query<(), With<ExtraRoot>>) -> bool {
    ui.open || !roots.is_empty()
}

#[derive(SystemParam)]
pub(crate) struct ExtraSyncContext<'w, 's> {
    commands: Commands<'w, 's>,
    ui: ResMut<'w, ExtraUi>,
    state: Res<'w, GameState>,
    config: Res<'w, GameConfigResource>,
    fonts: Res<'w, UiFonts>,
    settings: Res<'w, RuntimeSettings>,
    assets: Res<'w, AssetServer>,
    image_roles: Res<'w, ImageRoleRegistry>,
    ui_camera: Query<'w, 's, Entity, With<UiBlurCamera>>,
    dialog_camera: Query<'w, 's, Entity, With<DialogCamera>>,
    roots: Query<'w, 's, &'static mut ExtraMotion, With<ExtraRoot>>,
    proxies: Query<'w, 's, Entity, With<ExtraBlurProxy>>,
}

pub(crate) fn sync(mut context: ExtraSyncContext) {
    if !context.config.features.extra {
        context.ui.open = false;
    }
    if !context.ui.open {
        for mut motion in &mut context.roots {
            motion.target = 0.0;
        }
        return;
    }
    let (Ok(ui_camera), Ok(dialog_camera)) =
        (context.ui_camera.single(), context.dialog_camera.single())
    else {
        return;
    };
    if context.proxies.is_empty() {
        context
            .commands
            .spawn((
                ExtraBlurProxy,
                UiBlurSource,
                BlurStrength(0.0),
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Px(DESIGN_WIDTH),
                    height: Val::Px(DESIGN_HEIGHT),
                    ..default()
                },
                BackgroundColor(Color::NONE),
                FocusPolicy::Pass,
                GlobalZIndex(171),
                UiTargetCamera(ui_camera),
                RenderLayers::layer(1),
            ))
            .with_children(|proxy| {
                proxy.spawn((ExtraWatermark, menu_watermark("EXTRA", &context.fonts.text)));
            });
    }

    if !context.roots.is_empty() {
        return;
    }

    let mut cg = context
        .state
        .unlocked_cg
        .iter()
        .map(|(file, name)| (file.clone(), name.clone()))
        .collect::<Vec<_>>();
    cg.sort_unstable_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
    context.ui.page = context
        .ui
        .page
        .clamp(1, cg.len().div_ceil(CG_PER_PAGE).max(1));
    let mut bgm = context
        .state
        .unlocked_bgm
        .iter()
        .map(|(file, name)| (file.clone(), name.clone()))
        .collect::<Vec<_>>();
    bgm.sort_unstable_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));

    context
        .commands
        .spawn((
            ExtraRoot,
            ExtraMotion {
                current: 0.0,
                target: 1.0,
            },
            UiTransform::from_translation(Val2::px(31.5, 0.0)),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(DESIGN_WIDTH),
                height: Val::Px(DESIGN_HEIGHT),
                padding: UiRect::axes(Val::Percent(2.5), Val::Percent(2.0)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            FocusPolicy::Block,
            GlobalZIndex(172),
            UiTargetCamera(dialog_camera),
            RenderLayers::layer(2),
        ))
        .with_children(|root| {
            spawn_header(
                root,
                &context.fonts,
                context.ui.section,
                context.settings.locale,
            );
            root.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Percent(3.0),
                    top: Val::Percent(12.0),
                    width: Val::Percent(94.0),
                    height: Val::Percent(84.0),
                    padding: UiRect::axes(Val::Px(27.0), Val::Px(13.5)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(13.5),
                    ..default()
                },
                BackgroundColor(dark_surface(SURFACE_PANEL_ALPHA)),
            ))
            .with_children(|body| {
                body.spawn(Node {
                    position_type: PositionType::Relative,
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    min_width: Val::ZERO,
                    min_height: Val::ZERO,
                    flex_basis: Val::ZERO,
                    overflow: Overflow::clip(),
                    ..default()
                })
                .with_children(|content| {
                    spawn_cg_panel(
                        content,
                        &cg,
                        &context.ui,
                        &context.config,
                        &context.fonts,
                        &context.assets,
                        &context.image_roles,
                    );
                    spawn_bgm_panel(
                        content,
                        &bgm,
                        &context.ui,
                        &context.fonts,
                        &context.settings,
                    );
                });
            });
        });
}

pub(crate) fn content_ready(
    ui: Res<ExtraUi>,
    transition: Res<ExtraPageTransition>,
    full: Query<(), With<ExtraFullCg>>,
    roots: Query<&ExtraMotion, With<ExtraRoot>>,
) -> bool {
    ui.open
        && !transition.is_animating()
        && full.is_empty()
        && roots.iter().all(|motion| !motion.is_animating())
}

pub(crate) fn update_image_status(
    server: Res<AssetServer>,
    thumbnails: Res<Assets<crate::scene::images::GalleryThumbnail>>,
    mut cards: Query<(&ExtraThumbnail, &mut ImageNode)>,
    mut labels: Query<(&ExtraImageStatus, &mut LocalizedText, &mut Node)>,
) {
    for (thumbnail, mut image) in &mut cards {
        if let Some(loaded) = thumbnails.get(&thumbnail.0)
            && image.image != loaded.image
        {
            image.image = loaded.image.clone();
        }
    }
    for (image, mut label, mut node) in &mut labels {
        let state = server.load_state(&image.0);
        node.display = if matches!(state, LoadState::Loaded) {
            Display::None
        } else {
            Display::Flex
        };
        label.0 = if matches!(state, LoadState::Failed(_)) {
            UiText::GalleryUnavailable
        } else {
            UiText::GalleryLoading
        };
    }
}
