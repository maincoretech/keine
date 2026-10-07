//! Confirmation dialog motion.
use super::*;

pub fn animate_dialog(
    time: Res<Time>,
    mut fade_query: Query<&mut DialogFade>,
    mut backgrounds: Query<(&DialogBackground, &mut BackgroundColor)>,
    mut borders: Query<(&DialogBorder, &mut BorderColor)>,
    mut texts: Query<(&DialogText, &mut TextColor)>,
) {
    let Ok(mut fade) = fade_query.single_mut() else {
        return;
    };
    if fade.0 >= 1.0 {
        return;
    }
    fade.0 = (fade.0 + time.delta_secs() / FADE_DURATION).min(1.0);

    for (visual, mut color) in &mut backgrounds {
        color.0 = Color::srgba(0.0, 0.0, 0.0, visual.alpha * fade.0);
    }
    for (visual, mut color) in &mut borders {
        *color = BorderColor::all(Color::srgba(0.0, 0.0, 0.0, visual.alpha * fade.0));
    }
    for (visual, mut color) in &mut texts {
        color.0 = Color::srgba(1.0, 1.0, 1.0, visual.alpha * fade.0);
    }
}
