//! Frame attribution extracted with the same world snapshot as the renderer.
use std::sync::Arc;
use std::time::Instant;

use bevy::diagnostic::FrameCount;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::render::extract_resource::ExtractResource;
use bevy::window::{Monitor, OnMonitor, PrimaryWindow};

use crate::runtime::platform::RuntimeActivity;
use crate::runtime::resources::{GameState, LocalAssetManifest, ProjectRoot};

pub(crate) const TRACE_FIELDS: &str = "elapsed_seconds\tframe\tinterval_ms\tupdate_to_render_ms\tbudget_ms\tscene\tnext_cursor\tsource\tactivity\tline\tfocused\twidth\theight\texclusion";

#[derive(Resource, Clone, ExtractResource)]
pub(super) struct FrameContext {
    pub frame: u32,
    pub scene: Arc<str>,
    pub cursor: usize,
    pub source: Arc<str>,
    pub line: usize,
    pub activity: RuntimeActivity,
    pub focused: bool,
    pub size: (u32, u32),
    pub budget_ms: Option<f64>,
    pub update_started: Instant,
}

impl Default for FrameContext {
    fn default() -> Self {
        Self {
            frame: 0,
            scene: "".into(),
            cursor: 0,
            source: "".into(),
            line: 0,
            activity: RuntimeActivity::Active,
            focused: false,
            size: (0, 0),
            budget_ms: None,
            update_started: Instant::now(),
        }
    }
}

pub(super) fn begin_update(mut context: ResMut<FrameContext>) {
    context.update_started = Instant::now();
}

#[derive(SystemParam)]
pub(super) struct AttributionInputs<'w, 's> {
    state: Res<'w, GameState>,
    manifest: Res<'w, LocalAssetManifest>,
    root: Res<'w, ProjectRoot>,
    activity: Res<'w, RuntimeActivity>,
    frame: Res<'w, FrameCount>,
    windows: Query<'w, 's, (&'static Window, Option<&'static OnMonitor>), With<PrimaryWindow>>,
    monitors: Query<'w, 's, &'static Monitor>,
    config: Res<'w, super::RuntimeCaptureConfig>,
}

pub(super) fn attribute_frame(inputs: AttributionInputs, mut context: ResMut<FrameContext>) {
    let AttributionInputs {
        state,
        manifest,
        root,
        activity,
        frame,
        windows,
        monitors,
        config,
    } = inputs;
    context.frame = frame.0;
    if context.scene.as_ref() != state.current_scene {
        context.scene = state.current_scene.as_str().into();
        context.source = manifest.get(&state.current_scene).map_or_else(
            || "".into(),
            |assets| {
                assets
                    .source_path
                    .strip_prefix(&root.0)
                    .unwrap_or(&assets.source_path)
                    .to_string_lossy()
                    .as_ref()
                    .into()
            },
        );
    }
    // Core's cursor points after the executing/yielding action.
    context.cursor = state.cursor;
    context.line = manifest
        .get(&state.current_scene)
        .and_then(|s| s.action_spans.get(state.cursor.saturating_sub(1)))
        .map_or(0, |span| span.line);
    context.activity = *activity;
    if let Ok((window, monitor)) = windows.single() {
        context.focused = window.focused;
        context.size = (
            window.resolution.physical_width(),
            window.resolution.physical_height(),
        );
        let hz = config.refresh_hz.or_else(|| {
            monitor
                .and_then(|m| monitors.get(m.0).ok())
                .and_then(|m| m.refresh_rate_millihertz)
                .filter(|hz| *hz > 0)
                .map(|hz| f64::from(hz) / 1000.0)
        });
        context.budget_ms = hz.map(|hz| 1000.0 / hz);
    }
}

#[derive(Clone)]
pub(super) struct CapturedFrame {
    pub elapsed_seconds: f32,
    pub frame_ms: f64,
    pub latency_ms: f64,
    pub context: FrameContext,
    pub exclusion: &'static str,
}

impl CapturedFrame {
    pub fn exclude_sample_boundary(&mut self, warmup: f32, end: f32) {
        if self.exclusion == "none"
            && (f64::from(self.elapsed_seconds) - self.frame_ms / 1000.0 < f64::from(warmup)
                || self.elapsed_seconds > end)
        {
            self.exclusion = "sample-boundary";
        }
    }

    pub fn exclusion(
        previous: &FrameContext,
        current: &FrameContext,
        continuous: bool,
    ) -> &'static str {
        if previous.size != current.size || previous.budget_ms != current.budget_ms {
            "display-change"
        } else if !continuous && (!previous.focused || !current.focused) {
            "unfocused"
        } else if !continuous
            && !matches!(
                previous.activity,
                RuntimeActivity::Active | RuntimeActivity::Loading
            )
        {
            "sleep"
        } else {
            "none"
        }
    }

    pub fn raw_line(&self) -> String {
        let c = &self.context;
        let clean = |s: &str| s.replace(['\t', '\r', '\n'], " ");
        format!(
            "KEINE_TRACE\t{:.6}\t{}\t{:.6}\t{:.6}\t{}\t{}\t{}\t{}\t{:?}\t{}\t{}\t{}\t{}\t{}",
            self.elapsed_seconds,
            c.frame,
            self.frame_ms,
            self.latency_ms,
            c.budget_ms
                .map_or_else(|| "unknown".into(), |ms| ms.to_string()),
            clean(&c.scene),
            c.cursor,
            clean(&c.source),
            c.activity,
            c.line,
            c.focused,
            c.size.0,
            c.size.1,
            self.exclusion
        )
    }
}

#[derive(Debug, Default, PartialEq)]
pub(super) struct BudgetSummary {
    pub measured: usize,
    pub unknown: usize,
    pub over_budget: usize,
    pub missed_slots: usize,
    pub longest_run: usize,
}

impl BudgetSummary {
    pub fn from_frames(frames: &[CapturedFrame]) -> Self {
        let mut result = Self::default();
        let mut run = 0;
        for frame in frames {
            if frame.exclusion != "none" {
                run = 0;
                continue;
            }
            let Some(budget) = frame.context.budget_ms else {
                result.unknown += 1;
                run = 0;
                continue;
            };
            result.measured += 1;
            if frame.frame_ms > budget {
                result.over_budget += 1;
                run += 1;
                result.longest_run = result.longest_run.max(run);
            } else {
                run = 0;
            }
            // An interval alone cannot prove a presentation drop. Use the
            // nearest number of refresh slots and report this as an estimate.
            result.missed_slots += (frame.frame_ms / budget).round().max(1.0) as usize - 1;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(ms: f64, budget: Option<f64>, exclusion: &'static str) -> CapturedFrame {
        CapturedFrame {
            elapsed_seconds: 1.0,
            frame_ms: ms,
            latency_ms: 2.0,
            context: FrameContext {
                budget_ms: budget,
                ..Default::default()
            },
            exclusion,
        }
    }

    #[test]
    fn refresh_budget_and_sleep_boundaries_are_distinct() {
        let samples = [
            sample(20.0, Some(1000.0 / 60.0), "none"),
            sample(10.0, Some(1000.0 / 120.0), "none"),
            sample(1000.0, Some(1000.0 / 60.0), "sleep"),
            sample(25.0, Some(1000.0 / 120.0), "none"),
            sample(30.0, None, "none"),
        ];
        assert_eq!(
            BudgetSummary::from_frames(&samples),
            BudgetSummary {
                measured: 3,
                unknown: 1,
                over_budget: 3,
                missed_slots: 2,
                longest_run: 2
            }
        );
    }

    #[test]
    fn normal_mode_excludes_sleep_focus_and_display_changes() {
        let active = FrameContext {
            focused: true,
            ..Default::default()
        };
        let mut next = active.clone();
        next.activity = RuntimeActivity::Idle;
        assert_eq!(CapturedFrame::exclusion(&active, &next, false), "none");
        assert_eq!(CapturedFrame::exclusion(&next, &active, false), "sleep");
        assert_eq!(CapturedFrame::exclusion(&active, &next, true), "none");
        next.focused = false;
        assert_eq!(CapturedFrame::exclusion(&active, &next, false), "unfocused");
        next.size = (1920, 1080);
        assert_eq!(
            CapturedFrame::exclusion(&active, &next, true),
            "display-change"
        );
    }

    #[test]
    fn partial_warmup_and_end_intervals_are_not_active_samples() {
        let mut frame = sample(1000.0, Some(8.33), "none");
        frame.elapsed_seconds = 3.5;
        frame.exclude_sample_boundary(3.0, 6.0);
        assert_eq!(frame.exclusion, "sample-boundary");
        frame.elapsed_seconds = 6.1;
        frame.exclusion = "none";
        frame.exclude_sample_boundary(3.0, 6.0);
        assert_eq!(frame.exclusion, "sample-boundary");
        frame.elapsed_seconds = 5.0;
        frame.exclusion = "none";
        frame.exclude_sample_boundary(3.0, 6.0);
        assert_eq!(frame.exclusion, "none");
    }
}
