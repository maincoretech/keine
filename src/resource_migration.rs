//! Explicit, recoverable source-reference migration after offline asset conversion.
//!
//! This deliberately does not rename, convert, or fall back between assets. A
//! replacement is eligible only when both the old file and its converted peer
//! are visible through the project's effective asset mounts.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{self, IsTerminal, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use keine_core::config::GameConfig;
use keine_loader::{
    ContentProject, DiagnosticLevel, LoaderRegistry, MAX_SOURCE_FILE_BYTES, ResourceRef,
    load_project_with, load_scenes_with,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExtensionRule {
    from: String,
    to: String,
}

#[derive(Debug)]
struct Edit {
    path: PathBuf,
    original: Vec<u8>,
    replacement: Vec<u8>,
    replacements: usize,
}

#[derive(Debug)]
struct AssetChange {
    old: String,
    new: String,
    old_bytes: u64,
    new_bytes: u64,
}

pub(crate) fn run(
    project_path: &Path,
    loader: &LoaderRegistry,
    raw_rules: &[(String, String)],
    yes: bool,
) -> Result<()> {
    let rules = parse_rules(raw_rules)?;
    let project_path = project_path
        .canonicalize()
        .with_context(|| format!("failed to resolve project path {}", project_path.display()))?;
    let (config, content) = open_project(&project_path, loader)?;
    let languages = loader
        .languages(&config.adapter.script)
        .context("failed to select script adapter")?;
    let scenes = load_scenes_with(&content, &languages)
        .context("failed to load project scenes before migration")?;

    let AssetReplacements {
        mut available,
        missing: unavailable,
        changes,
    } = collect_asset_replacements(&content, &rules)?;
    let mut aliases = changes
        .keys()
        .map(|path| (path.clone(), BTreeSet::from([path.clone()])))
        .collect::<BTreeMap<_, _>>();
    let mut missing = BTreeSet::new();
    for scene in &scenes {
        for resource in &scene.resources {
            collect_resource_replacement(
                resource,
                &config,
                &content,
                &rules,
                &mut available,
                &mut aliases,
                &mut missing,
            );
        }
    }

    let source_files = source_files(&project_path)?;
    let edits = plan_edits(&source_files, &available)?;
    for old in unavailable {
        for path in &source_files {
            if file_contains_reference(path, &old)? {
                missing.insert(old.clone());
                break;
            }
        }
    }
    if !missing.is_empty() {
        anyhow::bail!(
            "refusing to rewrite references because converted targets are missing:\n{}",
            missing
                .iter()
                .map(|path| format!("  {path}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    if edits.is_empty() {
        println!("No matching asset references found.");
        return Ok(());
    }
    let reference_counts = reference_counts(&source_files, &available)?;
    print_plan(&edits, &project_path, &changes, &aliases, &reference_counts);
    if !yes && !confirm_apply()? {
        println!("Cancelled; no files were changed.");
        return Ok(());
    }
    if yes {
        println!("\nConfirmation skipped (-y).");
    }

    let backup_root = create_backup(&project_path, &edits)?;
    if let Err(error) = apply_edits(&edits)
        .and_then(|()| validate_after_apply(&project_path, loader, &rules, &available))
    {
        restore_edits(&edits, &backup_root).context("migration failed and rollback also failed")?;
        return Err(error).context("migration failed; original files were restored");
    }
    println!(
        "\nApplied safely. Backup retained at {}",
        backup_root.display()
    );
    Ok(())
}

fn parse_rules(raw: &[(String, String)]) -> Result<Vec<ExtensionRule>> {
    let mut rules = Vec::with_capacity(raw.len());
    let mut seen = BTreeMap::new();
    for (from, to) in raw {
        let from = normalize_extension(from)?;
        let to = normalize_extension(to)?;
        if from == to {
            anyhow::bail!("extension rule {from}={to} does not change anything");
        }
        if let Some(previous) = seen.insert(from.clone(), to.clone())
            && previous != to
        {
            anyhow::bail!("conflicting rules for .{from}: .{previous} and .{to}");
        }
        rules.push(ExtensionRule { from, to });
    }
    rules.sort_by(|left, right| left.from.cmp(&right.from));
    rules.dedup();
    Ok(rules)
}

fn normalize_extension(value: &str) -> Result<String> {
    let value = value.strip_prefix('.').unwrap_or(value);
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        anyhow::bail!("invalid extension {value:?}; use letters and digits without a path");
    }
    Ok(value.to_ascii_lowercase())
}

fn open_project(root: &Path, loader: &LoaderRegistry) -> Result<(GameConfig, ContentProject)> {
    if let Some(project) = loader.open_project(root)? {
        return Ok((project.config, project.content));
    }
    let config_path = root.join("config.yaml");
    let bytes = crate::storage::read_limited(
        &config_path,
        crate::runtime::bootstrap::MAX_PROJECT_CONFIG_BYTES,
    )
    .with_context(|| format!("failed to read {}", config_path.display()))?;
    let yaml = std::str::from_utf8(&bytes)
        .with_context(|| format!("project config is not UTF-8: {}", config_path.display()))?;
    let config = GameConfig::from_yaml(yaml)
        .with_context(|| format!("invalid project config {}", config_path.display()))?;
    let content = load_project_with(root, &config.adapter.asset, loader)?;
    Ok((config, content))
}

struct AssetReplacements {
    available: BTreeMap<String, String>,
    missing: BTreeSet<String>,
    changes: BTreeMap<String, AssetChange>,
}

fn collect_asset_replacements(
    content: &ContentProject,
    rules: &[ExtensionRule],
) -> Result<AssetReplacements> {
    let mut old_assets = BTreeSet::new();
    for mount in content.asset_mounts() {
        let Some(root) = mount.filesystem_root() else {
            continue;
        };
        for path in filesystem_asset_paths(&root)? {
            if matching_rule(&path, rules).is_some() {
                old_assets.insert(portable_path(&path)?);
            }
        }
    }
    let mut available = BTreeMap::new();
    let mut missing = BTreeSet::new();
    let mut changes = BTreeMap::new();
    for old in old_assets {
        let rule = matching_rule(Path::new(&old), rules).expect("filtered above");
        let new = with_extension(&old, &rule.to)?;
        if content.contains_asset(Path::new(&new)) {
            let old_bytes = asset_len(content, Path::new(&old))?;
            let new_bytes = asset_len(content, Path::new(&new))?;
            changes.insert(
                old.clone(),
                AssetChange {
                    old: old.clone(),
                    new: new.clone(),
                    old_bytes,
                    new_bytes,
                },
            );
            available.insert(old, new);
        } else {
            missing.insert(old);
        }
    }
    Ok(AssetReplacements {
        available,
        missing,
        changes,
    })
}

fn asset_len(content: &ContentProject, path: &Path) -> Result<u64> {
    let mount = content
        .asset_mounts()
        .into_iter()
        .rev()
        .find(|mount| mount.contains_file(path))
        .with_context(|| format!("asset disappeared while planning: {}", path.display()))?;
    mount
        .open_file(path)?
        .len()
        .with_context(|| format!("failed to read asset size: {}", path.display()))
}

fn filesystem_asset_paths(root: &Path) -> Result<Vec<PathBuf>> {
    fn visit(root: &Path, directory: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
        for entry in fs::read_dir(directory)
            .with_context(|| format!("failed to read asset directory {}", directory.display()))?
        {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                visit(root, &path, output)?;
            } else if file_type.is_file() {
                output.push(path.strip_prefix(root)?.to_owned());
            }
        }
        Ok(())
    }

    let mut paths = Vec::new();
    visit(root, root, &mut paths)?;
    Ok(paths)
}

fn collect_resource_replacement(
    resource: &ResourceRef,
    config: &GameConfig,
    content: &ContentProject,
    rules: &[ExtensionRule],
    replacements: &mut BTreeMap<String, String>,
    aliases: &mut BTreeMap<String, BTreeSet<String>>,
    missing: &mut BTreeSet<String>,
) {
    let resolved = resource.resolved_path(config);
    let Some(rule) = matching_rule(Path::new(&resolved), rules) else {
        return;
    };
    if !content.contains_asset(Path::new(&resolved)) {
        return;
    }
    let Ok(new_resolved) = with_extension(&resolved, &rule.to) else {
        return;
    };
    if !content.contains_asset(Path::new(&new_resolved)) {
        missing.insert(format!("{resolved} -> {new_resolved}"));
        return;
    }
    if matching_rule(Path::new(&resource.path), rules).is_some()
        && let Ok(new_reference) = with_extension(&resource.path, &rule.to)
    {
        replacements.insert(resource.path.clone(), new_reference);
        aliases
            .entry(resource.path.clone())
            .or_default()
            .insert(resolved);
    }
}

fn matching_rule<'a>(path: &Path, rules: &'a [ExtensionRule]) -> Option<&'a ExtensionRule> {
    let extension = path.extension()?.to_str()?;
    rules
        .iter()
        .find(|rule| extension.eq_ignore_ascii_case(&rule.from))
}

fn with_extension(path: &str, extension: &str) -> Result<String> {
    let mut path = PathBuf::from(path);
    if path.is_absolute()
        || path.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        anyhow::bail!("asset reference escapes the project: {path:?}");
    }
    path.set_extension(extension);
    portable_path(&path)
}

fn portable_path(path: &Path) -> Result<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(
                part.to_str()
                    .with_context(|| format!("asset path is not UTF-8: {}", path.display()))?,
            ),
            Component::CurDir => {}
            _ => anyhow::bail!("asset path is not project-relative: {}", path.display()),
        }
    }
    Ok(parts.join("/"))
}

fn source_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_source_files(root, root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_source_files(root: &Path, directory: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(directory)
        .with_context(|| format!("failed to read {}", directory.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            let name = entry.file_name();
            if skipped_directory(&name) {
                if name == OsStr::new("assets") {
                    let manifest = path.join(".manifest.json");
                    if manifest.is_file() {
                        output.push(manifest);
                    }
                }
                continue;
            }
            collect_source_files(root, &path, output)?;
        } else if file_type.is_file() && is_source_file(&path) {
            let canonical = path.canonicalize()?;
            if !canonical.starts_with(root) {
                anyhow::bail!("source file escapes project root: {}", path.display());
            }
            let length = usize::try_from(entry.metadata()?.len())
                .context("source file size exceeds this platform")?;
            if length > MAX_SOURCE_FILE_BYTES {
                anyhow::bail!(
                    "source {} exceeds the {MAX_SOURCE_FILE_BYTES}-byte per-file limit",
                    path.display()
                );
            }
            output.push(path);
        }
    }
    Ok(())
}

fn skipped_directory(name: &OsStr) -> bool {
    matches!(
        name.to_str(),
        Some(".git" | ".keine" | "assets" | "imported_assets" | "saves" | "target")
    )
}

fn is_source_file(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "json" | "txt" | "yaml" | "yml" | "toml"
            )
        })
}

fn plan_edits(files: &[PathBuf], replacements: &BTreeMap<String, String>) -> Result<Vec<Edit>> {
    let mut edits = Vec::new();
    for path in files {
        let original = crate::storage::read_limited(path, MAX_SOURCE_FILE_BYTES)?;
        let source = std::str::from_utf8(&original)
            .with_context(|| format!("source file is not UTF-8: {}", path.display()))?;
        let (replacement, count) = if path.extension() == Some(OsStr::new("json")) {
            replace_json_strings(source, replacements)
        } else {
            replace_path_tokens(source, replacements)
        };
        if count != 0 {
            edits.push(Edit {
                path: path.clone(),
                original,
                replacement: replacement.into_bytes(),
                replacements: count,
            });
        }
    }
    Ok(edits)
}

fn replace_json_strings(source: &str, replacements: &BTreeMap<String, String>) -> (String, usize) {
    let bytes = source.as_bytes();
    let mut output = String::with_capacity(source.len());
    let mut cursor = 0;
    let mut unchanged_start = 0;
    let mut count = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != b'"' {
            cursor += 1;
            continue;
        }
        let start = cursor;
        cursor += 1;
        let mut escaped = false;
        while cursor < bytes.len() {
            let byte = bytes[cursor];
            cursor += 1;
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                break;
            }
        }
        if bytes.get(cursor - 1) != Some(&b'"') {
            break;
        }
        let value = &source[start + 1..cursor - 1];
        let followed_by_colon = bytes[cursor..]
            .iter()
            .copied()
            .find(|byte| !byte.is_ascii_whitespace())
            == Some(b':');
        if !followed_by_colon && let Some(new) = replacements.get(value) {
            output.push_str(&source[unchanged_start..start]);
            output.push('"');
            output.push_str(new);
            output.push('"');
            unchanged_start = cursor;
            count += 1;
        }
    }
    output.push_str(&source[unchanged_start..]);
    (output, count)
}

fn replace_path_tokens(source: &str, replacements: &BTreeMap<String, String>) -> (String, usize) {
    let mut ordered = replacements.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|(old, _)| std::cmp::Reverse(old.len()));
    let mut output = source.to_owned();
    let mut total = 0;
    for (old, new) in ordered {
        let mut cursor = 0;
        let mut rewritten = String::with_capacity(output.len());
        let mut count = 0;
        while let Some(offset) = output[cursor..].find(old.as_str()) {
            let start = cursor + offset;
            let end = start + old.len();
            if token_boundary(&output, start, end) {
                rewritten.push_str(&output[cursor..start]);
                rewritten.push_str(new);
                cursor = end;
                count += 1;
            } else {
                rewritten.push_str(&output[cursor..end]);
                cursor = end;
            }
        }
        rewritten.push_str(&output[cursor..]);
        if count != 0 {
            output = rewritten;
            total += count;
        }
    }
    (output, total)
}

fn token_boundary(source: &str, start: usize, end: usize) -> bool {
    let path_byte = |byte: u8| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/' | b'\\')
    };
    (start == 0 || !path_byte(source.as_bytes()[start - 1]))
        && (end == source.len() || !path_byte(source.as_bytes()[end]))
}

fn file_contains_reference(path: &Path, reference: &str) -> Result<bool> {
    Ok(file_reference_count(path, reference)? != 0)
}

fn file_reference_count(path: &Path, reference: &str) -> Result<usize> {
    let bytes = crate::storage::read_limited(path, MAX_SOURCE_FILE_BYTES)?;
    let source = std::str::from_utf8(&bytes)
        .with_context(|| format!("source file is not UTF-8: {}", path.display()))?;
    let replacements = BTreeMap::from([(reference.to_owned(), String::new())]);
    let count = if path.extension() == Some(OsStr::new("json")) {
        replace_json_strings(source, &replacements).1
    } else {
        replace_path_tokens(source, &replacements).1
    };
    Ok(count)
}

fn reference_counts(
    files: &[PathBuf],
    replacements: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, usize>> {
    let mut counts = BTreeMap::new();
    for reference in replacements.keys() {
        let mut count = 0;
        for path in files {
            count += file_reference_count(path, reference)?;
        }
        counts.insert(reference.clone(), count);
    }
    Ok(counts)
}

fn print_plan(
    edits: &[Edit],
    root: &Path,
    changes: &BTreeMap<String, AssetChange>,
    aliases: &BTreeMap<String, BTreeSet<String>>,
    reference_counts: &BTreeMap<String, usize>,
) {
    let replacements = edits.iter().map(|edit| edit.replacements).sum::<usize>();
    let rows = changes
        .values()
        .filter_map(|change| {
            let references = aliases
                .iter()
                .filter(|(_, physical)| physical.contains(&change.old))
                .map(|(reference, _)| reference_counts.get(reference).copied().unwrap_or(0))
                .sum::<usize>();
            (references != 0).then_some((change, references))
        })
        .collect::<Vec<_>>();

    println!("Asset reference migration preview\n");
    print_asset_table(&rows);
    println!(
        "\nWill rewrite {replacements} reference(s) in {} source file(s):",
        edits.len()
    );
    for edit in edits {
        let path = edit.path.strip_prefix(root).unwrap_or(&edit.path);
        println!("  {} ({})", path.display(), edit.replacements);
    }
}

fn print_asset_table(rows: &[(&AssetChange, usize)]) {
    let color = io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    print!("{}", render_asset_table(rows, color));
}

fn render_asset_table(rows: &[(&AssetChange, usize)], color: bool) -> String {
    let path_width = rows
        .iter()
        .map(|(change, _)| format!("{} → {}", change.old, change.new).chars().count())
        .max()
        .unwrap_or("Path".len())
        .max("Path".len());
    let size_width = rows
        .iter()
        .map(|(change, _)| {
            format!(
                "{} → {}",
                human_bytes(change.old_bytes),
                human_bytes(change.new_bytes)
            )
            .len()
        })
        .max()
        .unwrap_or("Size".len())
        .max("Size".len());
    let change_width = rows
        .iter()
        .map(|(change, _)| size_change(change.old_bytes, change.new_bytes).len())
        .max()
        .unwrap_or(6)
        .max("Change".len());
    let mut output = format!(
        "{:<path_width$}  {:<size_width$}  {:>change_width$}\n",
        "Path", "Size", "Change"
    );
    for (change, _) in rows {
        let plain_path = format!("{} → {}", change.old, change.new);
        let path = if color {
            format!(
                "\x1b[31m{}\x1b[0m → \x1b[32m{}\x1b[0m",
                change.old, change.new
            )
        } else {
            plain_path.clone()
        };
        let size = format!(
            "{} → {}",
            human_bytes(change.old_bytes),
            human_bytes(change.new_bytes)
        );
        output.push_str(&format!(
            "{path}{:<path_padding$}  {size:<size_width$}  {:>change_width$}\n",
            "",
            size_change(change.old_bytes, change.new_bytes),
            path_padding = path_width - plain_path.chars().count()
        ));
    }
    let old_total = rows.iter().map(|(change, _)| change.old_bytes).sum::<u64>();
    let new_total = rows.iter().map(|(change, _)| change.new_bytes).sum::<u64>();
    let reference_total = rows.iter().map(|(_, references)| references).sum::<usize>();
    output.push_str(&format!(
        "Total: {} -> {} ({}) across {} referenced asset(s), {reference_total} reference(s)\n",
        human_bytes(old_total),
        human_bytes(new_total),
        size_change(old_total, new_total),
        rows.len()
    ));
    output
}

fn human_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= MIB {
        format!("{:.2} MiB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes / KIB)
    } else {
        format!("{} B", bytes as u64)
    }
}

fn size_change(before: u64, after: u64) -> String {
    if before == 0 {
        return if after == 0 {
            "0.0%".to_owned()
        } else {
            "+inf".to_owned()
        };
    }
    let percent = (after as f64 - before as f64) * 100.0 / before as f64;
    format!("{percent:+.1}%")
}

fn confirm_apply() -> Result<bool> {
    print!("\nApply these changes? [y/N]: ");
    io::stdout()
        .flush()
        .context("failed to show confirmation")?;
    let mut response = String::new();
    io::stdin()
        .read_line(&mut response)
        .context("failed to read confirmation")?;
    Ok(confirmation_accepted(&response))
}

fn confirmation_accepted(response: &str) -> bool {
    matches!(response.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn create_backup(root: &Path, edits: &[Edit]) -> Result<PathBuf> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_secs();
    let backup = root
        .join(".keine")
        .join("asset-remap-backups")
        .join(format!("{timestamp}-{}", std::process::id()));
    fs::create_dir_all(&backup)?;
    for edit in edits {
        let relative = edit.path.strip_prefix(root).with_context(|| {
            format!(
                "source file is outside project root: {}",
                edit.path.display()
            )
        })?;
        let target = backup.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(target, &edit.original)?;
    }
    let manifest = edits
        .iter()
        .map(|edit| {
            edit.path
                .strip_prefix(root)
                .unwrap_or(&edit.path)
                .display()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(backup.join("files.txt"), format!("{manifest}\n"))?;
    Ok(backup)
}

fn apply_edits(edits: &[Edit]) -> Result<()> {
    for edit in edits {
        replace_file(&edit.path, &edit.replacement)?;
    }
    Ok(())
}

fn replace_file(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path.parent().context("source file has no parent")?;
    let nonce = format!(".keine-remap-{}", std::process::id());
    let temporary = parent.join(format!(
        "{}{nonce}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    let displaced = parent.join(format!(
        "{}{nonce}.original",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    output.write_all(contents)?;
    output.sync_all()?;
    drop(output);
    let permissions = fs::metadata(path)?.permissions();
    fs::set_permissions(&temporary, permissions)?;
    fs::rename(path, &displaced)?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::rename(&displaced, path);
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    fs::remove_file(displaced)?;
    Ok(())
}

fn restore_edits(edits: &[Edit], backup: &Path) -> Result<()> {
    let root = backup
        .ancestors()
        .nth(3)
        .context("invalid asset migration backup path")?;
    for edit in edits {
        let relative = edit.path.strip_prefix(root)?;
        let contents = fs::read(backup.join(relative))?;
        replace_file(&edit.path, &contents)?;
    }
    Ok(())
}

fn validate_after_apply(
    root: &Path,
    loader: &LoaderRegistry,
    rules: &[ExtensionRule],
    replacements: &BTreeMap<String, String>,
) -> Result<()> {
    let (config, content) = open_project(root, loader)?;
    let languages = loader.languages(&config.adapter.script)?;
    let scenes = load_scenes_with(&content, &languages)?;
    for scene in &scenes {
        for diagnostic in &scene.diagnostics {
            if diagnostic.level == DiagnosticLevel::Error {
                anyhow::bail!(
                    "{}:{}:{}: {}",
                    scene.path.display(),
                    diagnostic.span.line,
                    diagnostic.span.column,
                    diagnostic.message
                );
            }
        }
        for resource in &scene.resources {
            let resolved = resource.resolved_path(&config);
            if matching_rule(Path::new(&resolved), rules).is_some()
                && replacements.contains_key(&resource.path)
            {
                anyhow::bail!(
                    "old resource reference remains after migration: {}",
                    resource.path
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_rewrite_changes_only_complete_string_values() {
        let replacements = BTreeMap::from([
            ("audio/click.wav".to_owned(), "audio/click.opus".to_owned()),
            ("portrait.png".to_owned(), "portrait.webp".to_owned()),
        ]);
        let source = r#"{"asset":"audio/click.wav", "note":"use audio/click.wav later", "image":"portrait.png", "portrait.png":"key"}"#;
        let (output, count) = replace_json_strings(source, &replacements);
        assert_eq!(count, 2);
        assert_eq!(
            output,
            r#"{"asset":"audio/click.opus", "note":"use audio/click.wav later", "image":"portrait.webp", "portrait.png":"key"}"#
        );
    }

    #[test]
    fn text_rewrite_requires_path_boundaries() {
        let replacements = BTreeMap::from([("voice.wav".to_owned(), "voice.opus".to_owned())]);
        let (output, count) = replace_path_tokens(
            "play voice.wav\nskip voice.wav.backup and myvoice.wav",
            &replacements,
        );
        assert_eq!(count, 1);
        assert_eq!(
            output,
            "play voice.opus\nskip voice.wav.backup and myvoice.wav"
        );
    }

    #[test]
    fn extension_rules_reject_paths_and_conflicts() {
        assert!(parse_rules(&[("../wav".into(), "opus".into())]).is_err());
        assert!(
            parse_rules(&[("wav".into(), "opus".into()), ("wav".into(), "flac".into())]).is_err()
        );
    }

    #[test]
    fn sizes_use_readable_units_and_signed_changes() {
        assert_eq!(human_bytes(900), "900 B");
        assert_eq!(human_bytes(1_536), "1.5 KiB");
        assert_eq!(human_bytes(2 * 1024 * 1024), "2.00 MiB");
        assert_eq!(size_change(1_000, 250), "-75.0%");
        assert_eq!(size_change(1_000, 1_250), "+25.0%");
    }

    #[test]
    fn confirmation_is_explicit_and_case_insensitive() {
        assert!(confirmation_accepted("y\n"));
        assert!(confirmation_accepted(" YES "));
        assert!(!confirmation_accepted(""));
        assert!(!confirmation_accepted("no"));
    }

    #[test]
    fn asset_preview_is_a_borderless_three_column_layout() {
        let change = AssetChange {
            old: "audio/voice.wav".to_owned(),
            new: "audio/voice.opus".to_owned(),
            old_bytes: 2_048,
            new_bytes: 1_024,
        };
        let output = render_asset_table(&[(&change, 3)], false);
        assert!(output.starts_with("Path"));
        assert!(output.contains("Size"));
        assert!(output.contains("Change"));
        assert!(output.contains("audio/voice.wav → audio/voice.opus"));
        assert!(output.contains("2.0 KiB → 1.0 KiB"));
        assert!(output.contains("-50.0%"));
        assert!(!output.contains('+'));
        assert!(!output.contains("Refs"));
        assert!(output.ends_with(
            "Total: 2.0 KiB -> 1.0 KiB (-50.0%) across 1 referenced asset(s), 3 reference(s)\n"
        ));

        let colored = render_asset_table(&[(&change, 3)], true);
        assert!(colored.contains("\x1b[31maudio/voice.wav\x1b[0m"));
        assert!(colored.contains("\x1b[32maudio/voice.opus\x1b[0m"));
    }
}
