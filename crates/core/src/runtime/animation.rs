//! Bevy-free transform rules shared by action startup and frame interpolation.
use crate::{AnimationPreset, SpriteTransform};

/// Entrance presets begin at their zero-progress sample. Other presets keep
/// the base until the presentation backend advances the animation.
pub fn preset_initial_transform(
    base: SpriteTransform,
    preset: &AnimationPreset,
) -> SpriteTransform {
    match preset {
        AnimationPreset::Enter
        | AnimationPreset::EnterFromBottom
        | AnimationPreset::EnterFromLeft
        | AnimationPreset::EnterFromRight => sample_preset(base, preset, 0.0),
        _ => base,
    }
}

/// Sample a built-in transform preset at normalized progress.
pub fn sample_preset(
    base: crate::SpriteTransform,
    preset: &crate::AnimationPreset,
    progress: f32,
) -> crate::SpriteTransform {
    use crate::AnimationPreset;
    let progress = progress.clamp(0.0, 1.0);
    let mut result = base;
    let eased = 1.0 - (1.0 - progress).powi(3);
    match preset {
        AnimationPreset::Enter => result.alpha *= eased,
        AnimationPreset::Exit => result.alpha *= 1.0 - progress * progress,
        AnimationPreset::EnterFromBottom => {
            result.offset_y += 220.0 * (1.0 - eased);
            result.blur += 5.0 * (1.0 - eased);
            result.alpha *= eased;
        }
        AnimationPreset::EnterFromLeft => {
            result.offset_x -= 280.0 * (1.0 - eased);
            result.blur += 5.0 * (1.0 - eased);
            result.alpha *= eased;
        }
        AnimationPreset::EnterFromRight => {
            result.offset_x += 280.0 * (1.0 - eased);
            result.blur += 5.0 * (1.0 - eased);
            result.alpha *= eased;
        }
        AnimationPreset::Shake => {
            let offset = if progress < 0.25 {
                -100.0 * (progress / 0.25)
            } else if progress < 0.75 {
                -100.0 + 200.0 * ((progress - 0.25) / 0.5)
            } else {
                100.0 * (1.0 - (progress - 0.75) / 0.25)
            };
            result.offset_x += offset;
        }
        AnimationPreset::MoveFrontAndBack => {
            let scale = 1.0 + (progress * std::f32::consts::PI).sin() * 0.15;
            result.scale_x *= scale;
            result.scale_y *= scale;
        }
        AnimationPreset::Blur => {
            result.blur += (progress * std::f32::consts::PI).sin() * 4.0;
        }
        AnimationPreset::ShockwaveIn
        | AnimationPreset::ShockwaveOut
        | AnimationPreset::OldFilm
        | AnimationPreset::DotFilm
        | AnimationPreset::ReflectionFilm
        | AnimationPreset::GlitchFilm
        | AnimationPreset::RgbFilm
        | AnimationPreset::GodrayFilm
        | AnimationPreset::RemoveFilm
        | AnimationPreset::Custom(_) => {}
    }
    result
}

/// Restore the base after completion; removal is owned by the animation lifecycle.
pub fn preset_final_transform(
    base: crate::SpriteTransform,
    _preset: &crate::AnimationPreset,
) -> crate::SpriteTransform {
    base
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> SpriteTransform {
        SpriteTransform {
            offset_x: 30.0,
            offset_y: -20.0,
            alpha: 0.4,
            blur: 2.0,
            scale_x: 0.8,
            scale_y: 1.2,
            ..Default::default()
        }
    }

    #[test]
    fn entrances_keep_start_midpoint_and_end_contracts() {
        let base = base();
        for (preset, dx, dy, blur) in [
            (AnimationPreset::Enter, 0.0, 0.0, 0.0),
            (AnimationPreset::EnterFromBottom, 0.0, 220.0, 5.0),
            (AnimationPreset::EnterFromLeft, -280.0, 0.0, 5.0),
            (AnimationPreset::EnterFromRight, 280.0, 0.0, 5.0),
        ] {
            let initial = SpriteTransform {
                offset_x: base.offset_x + dx,
                offset_y: base.offset_y + dy,
                blur: base.blur + blur,
                alpha: 0.0,
                ..base
            };
            assert_eq!(preset_initial_transform(base, &preset), initial);
            assert_eq!(sample_preset(base, &preset, -1.0), initial);
            assert_eq!(
                sample_preset(base, &preset, 0.5),
                SpriteTransform {
                    offset_x: base.offset_x + dx * 0.125,
                    offset_y: base.offset_y + dy * 0.125,
                    blur: base.blur + blur * 0.125,
                    alpha: base.alpha * 0.875,
                    ..base
                }
            );
            assert_eq!(sample_preset(base, &preset, 2.0), base);
            assert_eq!(preset_final_transform(base, &preset), base);
        }
    }

    #[test]
    fn transient_presets_preserve_sampling_and_completion() {
        let base = base();
        assert_eq!(
            sample_preset(base, &AnimationPreset::Shake, 0.25).offset_x,
            -70.0
        );
        assert_eq!(
            sample_preset(base, &AnimationPreset::Shake, 0.5).offset_x,
            30.0
        );
        assert_eq!(
            sample_preset(base, &AnimationPreset::Shake, 0.75).offset_x,
            130.0
        );
        assert_eq!(sample_preset(base, &AnimationPreset::Blur, 0.5).blur, 6.0);
        let zoom = sample_preset(base, &AnimationPreset::MoveFrontAndBack, 0.5);
        assert_eq!(zoom.scale_x, base.scale_x * 1.15);
        assert_eq!(zoom.scale_y, base.scale_y * 1.15);
        assert_eq!(
            sample_preset(base, &AnimationPreset::Exit, 0.5).alpha,
            base.alpha * 0.75
        );
        assert_eq!(sample_preset(base, &AnimationPreset::Exit, 1.0).alpha, 0.0);
        for preset in [
            AnimationPreset::Shake,
            AnimationPreset::Blur,
            AnimationPreset::Exit,
            AnimationPreset::MoveFrontAndBack,
            AnimationPreset::Custom("custom".into()),
            AnimationPreset::OldFilm,
            AnimationPreset::ShockwaveIn,
        ] {
            assert_eq!(preset_initial_transform(base, &preset), base);
            assert_eq!(preset_final_transform(base, &preset), base);
        }
    }
}
