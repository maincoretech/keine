//! Settings sync; registered through the parent facade.
use super::*;

pub fn sync_settings(
    ui: Res<SettingsUi>,
    settings: Res<RuntimeSettings>,
    save_load: Res<SaveLoadUi>,
    mut context: SettingsSyncContext,
) {
    if !ui.is_changed() {
        return;
    }
    for (entity, mut visibility) in &mut context.roots {
        if !ui.open
            && !context.route_transition.involves(MenuHeaderActive::Config)
            && let Ok(mut fade) = context.fades.get_mut(entity)
        {
            fade.release_to_title(&mut visibility);
        }
    }
    if !ui.open {
        if save_load.mode.is_none() && !context.route_transition.involves(MenuHeaderActive::Config)
        {
            for (mut watermark, _) in &mut context.watermarks {
                watermark.hide();
            }
        }
        for (entity, mut visibility) in &mut context.proxies {
            if let Ok(mut fade) = context.fades.get_mut(entity) {
                if save_load.mode.is_some() {
                    fade.target = 1.0;
                } else {
                    fade.release_to_title(&mut visibility);
                }
            }
        }
        return;
    }
    if !context.roots.is_empty() {
        let switching = context.route_transition.is_animating();
        if !switching {
            for (_, mut visibility) in &mut context.save_roots {
                *visibility = Visibility::Hidden;
            }
        }
        for (mut watermark, label) in &mut context.watermarks {
            watermark.show_label(label, "CONFIG");
        }
        for (entity, mut visibility) in &mut context.roots {
            *visibility = Visibility::Inherited;
            if let Ok(mut fade) = context.fades.get_mut(entity) {
                if switching {
                    fade.current = 1.0;
                }
                fade.target = 1.0;
            }
        }
        for (entity, mut visibility) in &mut context.proxies {
            *visibility = Visibility::Inherited;
            if let Ok(mut fade) = context.fades.get_mut(entity) {
                fade.target = 1.0;
            }
        }
        return;
    }
    let (Ok(camera), Ok(blur_camera)) = (context.camera.single(), context.blur_camera.single())
    else {
        return;
    };
    let switching = context.route_transition.is_animating();
    if !switching {
        for (_, mut visibility) in &mut context.save_roots {
            *visibility = Visibility::Hidden;
        }
    }
    let font = context.fonts.text.clone();
    if context.proxies.is_empty() {
        context
            .commands
            .spawn((
                Name::new("settings_blur"),
                SettingsBlurProxy,
                crate::ui::save_load::SaveLoadBlurProxy,
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
                BackgroundColor(Color::srgba(
                    0.0,
                    0.0,
                    0.0,
                    if switching {
                        crate::ui::MENU_BACKDROP_ALPHA
                    } else {
                        0.0
                    },
                )),
                GlobalZIndex(179),
                UiTargetCamera(blur_camera),
                RenderLayers::layer(1),
            ))
            .with_children(|proxy| spawn_menu_watermark(proxy, "CONFIG", &font));
    } else {
        for (entity, mut visibility) in &mut context.proxies {
            *visibility = Visibility::Inherited;
            if let Ok(mut fade) = context.fades.get_mut(entity) {
                fade.target = 1.0;
            }
        }
        if context.watermarks.is_empty()
            && let Some(proxy) = context.save_proxies.iter().next()
        {
            context
                .commands
                .entity(proxy)
                .with_children(|proxy| spawn_menu_watermark(proxy, "CONFIG", &font));
        }
    }
    let surface = if context.locale_transition.is_fading_in() {
        MenuSurface::fade_only()
    } else {
        MenuSurface::config()
    };
    context
        .commands
        .spawn((
            Name::new("system_settings"),
            SettingsRoot,
            MenuSurfaceState::new(surface, switching),
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
            spawn_options_content(
                root,
                &ui,
                &settings,
                &context.config,
                &context.content,
                context.development.is_some(),
                &font,
            );
        });
}
