//! Save_Load sync; registered through the parent facade.
use super::*;

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
        for (mut watermark, label) in &mut context.watermarks {
            watermark.show_label(label, mode.watermark());
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
    for (mut watermark, label) in &mut context.watermarks {
        watermark.show_label(label, mode.watermark());
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
