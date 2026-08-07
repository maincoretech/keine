//! `cargo startup` — process-to-first-frame segment timing (T-1..T7) for
//! A/B comparisons of the source-script and compiled `program.bin` scene
//! loading paths.
//!
//! Every mark sits on a boundary the runtime code already crosses, so a
//! source run and a compiled run differ only inside the scene-loading
//! segment. The capture is opt-in: normal runs never construct the timeline.

use std::time::{Duration, Instant};

use bevy::app::AppExit;
use bevy::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SceneLoadPath {
    Source,
    Compiled,
}

impl SceneLoadPath {
    const fn label(self) -> &'static str {
        match self {
            Self::Source => "source scripts",
            Self::Compiled => "compiled program.bin",
        }
    }
}

/// Capture carried from process entry into the running app.
#[derive(Debug, Clone, Copy, Resource)]
pub(crate) struct StartupTimeline {
    begin: Option<Instant>,
    cli_ready: Option<Instant>,
    project_opened: Option<Instant>,
    languages: Option<Instant>,
    store_ready: Option<Instant>,
    app_built: Option<Instant>,
    scenes_start: Option<Instant>,
    scenes_loaded: Option<Instant>,
    first_frame: Option<Instant>,
    path: SceneLoadPath,
    scene_count: usize,
    action_count: u64,
    finished: bool,
}

impl StartupTimeline {
    const fn new() -> Self {
        Self {
            begin: None,
            cli_ready: None,
            project_opened: None,
            languages: None,
            store_ready: None,
            app_built: None,
            scenes_start: None,
            scenes_loaded: None,
            first_frame: None,
            path: SceneLoadPath::Source,
            scene_count: 0,
            action_count: 0,
            finished: false,
        }
    }

    /// Start the capture at process entry, before any project work.
    pub(crate) fn begin() -> Self {
        Self {
            begin: Some(Instant::now()),
            ..Self::new()
        }
    }

    pub(crate) fn set_path(&mut self, path: SceneLoadPath) {
        self.path = path;
    }

    pub(crate) fn mark_cli_ready(&mut self) {
        self.cli_ready = Some(Instant::now());
    }

    pub(crate) fn mark_project_opened(&mut self) {
        self.project_opened = Some(Instant::now());
    }

    pub(crate) fn mark_languages(&mut self) {
        self.languages = Some(Instant::now());
    }

    pub(crate) fn mark_store(&mut self) {
        self.store_ready = Some(Instant::now());
    }

    pub(crate) fn mark_app_built(&mut self) {
        self.app_built = Some(Instant::now());
    }

    /// Start of `load_scenes_with`; the segment it opens is the only one that
    /// differs between source-script and compiled `program.bin` runs.
    pub(crate) fn mark_scenes_start(&mut self) {
        self.scenes_start = Some(Instant::now());
    }

    /// End of scene loading; also records the loaded program size.
    pub(crate) fn record_scenes(&mut self, scene_count: usize, action_count: u64) {
        self.scenes_loaded = Some(Instant::now());
        self.scene_count = scene_count;
        self.action_count = action_count;
    }

    /// Consecutive segment durations from process entry to the first frame.
    fn segments(&self) -> Vec<(Duration, &'static str)> {
        let Some(begin) = self.begin else {
            return Vec::new();
        };
        let boundaries = [
            ("cli ready", self.cli_ready),
            ("project open", self.project_opened),
            ("script language", self.languages),
            ("store + instance guard", self.store_ready),
            ("app assembly", self.app_built),
            ("scenes start", self.scenes_start),
            ("scenes loaded", self.scenes_loaded),
            ("first frame", self.first_frame),
        ];
        let mut segments = Vec::with_capacity(boundaries.len());
        let mut previous = begin;
        for (name, mark) in boundaries {
            let Some(mark) = mark else { break };
            segments.push((mark.duration_since(previous), name));
            previous = mark;
        }
        segments
    }

    pub(crate) fn print_report(&self) {
        print!(
            "{}",
            render_report(
                self.path,
                self.scene_count,
                self.action_count,
                &self.segments()
            )
        );
    }
}

fn render_report(
    path: SceneLoadPath,
    scene_count: usize,
    action_count: u64,
    segments: &[(Duration, &'static str)],
) -> String {
    let total = segments
        .iter()
        .map(|(duration, _)| *duration)
        .sum::<Duration>();
    let mut report = format!(
        "startup timing · {} · {scene_count} scene(s) · {action_count} action(s)\n",
        path.label()
    );
    for (index, (duration, name)) in segments.iter().enumerate() {
        let label = if index == 0 {
            "T-1".to_owned()
        } else {
            format!("T{index}")
        };
        report.push_str(&format!(
            "  {label:<3}  {name:<24} {:>10.3} ms\n",
            duration.as_secs_f64() * 1000.0
        ));
    }
    report.push_str(&format!(
        "  TOTAL {:<23} {:>10.3} ms\n",
        "startup",
        total.as_secs_f64() * 1000.0
    ));
    report
}

/// Runs once at the first frame: stamps it, prints the report, and exits the
/// capture like `cargo perf` does.
fn report_startup_timeline(mut timeline: ResMut<StartupTimeline>, mut commands: Commands) {
    if timeline.finished {
        return;
    }
    timeline.finished = true;
    timeline.first_frame = Some(Instant::now());
    timeline.print_report();
    commands.write_message(AppExit::Success);
}

pub(crate) fn install(app: &mut App, timeline: StartupTimeline) {
    app.insert_resource(timeline)
        .add_systems(First, report_startup_timeline);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_all_segments_with_labels_and_total() {
        let segments = [
            (Duration::from_millis(1), "cli ready"),
            (Duration::from_millis(2), "project open"),
            (Duration::from_millis(3), "script language"),
            (Duration::from_millis(4), "store + instance guard"),
            (Duration::from_millis(5), "app assembly"),
            (Duration::from_millis(6), "scenes start"),
            (Duration::from_millis(7), "scenes loaded"),
            (Duration::from_millis(8), "first frame"),
        ];
        let report = render_report(SceneLoadPath::Compiled, 2, 42, &segments);
        assert!(
            report
                .starts_with("startup timing · compiled program.bin · 2 scene(s) · 42 action(s)\n")
        );
        assert!(report.contains("T-1  cli ready"));
        assert!(report.contains("T7   first frame"));
        assert!(report.contains("TOTAL startup"));
    }
}
