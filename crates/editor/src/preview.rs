pub mod engine;
pub mod frame_transport;
pub mod instance;

use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use keine_authoring::{
    FrameTransportStats, OwnedFrame, PreviewInput, SNAPSHOT_CHUNK_BYTES, SharedFrameConsumer,
    remove_stale_mapping,
};

use crate::engine::{EngineLocator, EngineProcess};
use crate::project_key::ProjectKey;

const PREVIEW_WIDTH: u32 = 1920;
const PREVIEW_HEIGHT: u32 = 1080;
const POLL_INTERVAL: Duration = Duration::from_millis(16);
const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(250);
const POSITION_POLL_INTERVAL: Duration = Duration::from_millis(50);
const RECENT_FRAME_WINDOW: Duration = Duration::from_millis(250);
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum PreviewLifecycle {
    #[default]
    Off,
    Starting,
    Running,
    Paused,
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct PreviewSnapshot {
    pub lifecycle: PreviewLifecycle,
    pub revision: u64,
    pub frame: Option<Arc<OwnedFrame>>,
    pub last_frame_at: Option<Instant>,
    pub frame_stats: FrameTransportStats,
    pub runtime_position: Option<(PathBuf, usize, usize)>,
    pub diagnostics: Vec<keine_authoring::Diagnostic>,
}

impl Default for PreviewSnapshot {
    fn default() -> Self {
        Self {
            lifecycle: PreviewLifecycle::Off,
            revision: 0,
            frame: None,
            last_frame_at: None,
            frame_stats: FrameTransportStats::default(),
            runtime_position: None,
            diagnostics: Vec::new(),
        }
    }
}

enum PreviewCommand {
    Start,
    Stop,
    SetWindowVisible(bool),
    SetPanelVisible(bool),
    Snapshot {
        path: PathBuf,
        contents: Vec<u8>,
    },
    SetCursor {
        path: PathBuf,
        line: usize,
        column: usize,
        force: bool,
    },
    Input(PreviewInput),
    Shutdown,
}

pub struct PreviewController {
    commands: mpsc::Sender<PreviewCommand>,
    snapshot: Arc<Mutex<PreviewSnapshot>>,
}

impl PreviewController {
    pub fn new(project: ProjectKey) -> Arc<Self> {
        let (commands, receiver) = mpsc::channel();
        let snapshot = Arc::new(Mutex::new(PreviewSnapshot::default()));
        let shared = snapshot.clone();
        thread::Builder::new()
            .name("keine-preview-control".into())
            .spawn(move || worker(project, receiver, shared))
            .expect("preview control worker must be spawnable");
        Arc::new(Self { commands, snapshot })
    }

    pub fn snapshot(&self) -> PreviewSnapshot {
        self.snapshot
            .lock()
            .expect("preview snapshot lock poisoned")
            .clone()
    }

    /// Returns one coherent state sample and transfers the latest frame to the
    /// presenter. Old frames stay latest-only instead of being cloned once by
    /// the snapshot and again into GPUI's image buffer.
    pub fn take_snapshot(&self) -> PreviewSnapshot {
        let mut snapshot = self
            .snapshot
            .lock()
            .expect("preview snapshot lock poisoned");
        PreviewSnapshot {
            lifecycle: snapshot.lifecycle.clone(),
            revision: snapshot.revision,
            frame: snapshot.frame.take(),
            last_frame_at: snapshot.last_frame_at,
            frame_stats: snapshot.frame_stats,
            runtime_position: snapshot.runtime_position.clone(),
            diagnostics: snapshot.diagnostics.clone(),
        }
    }

    pub fn start(&self) {
        let _ = self.commands.send(PreviewCommand::Start);
    }

    pub fn stop(&self) {
        let _ = self.commands.send(PreviewCommand::Stop);
    }

    pub fn set_window_visible(&self, visible: bool) {
        let _ = self
            .commands
            .send(PreviewCommand::SetWindowVisible(visible));
    }

    pub fn set_panel_visible(&self, visible: bool) {
        let _ = self.commands.send(PreviewCommand::SetPanelVisible(visible));
    }

    pub fn apply_snapshot(&self, path: PathBuf, contents: Vec<u8>) {
        let _ = self
            .commands
            .send(PreviewCommand::Snapshot { path, contents });
    }

    pub fn set_cursor(&self, path: PathBuf, line: usize, column: usize) {
        self.send_cursor(path, line, column, false);
    }

    pub fn seek_cursor(&self, path: PathBuf, line: usize, column: usize) {
        self.send_cursor(path, line, column, true);
    }

    fn send_cursor(&self, path: PathBuf, line: usize, column: usize, force: bool) {
        let _ = self.commands.send(PreviewCommand::SetCursor {
            path,
            line,
            column,
            force,
        });
    }

    pub fn input(&self, input: PreviewInput) {
        let _ = self.commands.send(PreviewCommand::Input(input));
    }
}

impl Drop for PreviewController {
    fn drop(&mut self) {
        let _ = self.commands.send(PreviewCommand::Shutdown);
    }
}

struct Worker {
    project: ProjectKey,
    generation: u64,
    engine: Option<EngineProcess>,
    frames: Option<SharedFrameConsumer>,
    sources: BTreeMap<PathBuf, Vec<u8>>,
    revision: u64,
    cursor: Option<(PathBuf, usize, usize)>,
    applied_cursor: Option<(PathBuf, usize, u64)>,
    last_position_poll: Option<Instant>,
    window_visible: bool,
    panel_visible: bool,
    effectively_visible: bool,
    shared: Arc<Mutex<PreviewSnapshot>>,
}

fn worker(
    project: ProjectKey,
    receiver: mpsc::Receiver<PreviewCommand>,
    shared: Arc<Mutex<PreviewSnapshot>>,
) {
    let mut worker = Worker {
        project,
        generation: NEXT_GENERATION.fetch_add(1, Ordering::Relaxed),
        engine: None,
        frames: None,
        sources: BTreeMap::new(),
        revision: 0,
        cursor: None,
        applied_cursor: None,
        last_position_poll: None,
        window_visible: true,
        panel_visible: true,
        effectively_visible: true,
        shared,
    };
    loop {
        match receiver.recv_timeout(worker.poll_interval()) {
            Ok(PreviewCommand::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                worker.stop();
                break;
            }
            Ok(command) => worker.handle(command),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        worker.poll_frame();
    }
}

impl Worker {
    fn poll_interval(&self) -> Duration {
        let recently_active = self
            .shared
            .lock()
            .expect("preview snapshot lock poisoned")
            .last_frame_at
            .is_some_and(|instant| instant.elapsed() < RECENT_FRAME_WINDOW);
        if self.engine.is_some() && self.effectively_visible && recently_active {
            POLL_INTERVAL
        } else {
            IDLE_POLL_INTERVAL
        }
    }

    fn handle(&mut self, command: PreviewCommand) {
        let result = match command {
            PreviewCommand::Start => self.start(),
            PreviewCommand::Stop => {
                self.stop();
                Ok(())
            }
            PreviewCommand::SetWindowVisible(visible) => {
                self.window_visible = visible;
                self.update_visibility()
            }
            PreviewCommand::SetPanelVisible(visible) => {
                self.panel_visible = visible;
                self.update_visibility()
            }
            PreviewCommand::Snapshot { path, contents } => self.apply_snapshot(path, contents),
            PreviewCommand::SetCursor {
                path,
                line,
                column,
                force,
            } => self.set_cursor(path, line, column, force),
            PreviewCommand::Input(input) => self.input(input),
            PreviewCommand::Shutdown => unreachable!(),
        };
        if let Err(error) = result {
            self.fail(error);
        }
    }

    fn start(&mut self) -> io::Result<()> {
        if self.engine.is_some() {
            return Ok(());
        }
        self.mutate(|snapshot| snapshot.lifecycle = PreviewLifecycle::Starting);
        let executable = EngineLocator::current()?.locate()?;
        let mut engine = EngineProcess::launch(&executable, self.project.path(), self.generation)?;
        let report = engine.validate()?;
        self.mutate(|snapshot| snapshot.diagnostics = report.diagnostics);
        for (path, contents) in &self.sources {
            self.revision = self.revision.wrapping_add(1).max(1);
            engine.apply_snapshot(path, self.revision, contents)?;
        }
        let project_key =
            u64::from_str_radix(&self.project.workspace_id(), 16).map_err(io::Error::other)?;
        let path = std::env::temp_dir().join(format!(
            "keine-preview-{}-{}.frames",
            std::process::id(),
            self.generation
        ));
        remove_stale_mapping(&path)?;
        let frames = SharedFrameConsumer::create(
            path,
            project_key,
            self.generation,
            PREVIEW_WIDTH,
            PREVIEW_HEIGHT,
        )?;
        engine.start_preview(frames.descriptor().clone(), self.revision)?;
        if !self.effectively_visible {
            engine.pause()?;
        }
        self.engine = Some(engine);
        self.frames = Some(frames);
        self.applied_cursor = None;
        self.last_position_poll = None;
        let lifecycle = if self.effectively_visible {
            PreviewLifecycle::Running
        } else {
            PreviewLifecycle::Paused
        };
        let revision = self.revision;
        self.mutate(|snapshot| {
            snapshot.lifecycle = lifecycle;
            snapshot.revision = revision;
            snapshot.frame = None;
            snapshot.last_frame_at = Some(Instant::now());
            snapshot.frame_stats = FrameTransportStats::default();
            snapshot.runtime_position = None;
        });
        self.sync_cursor()?;
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(engine) = self.engine.as_mut() {
            let _ = engine.stop();
            let _ = engine.shutdown();
        }
        self.engine = None;
        self.frames = None;
        self.applied_cursor = None;
        self.last_position_poll = None;
        self.mutate(|snapshot| {
            snapshot.lifecycle = PreviewLifecycle::Off;
            snapshot.frame = None;
            snapshot.last_frame_at = None;
            snapshot.frame_stats = FrameTransportStats::default();
            snapshot.runtime_position = None;
        });
    }

    fn update_visibility(&mut self) -> io::Result<()> {
        let visible = self.window_visible && self.panel_visible;
        if self.effectively_visible == visible {
            return Ok(());
        }
        self.effectively_visible = visible;
        if visible {
            self.last_position_poll = None;
        }
        let Some(engine) = self.engine.as_mut() else {
            return Ok(());
        };
        let lifecycle = if visible {
            engine.resume()?;
            PreviewLifecycle::Running
        } else {
            engine.pause()?;
            PreviewLifecycle::Paused
        };
        self.mutate(|snapshot| {
            snapshot.lifecycle = lifecycle;
            snapshot.last_frame_at = visible.then(Instant::now);
        });
        Ok(())
    }

    fn apply_snapshot(&mut self, path: PathBuf, contents: Vec<u8>) -> io::Result<()> {
        let previous = self.sources.insert(path.clone(), contents.clone());
        if previous.as_deref() == Some(contents.as_slice()) {
            return Ok(());
        }
        let Some(engine) = self.engine.as_mut() else {
            return Ok(());
        };
        let base_revision = self.revision;
        self.revision = self.revision.wrapping_add(1).max(1);
        if let Some(previous) = previous
            && let Some(patch) = source_patch(&previous, &contents)
            && patch.replacement.len() <= SNAPSHOT_CHUNK_BYTES
        {
            let SourcePatch { range, replacement } = patch;
            engine.apply_patch(&path, base_revision, self.revision, range, &replacement)?;
        } else {
            engine.apply_snapshot(&path, self.revision, &contents)?;
        }
        let revision = self.revision;
        self.mutate(|snapshot| {
            snapshot.revision = revision;
            snapshot.frame = None;
            snapshot.last_frame_at = Some(Instant::now());
        });
        self.applied_cursor = None;
        self.last_position_poll = None;
        self.sync_cursor()?;
        Ok(())
    }

    fn set_cursor(
        &mut self,
        path: PathBuf,
        line: usize,
        column: usize,
        force: bool,
    ) -> io::Result<()> {
        if path.extension().and_then(|extension| extension.to_str()) != Some("shou") {
            return Ok(());
        }
        self.cursor = Some((path, line, column));
        if force {
            self.applied_cursor = None;
        }
        self.last_position_poll = None;
        self.sync_cursor()
    }

    fn sync_cursor(&mut self) -> io::Result<()> {
        let Some((path, line, column)) = self.cursor.as_ref() else {
            return Ok(());
        };
        let Some(engine) = self.engine.as_mut() else {
            return Ok(());
        };
        if self
            .applied_cursor
            .as_ref()
            .is_some_and(|(applied_path, applied_line, revision)| {
                applied_path == path && applied_line == line && *revision == self.revision
            })
        {
            return Ok(());
        }
        let position = engine.set_execution_cursor(self.revision, path, *line, *column)?;
        self.applied_cursor = Some((path.clone(), *line, self.revision));
        if let Some(position) = position {
            self.mutate(|snapshot| {
                snapshot.runtime_position = Some(position);
                snapshot.last_frame_at = Some(Instant::now());
            });
        }
        Ok(())
    }

    fn input(&mut self, input: PreviewInput) -> io::Result<()> {
        let Some(engine) = self.engine.as_mut() else {
            return Ok(());
        };
        engine.input(self.revision, input)?;
        self.last_position_poll = None;
        self.mutate(|snapshot| snapshot.last_frame_at = Some(Instant::now()));
        Ok(())
    }

    fn poll_frame(&mut self) {
        if let Err(error) = self.poll_position() {
            self.fail(error);
            return;
        }
        let Some(frames) = self.frames.as_mut() else {
            return;
        };
        match frames.read_latest(self.revision) {
            Ok(Some(frame)) => {
                let stats = frames.stats();
                self.mutate(|snapshot| {
                    snapshot.frame = Some(Arc::new(frame));
                    snapshot.last_frame_at = Some(Instant::now());
                    snapshot.frame_stats = stats;
                });
            }
            Ok(None) => {}
            Err(error) => self.fail(error),
        }
    }

    fn poll_position(&mut self) -> io::Result<()> {
        if !self.effectively_visible
            || self
                .last_position_poll
                .is_some_and(|last| last.elapsed() < POSITION_POLL_INTERVAL)
        {
            return Ok(());
        }
        let Some(engine) = self.engine.as_mut() else {
            return Ok(());
        };
        let position = engine.execution_location(self.revision)?;
        self.last_position_poll = Some(Instant::now());
        if self.applied_cursor.as_ref().is_some_and(|(path, line, _)| {
            position
                .as_ref()
                .is_none_or(|(current_path, current_line, _)| {
                    current_path != path || current_line != line
                })
        }) {
            self.applied_cursor = None;
        }
        self.mutate(|snapshot| {
            if snapshot.runtime_position != position {
                snapshot.runtime_position = position;
                snapshot.last_frame_at = Some(Instant::now());
            }
        });
        Ok(())
    }

    fn fail(&mut self, error: io::Error) {
        self.engine = None;
        self.frames = None;
        self.last_position_poll = None;
        self.mutate(|snapshot| {
            snapshot.lifecycle = PreviewLifecycle::Failed(error.to_string());
            snapshot.frame = None;
            snapshot.last_frame_at = None;
            snapshot.runtime_position = None;
        });
    }

    fn mutate(&self, update: impl FnOnce(&mut PreviewSnapshot)) {
        update(&mut self.shared.lock().expect("preview snapshot lock poisoned"));
    }
}

#[derive(Debug, PartialEq, Eq)]
struct SourcePatch {
    range: std::ops::Range<usize>,
    replacement: Vec<u8>,
}

fn source_patch(previous: &[u8], current: &[u8]) -> Option<SourcePatch> {
    if previous == current {
        return None;
    }
    let previous_text = std::str::from_utf8(previous).ok()?;
    let current_text = std::str::from_utf8(current).ok()?;
    let mut prefix = previous
        .iter()
        .zip(current)
        .take_while(|(left, right)| left == right)
        .count();
    while !previous_text.is_char_boundary(prefix) || !current_text.is_char_boundary(prefix) {
        prefix = prefix.saturating_sub(1);
    }
    let max_suffix = previous.len().min(current.len()).saturating_sub(prefix);
    let mut suffix = previous
        .iter()
        .rev()
        .zip(current.iter().rev())
        .take(max_suffix)
        .take_while(|(left, right)| left == right)
        .count();
    while !previous_text.is_char_boundary(previous.len() - suffix)
        || !current_text.is_char_boundary(current.len() - suffix)
    {
        suffix = suffix.saturating_sub(1);
    }
    Some(SourcePatch {
        range: prefix..previous.len() - suffix,
        replacement: current[prefix..current.len() - suffix].to_vec(),
    })
}

pub fn map_preview_point(
    surface_width: f32,
    surface_height: f32,
    x: f32,
    y: f32,
) -> Option<(f32, f32)> {
    if surface_width <= 0. || surface_height <= 0. {
        return None;
    }
    let scale = (surface_width / PREVIEW_WIDTH as f32).min(surface_height / PREVIEW_HEIGHT as f32);
    let rendered_width = PREVIEW_WIDTH as f32 * scale;
    let rendered_height = PREVIEW_HEIGHT as f32 * scale;
    let left = (surface_width - rendered_width) * 0.5;
    let top = (surface_height - rendered_height) * 0.5;
    if x < left || y < top || x > left + rendered_width || y > top + rendered_height {
        return None;
    }
    Some(((x - left) / scale, (y - top) / scale))
}

#[cfg(test)]
mod tests {
    use super::*;
    use keine_authoring::{FrameMetadata, PixelFormat};

    #[test]
    fn letterbox_mapping_rejects_bars_and_preserves_design_coordinates() {
        assert_eq!(
            map_preview_point(960., 540., 480., 270.),
            Some((960., 540.))
        );
        assert_eq!(map_preview_point(960., 800., 480., 129.), None);
        assert_eq!(
            map_preview_point(960., 800., 480., 400.),
            Some((960., 540.))
        );
    }

    #[test]
    fn source_patch_keeps_utf8_boundaries_and_only_replaces_the_changed_span() {
        let previous = "scene 开始 {\n  narrator: \"旧句子\"\n}\n";
        let current = "scene 开始 {\n  narrator: \"新句子\"\n}\n";
        let patch = source_patch(previous.as_bytes(), current.as_bytes()).unwrap();
        assert!(previous.is_char_boundary(patch.range.start));
        assert!(previous.is_char_boundary(patch.range.end));
        let mut rebuilt = previous.as_bytes().to_vec();
        rebuilt.splice(patch.range, patch.replacement);
        assert_eq!(rebuilt, current.as_bytes());
    }

    #[test]
    fn presenter_takes_each_latest_frame_only_once() {
        let (commands, _receiver) = mpsc::channel();
        let frame = Arc::new(OwnedFrame {
            metadata: FrameMetadata {
                project_key: 1,
                session_generation: 2,
                document_revision: 3,
                frame_id: 4,
                width: 1,
                height: 1,
                stride: 4,
                pixel_format: PixelFormat::Bgra8Srgb,
            },
            bytes: vec![1, 2, 3, 4],
        });
        let snapshot = Arc::new(Mutex::new(PreviewSnapshot {
            frame: Some(frame.clone()),
            ..PreviewSnapshot::default()
        }));
        let controller = PreviewController { commands, snapshot };

        let first = controller.take_snapshot().frame.unwrap();
        assert!(Arc::ptr_eq(&first, &frame));
        assert!(controller.take_snapshot().frame.is_none());
    }
}
