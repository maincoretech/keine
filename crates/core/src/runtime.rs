//! Deterministic state-machine execution and transition math.

pub mod eiyashou;
pub mod expression;
pub mod step;

pub use step::{ScriptRuntimeError, StepResult};

/// Transition easing functions shared by the state machine and renderer.
pub mod dissolve {
    /// Ren'Py-style dissolve curve with slow endpoints.
    pub fn renpy_dissolve(t: f32) -> f32 {
        if t < 0.1 {
            let p = t / 0.1;
            0.05 * ease_out_expo(p)
        } else if t < 0.9 {
            let p = (t - 0.1) / 0.8;
            0.05 + 0.9 * ease_in_out_cubic(p)
        } else {
            let p = (t - 0.9) / 0.1;
            0.95 + 0.05 * ease_in_expo(p)
        }
    }

    pub fn smooth_fade(t: f32) -> f32 {
        ease_in_out_cubic(t)
    }

    fn ease_out_expo(t: f32) -> f32 {
        if t >= 1.0 {
            1.0
        } else {
            1.0 - 2.0_f32.powf(-10.0 * t)
        }
    }

    fn ease_in_expo(t: f32) -> f32 {
        if t <= 0.0 {
            0.0
        } else {
            2.0_f32.powf(10.0 * (t - 1.0))
        }
    }

    fn ease_in_out_cubic(t: f32) -> f32 {
        if t < 0.5 {
            4.0 * t * t * t
        } else {
            1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn endpoints_are_stable() {
            assert!((renpy_dissolve(0.0) - 0.0).abs() < 0.01);
            assert!((renpy_dissolve(1.0) - 1.0).abs() < 0.01);
            assert!((smooth_fade(0.0) - 0.0).abs() < 0.01);
            assert!((smooth_fade(1.0) - 1.0).abs() < 0.01);
        }

        #[test]
        fn dissolve_is_monotonic() {
            let mut previous = 0.0;
            for index in 0..=100 {
                let progress = index as f32 / 100.0;
                let value = renpy_dissolve(progress);
                assert!(value >= previous);
                previous = value;
            }
        }
    }
}
