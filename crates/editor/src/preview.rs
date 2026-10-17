pub mod engine;
pub mod instance;
pub mod performance;

use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use keine_authoring::SNAPSHOT_CHUNK_BYTES;

use crate::engine::{EngineLocator, EngineProcess};
use crate::project_key::ProjectKey;

const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(250);
const POSITION_POLL_INTERVAL: Duration = Duration::from_millis(50);
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum PreviewLifecycle {
    #[default]
    Off,
    Starting,
    Running,
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct PreviewSnapshot {
    pub lifecycle: PreviewLifecycle,
    pub revision: u64,
    pub reload: Option<keine_authoring::PreviewReload>,
    pub runtime_position: Option<(PathBuf, usize, usize)>,
    pub runtime: Option<keine_authoring::RuntimeSnapshot>,
    pub diagnostics: Vec<keine_authoring::Diagnostic>,
    pub audition_path: Option<PathBuf>,
    pub audition_paused: bool,
    pub audition_gain_db: f32,
    pub audition_error: Option<String>,
}

impl Default for PreviewSnapshot {
    fn default() -> Self {
        Self {
            lifecycle: PreviewLifecycle::Off,
            revision: 0,
            reload: None,
            runtime_position: None,
            runtime: None,
            diagnostics: Vec::new(),
            audition_path: None,
            audition_paused: false,
            audition_gain_db: 0.0,
            audition_error: None,
        }
    }
}

impl PreviewSnapshot {
    pub fn source_status(&self) -> Option<&'static str> {
        use keine_authoring::PreviewReload;
        if self.lifecycle != PreviewLifecycle::Running {
            return None;
        }
        Some(match &self.reload {
            Some(PreviewReload::Rejected { .. }) => "Source error · showing last valid preview",
            Some(PreviewReload::Restarted) => "Reloaded from project entry",
            Some(PreviewReload::Preserved) => "Source synced · playback preserved",
            None => "Preview running",
        })
    }

    fn apply_reload(&mut self, revision: u64, reload: Option<keine_authoring::PreviewReload>) {
        self.revision = revision;
        // Old source locations must not navigate the newly edited document.
        self.runtime_position = None;
        self.runtime = None;
        self.diagnostics.clear();
        if let Some(keine_authoring::PreviewReload::Rejected {
            diagnostic: Some(diagnostic),
            ..
        }) = &reload
        {
            self.diagnostics.push(diagnostic.clone());
        }
        self.reload = reload;
    }
}

enum PreviewCommand {
    Start,
    Stop,
    Show,
    ToggleAudition(PathBuf, f32),
    RestartAudition(PathBuf, f32),
    StopAudition,
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
    Shutdown,
    Fail(String),
}

const PREVIEW_PENDING_BYTES: usize = 16 * 1024 * 1024;

#[derive(Default)]
struct PendingCommands {
    control: VecDeque<PreviewCommand>,
    sources: BTreeMap<PathBuf, Vec<u8>>,
    cursor: Option<PreviewCommand>,
    bytes: usize,
    failed: bool,
}

impl PendingCommands {
    fn fail_budget(&mut self) {
        self.sources.clear();
        self.bytes = 0;
        self.cursor = None;
        self.control.clear();
        self.failed = true;
        self.control.push_front(PreviewCommand::Fail(
            "Preview source updates exceed the 16 MiB pending budget".into(),
        ));
    }
    fn push(&mut self, command: PreviewCommand) {
        match command {
            PreviewCommand::Snapshot { path, contents } => {
                if self.failed {
                    return;
                }
                let previous = self.sources.get(&path).map_or(0, Vec::len);
                let bytes = self.bytes - previous + contents.len();
                if bytes > PREVIEW_PENDING_BYTES {
                    self.fail_budget();
                } else {
                    self.sources.insert(path, contents);
                    self.bytes = bytes;
                }
            }
            command @ PreviewCommand::SetCursor { .. } => {
                if !self.failed {
                    self.cursor = Some(command);
                }
            }
            command @ (PreviewCommand::Shutdown
            | PreviewCommand::Stop
            | PreviewCommand::Fail(_)) => {
                self.control.clear();
                self.cursor = None;
                self.control.push_front(command);
            }
            command => {
                if self.failed && matches!(command, PreviewCommand::Start) {
                    return;
                }
                // Lifecycle, visibility and audition each need at most one
                // pending intent. Stop/Shutdown always precede source work.
                self.control.retain(|old| {
                    !matches!(
                        (old, &command),
                        (PreviewCommand::Start, PreviewCommand::Start)
                            | (PreviewCommand::Show, PreviewCommand::Show)
                            | (
                                PreviewCommand::ToggleAudition(..)
                                    | PreviewCommand::RestartAudition(..)
                                    | PreviewCommand::StopAudition,
                                PreviewCommand::ToggleAudition(..)
                                    | PreviewCommand::RestartAudition(..)
                                    | PreviewCommand::StopAudition
                            )
                    )
                });
                self.control.push_back(command);
            }
        }
    }

    fn pop(&mut self) -> Option<PreviewCommand> {
        if self
            .control
            .front()
            .is_some_and(|command| !matches!(command, PreviewCommand::Start))
        {
            return self.control.pop_front();
        }
        if let Some((path, contents)) = self.sources.pop_first() {
            self.bytes -= contents.len();
            return Some(PreviewCommand::Snapshot { path, contents });
        }
        self.control.pop_front().or_else(|| self.cursor.take())
    }
}

#[derive(Default)]
struct CommandMailbox {
    pending: Mutex<PendingCommands>,
    ready: Condvar,
}

impl CommandMailbox {
    fn send(&self, command: PreviewCommand) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.push(command);
            self.ready.notify_one();
        }
    }

    fn receive(&self, timeout: Duration) -> Option<PreviewCommand> {
        let mut pending = self.pending.lock().ok()?;
        if let Some(command) = pending.pop() {
            return Some(command);
        }
        let (mut pending, _) = self.ready.wait_timeout(pending, timeout).ok()?;
        pending.pop()
    }
}

pub struct PreviewController {
    commands: Arc<CommandMailbox>,
    snapshot: Arc<Mutex<PreviewSnapshot>>,
    performance: Arc<Mutex<performance::PerformanceState>>,
}

impl PreviewController {
    pub fn apply_sources(&self, sources: Vec<(PathBuf, Vec<u8>)>) -> bool {
        let Ok(mut pending) = self.commands.pending.lock() else {
            return false;
        };
        let total = sources
            .iter()
            .map(|(_, contents)| contents.len())
            .sum::<usize>();
        pending.sources.clear();
        pending.bytes = 0;
        pending.failed = false;
        if total > PREVIEW_PENDING_BYTES {
            pending.fail_budget();
            self.commands.ready.notify_one();
            return false;
        }
        pending
            .control
            .retain(|command| !matches!(command, PreviewCommand::Fail(_)));
        for (path, contents) in sources {
            pending.push(PreviewCommand::Snapshot { path, contents });
        }
        self.commands.ready.notify_one();
        true
    }
    pub fn new(project: ProjectKey) -> Arc<Self> {
        let commands = Arc::new(CommandMailbox::default());
        let receiver = commands.clone();
        let snapshot = Arc::new(Mutex::new(PreviewSnapshot::default()));
        let shared = snapshot.clone();
        let performance = Arc::new(Mutex::new(performance::PerformanceState::new(
            Instant::now(),
        )));
        let history = performance.clone();
        thread::Builder::new()
            .name("keine-preview-control".into())
            .spawn(move || worker(project, receiver, shared, history))
            .expect("preview control worker must be spawnable");
        Arc::new(Self {
            commands,
            snapshot,
            performance,
        })
    }

    pub fn snapshot(&self) -> PreviewSnapshot {
        self.snapshot
            .lock()
            .expect("preview snapshot lock poisoned")
            .clone()
    }

    pub fn performance(&self) -> performance::PerformanceSnapshot {
        self.performance
            .lock()
            .expect("preview performance lock poisoned")
            .snapshot()
    }

    pub fn start(&self) {
        self.commands.send(PreviewCommand::Start);
    }

    pub fn stop(&self) {
        self.commands.send(PreviewCommand::Stop);
    }

    pub fn show(&self) {
        self.commands.send(PreviewCommand::Show);
    }

    pub fn toggle_audition(&self, path: PathBuf, gain_db: f32) {
        self.commands
            .send(PreviewCommand::ToggleAudition(path, gain_db));
    }

    pub fn restart_audition(&self, path: PathBuf, gain_db: f32) {
        self.commands
            .send(PreviewCommand::RestartAudition(path, gain_db));
    }

    pub fn stop_audition(&self) {
        self.commands.send(PreviewCommand::StopAudition);
    }

    pub fn apply_snapshot(&self, path: PathBuf, contents: Vec<u8>) {
        self.commands
            .send(PreviewCommand::Snapshot { path, contents });
    }

    pub fn set_cursor(&self, path: PathBuf, line: usize, column: usize) {
        self.send_cursor(path, line, column, false);
    }

    pub fn seek_cursor(&self, path: PathBuf, line: usize, column: usize) {
        self.send_cursor(path, line, column, true);
    }

    fn send_cursor(&self, path: PathBuf, line: usize, column: usize, force: bool) {
        self.commands.send(PreviewCommand::SetCursor {
            path,
            line,
            column,
            force,
        });
    }
}

impl Drop for PreviewController {
    fn drop(&mut self) {
        self.commands.send(PreviewCommand::Shutdown);
    }
}

struct Worker {
    project: ProjectKey,
    generation: u64,
    engine: Option<EngineProcess>,
    audition_engine: Option<EngineProcess>,
    last_audition_poll: Option<Instant>,
    sources: BTreeMap<PathBuf, Vec<u8>>,
    revision: u64,
    cursor: Option<(PathBuf, usize, usize)>,
    applied_cursor: Option<(PathBuf, usize, u64)>,
    last_position_poll: Option<Instant>,
    shared: Arc<Mutex<PreviewSnapshot>>,
    performance: Arc<Mutex<performance::PerformanceState>>,
    last_performance_sample: Option<Instant>,
}

fn worker(
    project: ProjectKey,
    receiver: Arc<CommandMailbox>,
    shared: Arc<Mutex<PreviewSnapshot>>,
    performance: Arc<Mutex<performance::PerformanceState>>,
) {
    let mut worker = Worker {
        project,
        generation: NEXT_GENERATION.fetch_add(1, Ordering::Relaxed),
        engine: None,
        audition_engine: None,
        last_audition_poll: None,
        sources: BTreeMap::new(),
        revision: 0,
        cursor: None,
        applied_cursor: None,
        last_position_poll: None,
        shared,
        performance,
        last_performance_sample: None,
    };
    loop {
        match receiver.receive(worker.poll_interval()) {
            Some(PreviewCommand::Shutdown) => {
                worker.stop();
                break;
            }
            Some(command) => worker.handle(command),
            None => {}
        }
        worker.poll_audition();
        if let Err(error) = worker.poll_position() {
            worker.fail(error);
        }
        worker.poll_performance();
    }
}

impl Worker {
    fn poll_interval(&self) -> Duration {
        if self.engine.is_some() {
            POSITION_POLL_INTERVAL
        } else {
            IDLE_POLL_INTERVAL
        }
    }

    fn handle(&mut self, command: PreviewCommand) {
        let restart = matches!(command, PreviewCommand::RestartAudition(..));
        let result = match command {
            PreviewCommand::Start => self.start(),
            PreviewCommand::Stop => {
                self.stop();
                Ok(())
            }
            PreviewCommand::Show => self
                .engine
                .as_mut()
                .map_or(Ok(()), |engine| engine.show_preview().map(|_| ())),
            PreviewCommand::ToggleAudition(path, gain_db)
            | PreviewCommand::RestartAudition(path, gain_db) => {
                if let Err(error) = self.audition(path, gain_db, restart) {
                    self.stop_audition();
                    self.mutate(|snapshot| snapshot.audition_error = Some(error.to_string()));
                }
                Ok(())
            }
            PreviewCommand::StopAudition => {
                self.stop_audition();
                Ok(())
            }
            PreviewCommand::Snapshot { path, contents } => self.apply_snapshot(path, contents),
            PreviewCommand::SetCursor {
                path,
                line,
                column,
                force,
            } => self.set_cursor(path, line, column, force),
            PreviewCommand::Shutdown => unreachable!(),
            PreviewCommand::Fail(message) => Err(io::Error::other(message)),
        };
        if let Err(error) = result {
            self.fail(error);
        }
    }

    fn audition(&mut self, path: PathBuf, gain_db: f32, restart: bool) -> io::Result<()> {
        let (current, paused, current_gain) = {
            let snapshot = self.shared.lock().expect("preview snapshot lock poisoned");
            (
                snapshot.audition_path.clone(),
                snapshot.audition_paused,
                snapshot.audition_gain_db,
            )
        };
        if !restart
            && current_gain == gain_db
            && current.as_ref() == Some(&path)
            && let Some(engine) = &mut self.audition_engine
        {
            let path = engine.pause_audition(!paused)?;
            self.mutate(|snapshot| {
                snapshot.audition_paused = path.is_some() && !paused;
                snapshot.audition_path = path;
                snapshot.audition_error = None;
            });
            return Ok(());
        }
        if self.audition_engine.is_none() {
            let executable = EngineLocator::current()?.locate()?;
            self.audition_engine = Some(EngineProcess::launch(
                &executable,
                self.project.path(),
                self.generation,
            )?);
        }
        let path = self
            .audition_engine
            .as_mut()
            .expect("audition host was just opened")
            .audition_audio(Some(path), gain_db)?;
        self.last_audition_poll = None;
        self.mutate(|snapshot| {
            snapshot.audition_path = path;
            snapshot.audition_gain_db = gain_db;
            snapshot.audition_paused = false;
            snapshot.audition_error = None;
        });
        Ok(())
    }

    fn stop_audition(&mut self) {
        if let Some(mut engine) = self.audition_engine.take() {
            let _ = engine.shutdown();
        }
        self.last_audition_poll = None;
        self.mutate(|snapshot| {
            snapshot.audition_path = None;
            snapshot.audition_paused = false;
            snapshot.audition_error = None;
        });
    }

    fn poll_audition(&mut self) {
        if self.audition_engine.is_none()
            || self
                .last_audition_poll
                .is_some_and(|last| last.elapsed() < IDLE_POLL_INTERVAL)
        {
            return;
        }
        self.last_audition_poll = Some(Instant::now());
        match self
            .audition_engine
            .as_mut()
            .expect("audition host checked")
            .audition_state()
        {
            Ok(Some(path)) => self.mutate(|snapshot| snapshot.audition_path = Some(path)),
            Ok(None) => self.stop_audition(),
            Err(error) => {
                self.stop_audition();
                self.mutate(|snapshot| snapshot.audition_error = Some(error.to_string()));
            }
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
        engine.start_preview(self.revision)?;
        self.performance
            .lock()
            .expect("preview performance lock poisoned")
            .begin_process(engine.id());
        self.engine = Some(engine);
        self.applied_cursor = None;
        self.last_position_poll = None;
        let revision = self.revision;
        self.mutate(|snapshot| {
            snapshot.lifecycle = PreviewLifecycle::Running;
            snapshot.reload = None;
            snapshot.revision = revision;
            snapshot.runtime_position = None;
            snapshot.runtime = None;
        });
        self.sync_cursor()?;
        Ok(())
    }

    fn stop(&mut self) {
        self.stop_audition();
        if let Some(engine) = self.engine.as_mut() {
            let _ = engine.stop();
            let _ = engine.shutdown();
        }
        self.engine = None;
        self.performance
            .lock()
            .expect("preview performance lock poisoned")
            .end_process();
        self.applied_cursor = None;
        self.last_position_poll = None;
        self.mutate(|snapshot| {
            snapshot.lifecycle = PreviewLifecycle::Off;
            snapshot.reload = None;
            snapshot.runtime_position = None;
            snapshot.runtime = None;
        });
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
        let reload = if let Some(previous) = previous
            && let Some(patch) = source_patch(&previous, &contents)
            && patch.replacement.len() <= SNAPSHOT_CHUNK_BYTES
        {
            let SourcePatch { range, replacement } = patch;
            engine.apply_patch(&path, base_revision, self.revision, range, &replacement)?
        } else {
            engine.apply_snapshot(&path, self.revision, &contents)?
        };
        let revision = self.revision;
        self.mutate(|snapshot| snapshot.apply_reload(revision, reload));
        self.applied_cursor = None;
        self.last_position_poll = None;

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
            });
        }
        Ok(())
    }

    fn poll_position(&mut self) -> io::Result<()> {
        if self
            .last_position_poll
            .is_some_and(|last| last.elapsed() < POSITION_POLL_INTERVAL)
        {
            return Ok(());
        }
        let Some(engine) = self.engine.as_mut() else {
            return Ok(());
        };
        let Some((position, runtime)) = engine.take_published_position(self.revision)? else {
            self.last_position_poll = Some(Instant::now());
            return Ok(());
        };
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
            snapshot.runtime = Some(runtime);
            if snapshot.runtime_position != position {
                snapshot.runtime_position = position;
            }
        });
        Ok(())
    }

    fn fail(&mut self, error: io::Error) {
        self.engine = None;
        self.performance
            .lock()
            .expect("preview performance lock poisoned")
            .end_process();
        self.last_position_poll = None;
        self.mutate(|snapshot| {
            snapshot.lifecycle = PreviewLifecycle::Failed(error.to_string());
            snapshot.runtime_position = None;
            snapshot.runtime = None;
        });
    }

    fn mutate(&self, update: impl FnOnce(&mut PreviewSnapshot)) {
        let mut snapshot = self.shared.lock().expect("preview snapshot lock poisoned");
        update(&mut snapshot);
        self.performance
            .lock()
            .expect("preview performance lock poisoned")
            .lifecycle(Instant::now(), &snapshot.lifecycle);
    }

    fn poll_performance(&mut self) {
        if self
            .last_performance_sample
            .is_some_and(|last| last.elapsed() < performance::SAMPLE_INTERVAL)
        {
            return;
        }
        // The owned Preview Engine only; audition and Editor processes are excluded.
        let usage = self.engine.as_mut().and_then(EngineProcess::process_usage);
        let at = Instant::now();
        self.performance
            .lock()
            .expect("preview performance lock poisoned")
            .sample(at, usage);
        self.last_performance_sample = Some(at);
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SourcePatch {
    pub(crate) range: std::ops::Range<usize>,
    pub(crate) replacement: Vec<u8>,
}

pub(crate) fn source_patch(previous: &[u8], current: &[u8]) -> Option<SourcePatch> {
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

#[cfg(test)]
mod tests {
    #[test]
    fn reload_feedback_keeps_running_but_never_exposes_stale_source_locations() {
        use keine_authoring::{Diagnostic, DiagnosticLevel, PreviewReload};
        let mut snapshot = super::PreviewSnapshot {
            lifecycle: super::PreviewLifecycle::Running,
            ..Default::default()
        };
        for result in [
            PreviewReload::Rejected {
                message: "error".into(),
                diagnostic: Some(Diagnostic {
                    level: DiagnosticLevel::Error,
                    path: "scripts/main.shou".into(),
                    line: 2,
                    column: 1,
                    message: "error".into(),
                }),
            },
            PreviewReload::Preserved,
            PreviewReload::Restarted,
        ] {
            snapshot.runtime_position = Some(("scripts/main.shou".into(), 8, 1));
            snapshot.runtime = Some(Default::default());
            snapshot.apply_reload(7, Some(result.clone()));
            assert_eq!(snapshot.lifecycle, super::PreviewLifecycle::Running);
            assert!(snapshot.runtime_position.is_none() && snapshot.runtime.is_none());
            assert_eq!(
                snapshot.diagnostics.len(),
                usize::from(matches!(result, PreviewReload::Rejected { .. }))
            );
            assert_eq!(snapshot.reload, Some(result));
            assert!(snapshot.source_status().is_some());
        }
        snapshot.lifecycle = super::PreviewLifecycle::Off;
        assert!(snapshot.source_status().is_none());
    }

    use super::*;

    #[test]
    fn replay_replaces_pending_audio_intent_and_stop_cancels_it() {
        let mut pending = PendingCommands::default();
        pending.push(PreviewCommand::ToggleAudition("assets/a.opus".into(), 0.0));
        pending.push(PreviewCommand::RestartAudition("assets/b.opus".into(), 0.0));
        assert!(
            matches!(pending.pop(), Some(PreviewCommand::RestartAudition(path, _)) if path == std::path::Path::new("assets/b.opus"))
        );
        assert!(pending.pop().is_none());
        pending.push(PreviewCommand::RestartAudition("assets/a.opus".into(), 0.0));
        pending.push(PreviewCommand::StopAudition);
        assert!(matches!(pending.pop(), Some(PreviewCommand::StopAudition)));
        assert!(pending.pop().is_none());
    }

    #[test]
    fn latest_source_and_cursor_are_coalesced_and_stop_precedes_backlog() {
        let mut pending = PendingCommands::default();
        for value in 0..100u8 {
            pending.push(PreviewCommand::Snapshot {
                path: "scripts/main.shou".into(),
                contents: vec![value; 1024],
            });
            pending.push(PreviewCommand::SetCursor {
                path: "scripts/main.shou".into(),
                line: value as usize + 1,
                column: 1,
                force: false,
            });
        }
        assert_eq!(pending.bytes, 1024);
        pending.push(PreviewCommand::Stop);
        assert!(matches!(pending.pop(), Some(PreviewCommand::Stop)));
        assert!(
            matches!(pending.pop(), Some(PreviewCommand::Snapshot { contents, .. }) if contents[0] == 99)
        );
        assert!(pending.pop().is_none());
    }

    #[test]
    fn preview_budget_fails_explicitly_and_shutdown_preempts_it() {
        let mut pending = PendingCommands::default();
        pending.push(PreviewCommand::Snapshot {
            path: "scripts/main.shou".into(),
            contents: vec![0; PREVIEW_PENDING_BYTES + 1],
        });
        assert_eq!(pending.bytes, 0);
        assert!(matches!(pending.pop(), Some(PreviewCommand::Fail(_))));
        pending.push(PreviewCommand::Start);
        pending.push(PreviewCommand::Shutdown);
        assert!(matches!(pending.pop(), Some(PreviewCommand::Shutdown)));
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
}
