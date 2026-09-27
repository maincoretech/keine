pub mod engine;
pub mod instance;

use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
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
    pub runtime_position: Option<(PathBuf, usize, usize)>,
    pub diagnostics: Vec<keine_authoring::Diagnostic>,
}

impl Default for PreviewSnapshot {
    fn default() -> Self {
        Self {
            lifecycle: PreviewLifecycle::Off,
            revision: 0,
            runtime_position: None,
            diagnostics: Vec::new(),
        }
    }
}

enum PreviewCommand {
    Start,
    Stop,
    Show,
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

    pub fn start(&self) {
        let _ = self.commands.send(PreviewCommand::Start);
    }

    pub fn stop(&self) {
        let _ = self.commands.send(PreviewCommand::Stop);
    }

    pub fn show(&self) {
        let _ = self.commands.send(PreviewCommand::Show);
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
    sources: BTreeMap<PathBuf, Vec<u8>>,
    revision: u64,
    cursor: Option<(PathBuf, usize, usize)>,
    applied_cursor: Option<(PathBuf, usize, u64)>,
    last_position_poll: Option<Instant>,
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
        sources: BTreeMap::new(),
        revision: 0,
        cursor: None,
        applied_cursor: None,
        last_position_poll: None,
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
        if let Err(error) = worker.poll_position() {
            worker.fail(error);
        }
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
            PreviewCommand::Snapshot { path, contents } => self.apply_snapshot(path, contents),
            PreviewCommand::SetCursor {
                path,
                line,
                column,
                force,
            } => self.set_cursor(path, line, column, force),
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
        engine.start_preview(self.revision)?;
        self.engine = Some(engine);
        self.applied_cursor = None;
        self.last_position_poll = None;
        let revision = self.revision;
        self.mutate(|snapshot| {
            snapshot.lifecycle = PreviewLifecycle::Running;
            snapshot.revision = revision;
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
        self.applied_cursor = None;
        self.last_position_poll = None;
        self.mutate(|snapshot| {
            snapshot.lifecycle = PreviewLifecycle::Off;
            snapshot.runtime_position = None;
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
        let Some(position) = engine.take_published_position(self.revision)? else {
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
            if snapshot.runtime_position != position {
                snapshot.runtime_position = position;
            }
        });
        Ok(())
    }

    fn fail(&mut self, error: io::Error) {
        self.engine = None;
        self.last_position_poll = None;
        self.mutate(|snapshot| {
            snapshot.lifecycle = PreviewLifecycle::Failed(error.to_string());
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

#[cfg(test)]
mod tests {
    use super::*;

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
