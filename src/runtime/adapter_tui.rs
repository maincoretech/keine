use std::collections::HashMap;
use std::fs;
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

const CONFIG_ENV: &str = "KEINE_ADAPTER_CONFIG";

#[derive(Clone)]
struct AdapterRow {
    adapter: AdapterDescriptor,
    enabled: bool,
}

pub(crate) fn configure(registry: &LoaderRegistry) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("adapter configuration requires an interactive terminal");
    }
    let path = config_path()?;
    let saved = read_selection(&path)?;
    let mut rows = registry
        .adapters()
        .into_iter()
        .map(|adapter| AdapterRow {
            enabled: saved.get(&adapter.id()).copied().unwrap_or(true),
            adapter,
        })
        .collect::<Vec<_>>();
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
                if !toggle(&mut rows, selected) {
                    message = "asset/script/store must keep at least one adapter enabled".into();
                }
            }
            KeyCode::Enter => {
                write_selection(&path, &rows)?;
                break;
            }
            KeyCode::Esc => return Ok(()),
            _ => {}
        }
    }

    drop(terminal);
    println!("adapter configuration saved · {}", path.display());
    Ok(())
}

pub(crate) fn apply_saved_selection(registry: &mut LoaderRegistry) -> Result<()> {
    let path = config_path()?;
    let selection = read_selection(&path)?;
    if selection.is_empty() {
        return Ok(());
    }
    registry.retain_adapters(|category, name| {
        selection
            .get(&adapter_id(category, name))
            .copied()
            .unwrap_or(true)
    });
    let remaining = registry.adapters();
    for category in [
        AdapterCategory::Asset,
        AdapterCategory::Script,
        AdapterCategory::Store,
    ] {
        if !remaining.iter().any(|adapter| adapter.category == category) {
            bail!(
                "adapter configuration disables every {} adapter; run `cargo adapters` to repair it",
                category.id()
            );
        }
    }
    Ok(())
}

fn toggle(rows: &mut [AdapterRow], selected: usize) -> bool {
    let Some(row) = rows.get(selected) else {
        return false;
    };
    let category = row.adapter.category;
    if row.enabled
        && category != AdapterCategory::Project
        && rows
            .iter()
            .filter(|row| row.adapter.category == category && row.enabled)
            .count()
            == 1
    {
        return false;
    }
    rows[selected].enabled = !rows[selected].enabled;
    true
}

fn draw(rows: &[AdapterRow], selected: usize, message: &str, path: &Path) -> Result<()> {
    let mut stdout = io::stdout().lock();
    execute!(stdout, MoveTo(0, 0), Clear(ClearType::All))?;
    write_line(&mut stdout, "Kēne adapters")?;
    write_line(
        &mut stdout,
        "↑/↓ select   ←/→ or Space toggle   Enter save   Esc cancel",
    )?;
    write_line(&mut stdout, "")?;

    let mut category = None;
    for (index, row) in rows.iter().enumerate() {
        if category != Some(row.adapter.category) {
            category = Some(row.adapter.category);
            write_line(&mut stdout, category_label(row.adapter.category))?;
        }
        if index == selected {
            execute!(stdout, SetAttribute(Attribute::Reverse))?;
        }
        execute!(
            stdout,
            Print(format!(
                "  {} {}",
                if row.enabled { "[x]" } else { "[ ]" },
                row.adapter.name
            )),
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

fn category_label(category: AdapterCategory) -> &'static str {
    match category {
        AdapterCategory::Asset => "ASSET",
        AdapterCategory::Script => "SCRIPT",
        AdapterCategory::Project => "PROJECT",
        AdapterCategory::Store => "STORE",
    }
}

fn adapter_id(category: AdapterCategory, name: &str) -> String {
    format!("{}:{}", category.id(), name.to_ascii_lowercase())
}

fn read_selection(path: &Path) -> Result<HashMap<String, bool>> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", path.display()));
        }
    };
    contents
        .lines()
        .enumerate()
        .filter_map(|(line, raw)| {
            let raw = raw.trim();
            (!raw.is_empty() && !raw.starts_with('#')).then_some((line, raw))
        })
        .map(|(line, raw)| {
            let (id, value) = raw.split_once('=').with_context(|| {
                format!("invalid adapter config at {}:{}", path.display(), line + 1)
            })?;
            let enabled = match value.trim() {
                "true" => true,
                "false" => false,
                _ => bail!(
                    "invalid adapter state at {}:{}; expected true or false",
                    path.display(),
                    line + 1
                ),
            };
            Ok((id.trim().to_ascii_lowercase(), enabled))
        })
        .collect()
}

fn write_selection(path: &Path, rows: &[AdapterRow]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut contents = String::from("# keine adapter selection v1\n");
    for row in rows {
        contents.push_str(&format!("{}={}\n", row.adapter.id(), row.enabled));
    }
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, contents)
        .with_context(|| format!("failed to write {}", temporary.display()))?;
    replace_file(&temporary, path).with_context(|| format!("failed to replace {}", path.display()))
}

#[cfg(not(target_os = "windows"))]
fn replace_file(source: &Path, target: &Path) -> io::Result<()> {
    fs::rename(source, target)
}

#[cfg(target_os = "windows")]
fn replace_file(source: &Path, target: &Path) -> io::Result<()> {
    match fs::rename(source, target) {
        Ok(()) => Ok(()),
        Err(_) if target.exists() => {
            fs::remove_file(target)?;
            fs::rename(source, target)
        }
        Err(error) => Err(error),
    }
}

fn config_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os(CONFIG_ENV) {
        return Ok(PathBuf::from(path));
    }
    #[cfg(target_os = "windows")]
    if let Some(root) = std::env::var_os("APPDATA") {
        return Ok(PathBuf::from(root).join("keine/adapters.conf"));
    }
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        return Ok(PathBuf::from(home).join("Library/Application Support/keine/adapters.conf"));
    }
    if let Some(root) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(root).join("keine/adapters.conf"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        return Ok(PathBuf::from(home).join(".config/keine/adapters.conf"));
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
    use super::*;

    fn row(category: AdapterCategory, name: &str, enabled: bool) -> AdapterRow {
        AdapterRow {
            adapter: AdapterDescriptor {
                category,
                name: name.into(),
            },
            enabled,
        }
    }

    #[test]
    fn required_categories_cannot_be_emptied() {
        let mut rows = vec![
            row(AdapterCategory::Asset, "fs", true),
            row(AdapterCategory::Project, "letsgal", true),
        ];
        assert!(!toggle(&mut rows, 0));
        assert!(toggle(&mut rows, 1));
        assert!(!rows[1].enabled);
    }

    #[test]
    fn one_of_multiple_adapters_can_be_disabled() {
        let mut rows = vec![
            row(AdapterCategory::Asset, "fs", true),
            row(AdapterCategory::Asset, "auto", true),
        ];
        assert!(toggle(&mut rows, 0));
        assert!(!rows[0].enabled);
        assert!(!toggle(&mut rows, 1));
    }

    #[test]
    fn raw_terminal_lines_return_to_column_zero() {
        let mut output = Vec::new();
        write_line(&mut output, "one").unwrap();
        write_line(&mut output, "two").unwrap();
        assert_eq!(output, b"one\r\ntwo\r\n");
    }
}
