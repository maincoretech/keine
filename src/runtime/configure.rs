use std::collections::HashMap;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::style::{Attribute, Print, SetAttribute};
use crossterm::terminal::{
    Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use keine_loader::{AdapterCategory, AdapterDescriptor, LoaderRegistry};

use crate::scene::video::{VideoSelection, automatic_video_backend_name};

const CONFIG_ENV: &str = "KEINE_ENGINE_CONFIG";
const VIDEO_KEY: &str = "media:video";
const MAX_CONFIG_BYTES: usize = 256 * 1024;

#[derive(Clone)]
enum ConfigRow {
    Adapter {
        adapter: AdapterDescriptor,
        enabled: bool,
    },
    Video {
        selection: VideoSelection,
        selected: bool,
    },
}

impl ConfigRow {
    fn section(&self) -> Section {
        match self {
            Self::Adapter { adapter, .. } => match adapter.category {
                AdapterCategory::Asset => Section::Asset,
                AdapterCategory::Script => Section::Script,
                AdapterCategory::Project => Section::Project,
                AdapterCategory::Store => Section::Store,
            },
            Self::Video { .. } => Section::Video,
        }
    }

    fn marker(&self) -> &'static str {
        match self {
            Self::Adapter { enabled: true, .. } => "[x]",
            Self::Adapter { enabled: false, .. } => "[ ]",
            Self::Video { selected: true, .. } => "(*)",
            Self::Video {
                selected: false, ..
            } => "( )",
        }
    }

    fn label(&self) -> String {
        match self {
            Self::Adapter { adapter, .. } => adapter.name.clone(),
            Self::Video {
                selection: VideoSelection::Automatic,
                ..
            } => format!("automatic ({})", automatic_video_backend_name()),
            Self::Video {
                selection: VideoSelection::Disabled,
                ..
            } => "disabled".into(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Asset,
    Script,
    Project,
    Store,
    Video,
}

impl Section {
    const fn label(self) -> &'static str {
        match self {
            Self::Asset => "CONTENT / ASSET",
            Self::Script => "CONTENT / SCRIPT",
            Self::Project => "CONTENT / PROJECT",
            Self::Store => "PERSISTENCE / STORE CODEC",
            Self::Video => "MEDIA / VIDEO",
        }
    }
}

pub(crate) fn configure(registry: &LoaderRegistry) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("engine configuration requires an interactive terminal");
    }
    let path = config_path()?;
    let saved = read_saved(&path)?;
    let mut rows = rows(registry, &saved)?;
    let mut selected = 0usize;
    let mut message = String::new();
    let terminal = TerminalSession::enter()?;

    loop {
        draw(&rows, selected, &message, &path)?;
        let Event::Key(key) = event::read().context("failed to read terminal input")? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        message.clear();
        match key.code {
            KeyCode::Up => selected = selected.saturating_sub(1),
            KeyCode::Down => selected = (selected + 1).min(rows.len().saturating_sub(1)),
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') => {
                if !select(&mut rows, selected) {
                    message = "asset/script/store must keep at least one adapter enabled".into();
                }
            }
            KeyCode::Enter => {
                write_configuration(&path, &rows)?;
                break;
            }
            KeyCode::Esc => return Ok(()),
            _ => {}
        }
    }

    drop(terminal);
    println!("engine configuration saved · {}", path.display());
    Ok(())
}

pub(crate) fn apply_saved_configuration(registry: &mut LoaderRegistry) -> Result<VideoSelection> {
    let saved = read_saved(&config_path()?)?;
    if !saved.values.is_empty() {
        registry.retain_adapters(|category, name| {
            saved.adapter_enabled(category, name).unwrap_or(true)
        });
    }
    let remaining = registry.adapters();
    for category in [
        AdapterCategory::Asset,
        AdapterCategory::Script,
        AdapterCategory::Store,
    ] {
        if !remaining.iter().any(|adapter| adapter.category == category) {
            bail!(
                "engine configuration disables every {} adapter; run `cargo configure` to repair it",
                category.id()
            );
        }
    }
    saved.video_selection()
}

fn rows(registry: &LoaderRegistry, saved: &SavedConfiguration) -> Result<Vec<ConfigRow>> {
    let mut rows = registry
        .adapters()
        .into_iter()
        .map(|adapter| ConfigRow::Adapter {
            enabled: saved
                .adapter_enabled(adapter.category, &adapter.name)
                .unwrap_or(true),
            adapter,
        })
        .collect::<Vec<_>>();
    let selected_video = saved.video_selection()?;
    rows.extend(
        [VideoSelection::Automatic, VideoSelection::Disabled]
            .into_iter()
            .map(|selection| ConfigRow::Video {
                selection,
                selected: selection == selected_video,
            }),
    );
    Ok(rows)
}

fn select(rows: &mut [ConfigRow], selected: usize) -> bool {
    let Some(row) = rows.get(selected) else {
        return false;
    };
    match row {
        ConfigRow::Adapter { adapter, enabled } => {
            let category = adapter.category;
            if *enabled
                && category != AdapterCategory::Project
                && rows
                    .iter()
                    .filter(|row| {
                        matches!(
                            row,
                            ConfigRow::Adapter {
                                adapter,
                                enabled: true,
                            } if adapter.category == category
                        )
                    })
                    .count()
                    == 1
            {
                return false;
            }
            let ConfigRow::Adapter { enabled, .. } = &mut rows[selected] else {
                unreachable!();
            };
            *enabled = !*enabled;
        }
        ConfigRow::Video { selection, .. } => {
            let selection = *selection;
            for row in rows {
                if let ConfigRow::Video {
                    selection: candidate,
                    selected,
                } = row
                {
                    *selected = *candidate == selection;
                }
            }
        }
    }
    true
}

fn draw(rows: &[ConfigRow], selected: usize, message: &str, path: &Path) -> Result<()> {
    let mut stdout = io::stdout().lock();
    execute!(stdout, MoveTo(0, 0), Clear(ClearType::All))?;
    write_line(&mut stdout, "Kēne configuration")?;
    write_line(
        &mut stdout,
        "↑/↓ select   ←/→ or Space change   Enter save   Esc cancel",
    )?;
    write_line(&mut stdout, "")?;

    let mut section = None;
    for (index, row) in rows.iter().enumerate() {
        if section != Some(row.section()) {
            section = Some(row.section());
            write_line(&mut stdout, row.section().label())?;
        }
        if index == selected {
            execute!(stdout, SetAttribute(Attribute::Reverse))?;
        }
        execute!(
            stdout,
            Print(format!("  {} {}", row.marker(), row.label())),
            SetAttribute(Attribute::Reset),
            Print("\r\n")
        )?;
    }
    write_line(&mut stdout, "")?;
    if !message.is_empty() {
        write_line(&mut stdout, message)?;
    }
    write_line(&mut stdout, &format!("config: {}", path.display()))?;
    stdout.flush()?;
    Ok(())
}

fn write_line(output: &mut impl Write, text: &str) -> io::Result<()> {
    output.write_all(text.as_bytes())?;
    output.write_all(b"\r\n")
}

#[derive(Default)]
struct SavedConfiguration {
    values: HashMap<String, String>,
}

impl SavedConfiguration {
    fn adapter_enabled(&self, category: AdapterCategory, name: &str) -> Option<bool> {
        self.values
            .get(&adapter_id(category, name))
            .and_then(|value| parse_bool(value))
    }

    fn video_selection(&self) -> Result<VideoSelection> {
        self.values
            .get(VIDEO_KEY)
            .map_or(Ok(VideoSelection::Automatic), |value| {
                VideoSelection::parse(value)
                    .with_context(|| format!("invalid {VIDEO_KEY} value {value:?}"))
            })
    }
}

fn adapter_id(category: AdapterCategory, name: &str) -> String {
    format!("adapter:{}:{}", category.id(), name.to_ascii_lowercase())
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn read_saved(path: &Path) -> Result<SavedConfiguration> {
    if path.exists() {
        return read_configuration(path);
    }
    Ok(SavedConfiguration::default())
}

fn read_configuration(path: &Path) -> Result<SavedConfiguration> {
    let bytes = crate::storage::read_limited(path, MAX_CONFIG_BYTES)?;
    let contents = std::str::from_utf8(&bytes)
        .with_context(|| format!("engine configuration is not UTF-8: {}", path.display()))?;
    let mut values = HashMap::new();
    for (line, raw) in contents.lines().enumerate() {
        let raw = raw.trim();
        if raw.is_empty() || raw.starts_with('#') {
            continue;
        }
        let (id, value) = raw.split_once('=').with_context(|| {
            format!(
                "invalid engine configuration at {}:{}",
                path.display(),
                line + 1
            )
        })?;
        let id = id.trim().to_ascii_lowercase();
        let value = value.trim().to_ascii_lowercase();
        if id.starts_with("adapter:") && parse_bool(&value).is_none() {
            bail!(
                "invalid adapter state at {}:{}; expected true or false",
                path.display(),
                line + 1
            );
        }
        values.insert(id, value);
    }
    Ok(SavedConfiguration { values })
}

fn write_configuration(path: &Path, rows: &[ConfigRow]) -> Result<()> {
    let mut contents = String::from("# keine engine configuration v1\n");
    for row in rows {
        if let ConfigRow::Adapter { adapter, enabled } = row {
            contents.push_str(&format!(
                "{}={enabled}\n",
                adapter_id(adapter.category, &adapter.name)
            ));
        }
    }
    let video = rows
        .iter()
        .find_map(|row| match row {
            ConfigRow::Video {
                selection,
                selected: true,
            } => Some(*selection),
            _ => None,
        })
        .unwrap_or_default();
    contents.push_str(&format!("{VIDEO_KEY}={}\n", video.id()));
    if contents.len() > MAX_CONFIG_BYTES {
        bail!("engine configuration exceeds the {MAX_CONFIG_BYTES}-byte limit");
    }
    crate::storage::write_atomically(path, contents.as_bytes())
        .with_context(|| format!("failed to replace {}", path.display()))
}

fn config_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os(CONFIG_ENV) {
        return Ok(PathBuf::from(path));
    }
    Ok(config_directory()?.join("engine.conf"))
}

fn config_directory() -> Result<PathBuf> {
    #[cfg(target_os = "windows")]
    if let Some(root) = std::env::var_os("APPDATA") {
        return Ok(PathBuf::from(root).join("keine"));
    }
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        return Ok(PathBuf::from(home).join("Library/Application Support/keine"));
    }
    if let Some(root) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(root).join("keine"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        return Ok(PathBuf::from(home).join(".config/keine"));
    }
    bail!("could not locate the user configuration directory")
}

struct TerminalSession;

impl TerminalSession {
    fn enter() -> Result<Self> {
        enable_raw_mode().context("failed to enter terminal raw mode")?;
        if let Err(error) = execute!(io::stdout(), EnterAlternateScreen, Hide) {
            let _ = disable_raw_mode();
            return Err(error).context("failed to enter alternate terminal screen");
        }
        Ok(Self)
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), Show, LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn adapter(category: AdapterCategory, name: &str, enabled: bool) -> ConfigRow {
        ConfigRow::Adapter {
            adapter: AdapterDescriptor {
                category,
                name: name.into(),
            },
            enabled,
        }
    }

    fn video(selection: VideoSelection, selected: bool) -> ConfigRow {
        ConfigRow::Video {
            selection,
            selected,
        }
    }

    #[test]
    fn required_adapter_categories_cannot_be_emptied() {
        let mut rows = vec![
            adapter(AdapterCategory::Asset, "fs", true),
            adapter(AdapterCategory::Project, "letsgal", true),
        ];
        assert!(!select(&mut rows, 0));
        assert!(select(&mut rows, 1));
        assert!(matches!(rows[1], ConfigRow::Adapter { enabled: false, .. }));
    }

    #[test]
    fn one_of_multiple_adapters_can_be_disabled() {
        let mut rows = vec![
            adapter(AdapterCategory::Asset, "fs", true),
            adapter(AdapterCategory::Asset, "auto", true),
        ];
        assert!(select(&mut rows, 0));
        assert!(matches!(rows[0], ConfigRow::Adapter { enabled: false, .. }));
        assert!(!select(&mut rows, 1));
    }

    #[test]
    fn video_selection_is_exclusive() {
        let mut rows = vec![
            video(VideoSelection::Automatic, true),
            video(VideoSelection::Disabled, false),
        ];
        assert!(select(&mut rows, 1));
        assert!(matches!(
            rows[0],
            ConfigRow::Video {
                selected: false,
                ..
            }
        ));
        assert!(matches!(rows[1], ConfigRow::Video { selected: true, .. }));
    }

    #[test]
    fn configuration_round_trip_keeps_adapters_and_video() {
        let root = temporary_path("roundtrip");
        let path = root.join("engine.conf");
        let rows = vec![
            adapter(AdapterCategory::Asset, "fs", true),
            adapter(AdapterCategory::Project, "letsgal", false),
            video(VideoSelection::Automatic, false),
            video(VideoSelection::Disabled, true),
        ];
        write_configuration(&path, &rows).unwrap();
        let saved = read_configuration(&path).unwrap();
        assert_eq!(
            saved.adapter_enabled(AdapterCategory::Asset, "fs"),
            Some(true)
        );
        assert_eq!(
            saved.adapter_enabled(AdapterCategory::Project, "letsgal"),
            Some(false)
        );
        assert_eq!(saved.video_selection().unwrap(), VideoSelection::Disabled);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn raw_terminal_lines_return_to_column_zero() {
        let mut output = Vec::new();
        write_line(&mut output, "one").unwrap();
        write_line(&mut output, "two").unwrap();
        assert_eq!(output, b"one\r\ntwo\r\n");
    }

    fn temporary_path(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("keine-engine-config-{label}-{nonce}"))
    }
}
