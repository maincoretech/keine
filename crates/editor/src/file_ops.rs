use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use image::{ImageFormat, ImageReader};
use keine_core::config::{EiyashouAssetEntry, EiyashouAssetManifest, GameConfig};

use crate::authoring::AssetKind;
use crate::document::atomic_source;

const MAX_MEDIA_BYTES: u64 = 512 * 1024 * 1024;
const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
const PROBE_BYTES: usize = 1024 * 1024;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportResult {
    pub destination: PathBuf,
    pub manifest_update: Option<(PathBuf, String)>,
    pub registered: bool,
}

pub fn create_file(root: &Path, parent: &Path, name: &str) -> io::Result<PathBuf> {
    let relative = child_path(parent, name)?;
    let destination = confined_destination(root, &relative)?;
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&destination)?;
    Ok(relative)
}

pub fn create_directory(root: &Path, parent: &Path, name: &str) -> io::Result<PathBuf> {
    let relative = child_path(parent, name)?;
    let destination = confined_destination(root, &relative)?;
    fs::create_dir(&destination)?;
    Ok(relative)
}

pub fn import_external(root: &Path, target_dir: &Path, source: &Path) -> io::Result<ImportResult> {
    let metadata = fs::symlink_metadata(source)?;
    if !metadata.file_type().is_file() {
        return Err(invalid("Only files can be imported here"));
    }
    let file_name = source
        .file_name()
        .ok_or_else(|| invalid("The imported file has no name"))?;
    let relative = checked_relative(target_dir)?.join(file_name);
    let destination = confined_destination(root, &relative)?;
    if destination.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", relative.display()),
        ));
    }

    let extension = extension(source);
    if is_media_extension(&extension) {
        let kind = kind_for_path(&relative).ok_or_else(|| {
            invalid("Drop media into Background, Figure, Voice, BGM, SE, or Video")
        })?;
        validate_resource(source, kind, &extension)?;
        let id = identifier_from_filename(source)?;
        let (manifest_relative, old_manifest) = manifest_source(root)?;
        let manifest = EiyashouAssetManifest::from_yaml(&old_manifest)
            .map_err(|error| invalid(format!("Invalid asset manifest: {error}")))?;
        reject_manifest_conflict(&manifest, kind, &id, &relative)?;
        let new_manifest = insert_manifest_entry(&old_manifest, kind, &id, &relative)?;
        let manifest_path = root.join(&manifest_relative);

        copy_atomic(source, &destination)?;
        if let Err(error) = atomic_source(&manifest_path, new_manifest.as_bytes()) {
            let _ = fs::remove_file(&destination);
            return Err(error);
        }
        return Ok(ImportResult {
            destination: relative,
            manifest_update: Some((manifest_relative, new_manifest)),
            registered: true,
        });
    }

    copy_atomic(source, &destination)?;
    Ok(ImportResult {
        destination: relative,
        manifest_update: None,
        registered: false,
    })
}

pub fn move_entry(root: &Path, source: &Path, target_dir: &Path) -> io::Result<ImportResult> {
    let source = checked_relative(source)?;
    let target_dir = checked_relative_or_root(target_dir)?;
    let name = source
        .file_name()
        .ok_or_else(|| invalid("The source has no name"))?;
    let destination = target_dir.join(name);
    move_or_rename(root, &source, &destination)
}

pub fn rename_entry(root: &Path, source: &Path, name: &str) -> io::Result<ImportResult> {
    let source = checked_relative(source)?;
    let parent = source.parent().unwrap_or_else(|| Path::new(""));
    let destination = child_path(parent, name)?;
    move_or_rename(root, &source, &destination)
}

pub fn copy_entry(root: &Path, source: &Path, target_dir: &Path) -> io::Result<Vec<ImportResult>> {
    let source = checked_relative(source)?;
    let absolute = confined_existing(root, &source)?;
    if absolute.is_file() {
        return import_external(root, target_dir, &absolute).map(|result| vec![result]);
    }

    let directory_name = source
        .file_name()
        .ok_or_else(|| invalid("The source has no name"))?;
    let new_root = checked_relative_or_root(target_dir)?.join(directory_name);
    if root.join(&new_root).exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", new_root.display()),
        ));
    }
    fs::create_dir(root.join(&new_root))?;
    let manifest_backup = manifest_source(root).ok();
    let mut results = Vec::new();
    let result = copy_directory_contents(root, &absolute, &new_root, &mut results);
    if result.is_err() {
        let _ = fs::remove_dir_all(root.join(&new_root));
        if let Some((relative, source)) = manifest_backup {
            let _ = atomic_source(&root.join(relative), source.as_bytes());
        }
    }
    result.map(|_| results)
}

pub fn delete_entry(root: &Path, source: &Path) -> io::Result<()> {
    let source = checked_relative(source)?;
    if mapped_paths(root)?
        .iter()
        .any(|mapped| mapped == &source || mapped.starts_with(&source))
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Delete mapped assets from Asset",
        ));
    }
    let absolute = confined_existing(root, &source)?;
    if absolute.is_dir() {
        fs::remove_dir_all(absolute)
    } else {
        fs::remove_file(absolute)
    }
}

fn move_or_rename(root: &Path, source: &Path, destination: &Path) -> io::Result<ImportResult> {
    let source_path = confined_existing(root, source)?;
    let destination = checked_relative(destination)?;
    if destination.starts_with(source) {
        return Err(invalid("A folder cannot be moved into itself"));
    }
    let destination_path = confined_destination(root, &destination)?;
    if destination_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", destination.display()),
        ));
    }

    let (manifest_relative, old_manifest) = manifest_source(root)?;
    let mapped_entries = mapped_entries_from_source(&old_manifest)?;
    if source_path.is_file()
        && let Some((kind, _)) = mapped_entries.iter().find(|(_, path)| path == source)
    {
        validate_resource(&source_path, *kind, &extension(&destination))?;
    }
    let rewrites = mapped_entries
        .into_iter()
        .filter_map(|(_, path)| {
            path.strip_prefix(source).ok().map(|suffix| {
                let rewritten = if suffix.as_os_str().is_empty() {
                    destination.clone()
                } else {
                    destination.join(suffix)
                };
                (path.clone(), rewritten)
            })
        })
        .collect::<Vec<_>>();
    let manifest_update = if rewrites.is_empty() {
        None
    } else {
        Some(rewrite_manifest_paths(&old_manifest, &rewrites)?)
    };

    fs::rename(&source_path, &destination_path)?;
    if let Some(new_manifest) = &manifest_update
        && let Err(error) = atomic_source(&root.join(&manifest_relative), new_manifest.as_bytes())
    {
        let _ = fs::rename(&destination_path, &source_path);
        return Err(error);
    }
    Ok(ImportResult {
        destination,
        manifest_update: manifest_update.map(|source| (manifest_relative, source)),
        registered: false,
    })
}

fn copy_directory_contents(
    root: &Path,
    source: &Path,
    target: &Path,
    results: &mut Vec<ImportResult>,
) -> io::Result<()> {
    let mut entries = fs::read_dir(source)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(invalid("Symbolic links cannot be copied"));
        }
        if file_type.is_dir() {
            let child_target = target.join(entry.file_name());
            fs::create_dir(root.join(&child_target))?;
            copy_directory_contents(root, &entry.path(), &child_target, results)?;
        } else if file_type.is_file() {
            results.push(import_external(root, target, &entry.path())?);
        }
    }
    Ok(())
}

fn manifest_source(root: &Path) -> io::Result<(PathBuf, String)> {
    let config = fs::read_to_string(root.join("config.yaml"))?;
    let config = GameConfig::from_yaml(&config)
        .map_err(|error| invalid(format!("Invalid config.yaml: {error}")))?;
    if config.adapter.script != "keine" {
        return Err(invalid("Asset import requires a native Kēne project"));
    }
    let relative = checked_relative(Path::new(&config.script.assets))?;
    let source = fs::read_to_string(root.join(&relative))?;
    Ok((relative, source))
}

fn mapped_paths(root: &Path) -> io::Result<Vec<PathBuf>> {
    let (_, source) = manifest_source(root)?;
    mapped_paths_from_source(&source)
}

fn mapped_paths_from_source(source: &str) -> io::Result<Vec<PathBuf>> {
    Ok(mapped_entries_from_source(source)?
        .into_iter()
        .map(|(_, path)| path)
        .collect())
}

fn mapped_entries_from_source(source: &str) -> io::Result<Vec<(AssetKind, PathBuf)>> {
    let manifest = EiyashouAssetManifest::from_yaml(source)
        .map_err(|error| invalid(format!("Invalid asset manifest: {error}")))?;
    Ok([
        (AssetKind::Background, manifest.backgrounds),
        (AssetKind::Figure, manifest.figures),
        (AssetKind::Voice, manifest.voices),
        (AssetKind::Bgm, manifest.bgm),
        (AssetKind::Effect, manifest.effects),
        (AssetKind::Video, manifest.videos),
    ]
    .into_iter()
    .flat_map(|(kind, entries)| entries.into_values().map(move |entry| (kind, entry)))
    .map(|(kind, entry)| (kind, PathBuf::from(entry.into_path())))
    .collect())
}

fn entries_for_kind(
    manifest: &EiyashouAssetManifest,
    kind: AssetKind,
) -> &HashMap<String, EiyashouAssetEntry> {
    match kind {
        AssetKind::Background => &manifest.backgrounds,
        AssetKind::Figure => &manifest.figures,
        AssetKind::Voice => &manifest.voices,
        AssetKind::Bgm => &manifest.bgm,
        AssetKind::Effect => &manifest.effects,
        AssetKind::Video => &manifest.videos,
    }
}

fn reject_manifest_conflict(
    manifest: &EiyashouAssetManifest,
    kind: AssetKind,
    id: &str,
    path: &Path,
) -> io::Result<()> {
    let entries = entries_for_kind(manifest, kind);
    if entries.contains_key(id) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} `{id}` already exists", kind.label()),
        ));
    }
    let path = slash_path(path);
    if entries.values().any(|entry| entry.path() == path) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} is already registered", path),
        ));
    }
    Ok(())
}

fn insert_manifest_entry(
    source: &str,
    kind: AssetKind,
    id: &str,
    path: &Path,
) -> io::Result<String> {
    let namespace = namespace(kind);
    let entry = format!("  {id}: '{}'\n", yaml_single_quote(&slash_path(path)));
    let lines = line_ranges(source);
    let mut namespace_start = None;
    for (start, end) in &lines {
        let line = &source[*start..*end];
        let value = line.trim_end_matches(['\r', '\n']);
        if value.starts_with(char::is_whitespace) || value.trim_start().starts_with('#') {
            continue;
        }
        let Some((key, rest)) = value.split_once(':') else {
            continue;
        };
        if key.trim() == namespace {
            namespace_start = Some((*start, *end, rest.trim()));
            break;
        }
    }

    if let Some((start, end, rest)) = namespace_start {
        if rest == "{}" || rest.starts_with("{} #") {
            let mut output = String::with_capacity(source.len() + entry.len());
            output.push_str(&source[..start]);
            output.push_str(namespace);
            output.push_str(":\n");
            output.push_str(&entry);
            output.push_str(&source[end..]);
            return validate_manifest(output);
        }
        if !rest.is_empty() && !rest.starts_with('#') {
            return Err(invalid(format!("`{namespace}` must be a map")));
        }
        let insertion = lines
            .iter()
            .skip_while(|(line_start, _)| *line_start <= start)
            .find_map(|(line_start, line_end)| {
                let line = source[*line_start..*line_end].trim_end_matches(['\r', '\n']);
                (!line.is_empty()
                    && !line.starts_with(char::is_whitespace)
                    && !line.trim_start().starts_with('#'))
                .then_some(*line_start)
            })
            .unwrap_or(source.len());
        let mut output = String::with_capacity(source.len() + entry.len());
        output.push_str(&source[..insertion]);
        output.push_str(&entry);
        output.push_str(&source[insertion..]);
        return validate_manifest(output);
    }

    let mut output = source.to_owned();
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    if !output.is_empty() && !output.ends_with("\n\n") {
        output.push('\n');
    }
    output.push_str(namespace);
    output.push_str(":\n");
    output.push_str(&entry);
    validate_manifest(output)
}

fn rewrite_manifest_paths(source: &str, rewrites: &[(PathBuf, PathBuf)]) -> io::Result<String> {
    let rewrites = rewrites
        .iter()
        .map(|(old, new)| (slash_path(old), slash_path(new)))
        .collect::<HashMap<_, _>>();
    let mut namespace = false;
    let mut output = String::with_capacity(source.len());
    for (start, end) in line_ranges(source) {
        let full_line = &source[start..end];
        let ending = if full_line.ends_with("\r\n") {
            "\r\n"
        } else if full_line.ends_with('\n') {
            "\n"
        } else {
            ""
        };
        let line = full_line.trim_end_matches(['\r', '\n']);
        let indent = line.len() - line.trim_start().len();
        if indent == 0 && !line.trim().is_empty() && !line.trim_start().starts_with('#') {
            namespace = line
                .split_once(':')
                .is_some_and(|(key, _)| is_asset_namespace(key.trim()));
        }
        let mut replaced = None;
        if namespace
            && (indent == 2 || (indent >= 4 && line.trim_start().starts_with("path:")))
            && let Some((prefix, value)) = line.split_once(':')
        {
            let (scalar, comment) = split_yaml_comment(value.trim_start());
            if let Some(old) = parse_yaml_scalar(scalar)
                && let Some(new) = rewrites.get(old)
            {
                replaced = Some(format!(
                    "{prefix}: '{}'{}{}",
                    yaml_single_quote(new),
                    if comment.is_empty() { "" } else { " " },
                    comment
                ));
            }
        }
        output.push_str(replaced.as_deref().unwrap_or(line));
        output.push_str(ending);
    }
    validate_manifest(output)
}

fn validate_manifest(source: String) -> io::Result<String> {
    if source.len() > MAX_MANIFEST_BYTES {
        return Err(invalid("Asset manifest exceeds 1 MiB"));
    }
    EiyashouAssetManifest::from_yaml(&source)
        .map_err(|error| invalid(format!("Invalid asset manifest update: {error}")))?;
    Ok(source)
}

fn validate_resource(path: &Path, kind: AssetKind, extension: &str) -> io::Result<()> {
    let size = fs::metadata(path)?.len();
    if size == 0 || size > MAX_MEDIA_BYTES {
        return Err(invalid("Resource size is invalid"));
    }
    match kind {
        AssetKind::Background | AssetKind::Figure => {
            if extension != "webp" {
                return Err(invalid("Images must already be WebP"));
            }
            ImageReader::with_format(BufReader::new(File::open(path)?), ImageFormat::WebP)
                .decode()
                .map_err(|_| invalid("Invalid WebP image"))?;
        }
        AssetKind::Voice | AssetKind::Bgm | AssetKind::Effect => {
            if !matches!(extension, "ogg" | "opus") {
                return Err(invalid("Audio must already be Ogg Opus"));
            }
            let bytes = read_probe(path)?;
            if !bytes.starts_with(b"OggS") || !contains_marker(&bytes, b"OpusHead") {
                return Err(invalid("Invalid Ogg Opus audio"));
            }
        }
        AssetKind::Video => {
            if !matches!(extension, "mp4" | "m4v") {
                return Err(invalid("Video must already be H.264 MP4"));
            }
            let bytes = read_probe(path)?;
            if bytes.len() < 12
                || &bytes[4..8] != b"ftyp"
                || !(contains_marker(&bytes, b"avc1") || contains_marker(&bytes, b"avc3"))
            {
                return Err(invalid("Invalid H.264 MP4 video"));
            }
        }
    }
    Ok(())
}

fn kind_for_path(path: &Path) -> Option<AssetKind> {
    path.parent()?.components().rev().find_map(|component| {
        let Component::Normal(value) = component else {
            return None;
        };
        match value.to_string_lossy().to_ascii_lowercase().as_str() {
            "background" | "backgrounds" | "bg" => Some(AssetKind::Background),
            "figure" | "figures" | "sprite" | "sprites" => Some(AssetKind::Figure),
            "voice" | "voices" | "vocal" => Some(AssetKind::Voice),
            "bgm" | "music" => Some(AssetKind::Bgm),
            "se" | "effect" | "effects" | "sfx" => Some(AssetKind::Effect),
            "video" | "videos" | "movie" | "movies" => Some(AssetKind::Video),
            _ => None,
        }
    })
}

fn namespace(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Background => "backgrounds",
        AssetKind::Figure => "figures",
        AssetKind::Voice => "voices",
        AssetKind::Bgm => "bgm",
        AssetKind::Effect => "se",
        AssetKind::Video => "videos",
    }
}

fn is_asset_namespace(value: &str) -> bool {
    matches!(
        value,
        "backgrounds" | "figures" | "voices" | "bgm" | "se" | "videos"
    )
}

fn identifier_from_filename(path: &Path) -> io::Result<String> {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| invalid("Resource filename is not valid UTF-8"))?;
    let mut id = String::with_capacity(stem.len());
    let mut separator = false;
    for character in stem.chars() {
        if character.is_alphanumeric() || character == '_' {
            if separator && !id.is_empty() && !id.ends_with('_') {
                id.push('_');
            }
            separator = false;
            id.push(character);
        } else {
            separator = true;
        }
    }
    let id = id.trim_matches('_').to_owned();
    if id.is_empty() {
        return Err(invalid("Resource filename cannot form an ID"));
    }
    let starts_valid = id
        .chars()
        .next()
        .is_some_and(|character| character == '_' || character.is_alphabetic());
    Ok(if starts_valid {
        id
    } else {
        format!("asset_{id}")
    })
}

fn child_path(parent: &Path, name: &str) -> io::Result<PathBuf> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.starts_with('.')
    {
        return Err(invalid("Invalid file name"));
    }
    let parent = checked_relative_or_root(parent)?;
    checked_relative(&parent.join(name))
}

fn checked_relative_or_root(path: &Path) -> io::Result<PathBuf> {
    if path.as_os_str().is_empty() {
        return Ok(PathBuf::new());
    }
    checked_relative(path)
}

fn checked_relative(path: &Path) -> io::Result<PathBuf> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(invalid("Path must remain inside the workspace"));
    }
    Ok(path.to_owned())
}

fn confined_existing(root: &Path, relative: &Path) -> io::Result<PathBuf> {
    let root = root.canonicalize()?;
    let candidate = root.join(relative).canonicalize()?;
    if !candidate.starts_with(&root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Path escapes workspace",
        ));
    }
    Ok(candidate)
}

fn confined_destination(root: &Path, relative: &Path) -> io::Result<PathBuf> {
    let root = root.canonicalize()?;
    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    let parent = if parent.as_os_str().is_empty() {
        root.clone()
    } else {
        root.join(parent).canonicalize()?
    };
    if !parent.starts_with(&root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Path escapes workspace",
        ));
    }
    Ok(parent.join(
        relative
            .file_name()
            .ok_or_else(|| invalid("Path has no name"))?,
    ))
}

fn copy_atomic(source: &Path, destination: &Path) -> io::Result<()> {
    let parent = destination
        .parent()
        .ok_or_else(|| invalid("Destination has no parent"))?;
    let name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("import");
    let nonce = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(".{name}.import-{}-{nonce}", std::process::id()));
    let result = (|| {
        let mut input = File::open(source)?;
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        io::copy(&mut input, &mut output)?;
        output.sync_all()?;
        fs::rename(&temporary, destination)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn read_probe(path: &Path) -> io::Result<Vec<u8>> {
    let file = File::open(path)?;
    let mut bytes = Vec::with_capacity(PROBE_BYTES);
    file.take(PROBE_BYTES as u64).read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn contains_marker(bytes: &[u8], marker: &[u8]) -> bool {
    bytes.windows(marker.len()).any(|window| window == marker)
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn is_media_extension(extension: &str) -> bool {
    matches!(
        extension,
        "webp"
            | "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "bmp"
            | "tif"
            | "tiff"
            | "ogg"
            | "opus"
            | "oga"
            | "wav"
            | "mp3"
            | "flac"
            | "aac"
            | "m4a"
            | "mp4"
            | "m4v"
            | "mov"
            | "webm"
            | "mkv"
    )
}

fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn yaml_single_quote(value: &str) -> String {
    value.replace('\'', "''")
}

fn parse_yaml_scalar(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.len() >= 2 && value.starts_with('\'') && value.ends_with('\'') {
        return Some(&value[1..value.len() - 1]);
    }
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        return Some(&value[1..value.len() - 1]);
    }
    (!value.is_empty()).then_some(value)
}

fn split_yaml_comment(value: &str) -> (&str, &str) {
    let mut quote = None;
    for (index, character) in value.char_indices() {
        match character {
            '\'' | '"' if quote == Some(character) => quote = None,
            '\'' | '"' if quote.is_none() => quote = Some(character),
            '#' if quote.is_none()
                && index > 0
                && value[..index].ends_with(char::is_whitespace) =>
            {
                return (value[..index].trim_end(), &value[index..]);
            }
            _ => {}
        }
    }
    (value.trim_end(), "")
}

fn line_ranges(source: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = 0;
    for (index, character) in source.char_indices() {
        if character == '\n' {
            ranges.push((start, index + 1));
            start = index + 1;
        }
    }
    if start < source.len() {
        ranges.push((start, source.len()));
    }
    ranges
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn fixture() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("keine-file-ops-{nonce}"));
        fs::create_dir_all(root.join("assets/background")).unwrap();
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::write(
            root.join("config.yaml"),
            "adapter:\n  script: keine\nscript:\n  version: 1\n  entry: opening\n  assets: assets.yaml\n  characters: characters.yaml\n",
        )
        .unwrap();
        fs::write(
            root.join("assets.yaml"),
            "# keep\nbackgrounds: {}\nfigures: {}\n",
        )
        .unwrap();
        fs::write(root.join("characters.yaml"), "characters: {}\n").unwrap();
        root
    }

    fn write_webp(path: &Path) {
        let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]));
        image.save_with_format(path, ImageFormat::WebP).unwrap();
    }

    #[test]
    fn canonical_image_import_registers_once() {
        let root = fixture();
        let source = root.parent().unwrap().join("morning.webp");
        write_webp(&source);
        let result = import_external(&root, Path::new("assets/background"), &source).unwrap();
        assert!(result.registered);
        assert!(root.join("assets/background/morning.webp").is_file());
        let manifest = fs::read_to_string(root.join("assets.yaml")).unwrap();
        assert!(manifest.contains("morning: 'assets/background/morning.webp'"));
        assert!(manifest.contains("# keep"));
        fs::remove_file(source).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn noncanonical_image_leaves_no_destination_or_mapping() {
        let root = fixture();
        let source = root.parent().unwrap().join("wrong.png");
        fs::write(&source, b"not a png").unwrap();
        let error = import_external(&root, Path::new("assets/background"), &source).unwrap_err();
        assert!(error.to_string().contains("WebP"));
        assert!(!root.join("assets/background/wrong.png").exists());
        assert!(
            !fs::read_to_string(root.join("assets.yaml"))
                .unwrap()
                .contains("wrong")
        );
        fs::remove_file(source).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mapped_rename_updates_manifest_and_preserves_comments() {
        let root = fixture();
        write_webp(&root.join("assets/background/old.webp"));
        fs::write(
            root.join("assets.yaml"),
            "# keep\nbackgrounds:\n  old: assets/background/old.webp # note\n",
        )
        .unwrap();
        let result =
            rename_entry(&root, Path::new("assets/background/old.webp"), "new.webp").unwrap();
        assert!(root.join("assets/background/new.webp").is_file());
        assert!(result.manifest_update.is_some());
        let source = fs::read_to_string(root.join("assets.yaml")).unwrap();
        assert!(source.contains("'assets/background/new.webp' # note"));
        assert!(source.contains("# keep"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mapped_delete_is_blocked() {
        let root = fixture();
        write_webp(&root.join("assets/background/day.webp"));
        fs::write(
            root.join("assets.yaml"),
            "backgrounds:\n  day: assets/background/day.webp\n",
        )
        .unwrap();
        let error = delete_entry(&root, Path::new("assets/background/day.webp")).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(root.join("assets/background/day.webp").is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mapped_rename_cannot_disguise_a_resource_format() {
        let root = fixture();
        write_webp(&root.join("assets/background/day.webp"));
        fs::write(
            root.join("assets.yaml"),
            "backgrounds:\n  day: assets/background/day.webp\n",
        )
        .unwrap();
        let error =
            rename_entry(&root, Path::new("assets/background/day.webp"), "day.png").unwrap_err();
        assert!(error.to_string().contains("WebP"));
        assert!(root.join("assets/background/day.webp").is_file());
        assert!(!root.join("assets/background/day.png").exists());
        assert!(
            fs::read_to_string(root.join("assets.yaml"))
                .unwrap()
                .contains("assets/background/day.webp")
        );
        fs::remove_dir_all(root).unwrap();
    }
}
