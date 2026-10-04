#[path = "assets.rs"]
mod assets;
pub use assets::*;
#[path = "media.rs"]
mod media;

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use image::{ImageFormat, ImageReader};
use keine_core::config::{EiyashouAssetEntry, EiyashouAssetManifest, GameConfig};

use crate::authoring::{AssetEntry, AssetKind, valid_identifier};
use crate::document::atomic_source;

const MAX_MEDIA_BYTES: u64 = 512 * 1024 * 1024;
const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
const PROBE_BYTES: usize = 1024 * 1024;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

// One manifest writer per physical project. Weak entries do not retain closed
// projects; the disk baseline check also catches changes from other programs.
fn manifest_writer(root: &Path) -> io::Result<Arc<Mutex<()>>> {
    static WRITERS: OnceLock<Mutex<HashMap<PathBuf, Weak<Mutex<()>>>>> = OnceLock::new();
    let key = root.canonicalize()?;
    let mut writers = WRITERS
        .get_or_init(Mutex::default)
        .lock()
        .map_err(|_| io::Error::other("manifest writer lock poisoned"))?;
    writers.retain(|_, writer| writer.strong_count() > 0);
    let writer = writers
        .get(&key)
        .and_then(Weak::upgrade)
        .unwrap_or_else(|| Arc::new(Mutex::new(())));
    writers.insert(key, Arc::downgrade(&writer));
    Ok(writer)
}

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
    let writer = manifest_writer(root)?;
    let _transaction = writer
        .lock()
        .map_err(|_| io::Error::other("manifest writer lock poisoned"))?;
    let metadata = fs::symlink_metadata(source)?;
    if !metadata.file_type().is_file() {
        return Err(invalid("Only files can be imported here"));
    }
    let file_name = source
        .file_name()
        .ok_or_else(|| invalid("The imported file has no name"))?;
    let mut relative = checked_relative(target_dir)?.join(file_name);
    let extension = extension(source);
    if is_media_extension(&extension) {
        let kind = kind_for_path(&relative).ok_or_else(|| {
            invalid("Drop media into Background, Figure, Voice, BGM, SE, or Video")
        })?;
        let output_extension = media::output_extension(source, kind)?;
        relative.set_extension(output_extension);
        let destination = confined_destination(root, &relative)?;
        if destination.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} already exists", relative.display()),
            ));
        }
        let id = identifier_from_filename(source)?;
        let (manifest_relative, old_manifest) = manifest_source(root)?;
        let manifest = EiyashouAssetManifest::from_yaml(&old_manifest)
            .map_err(|error| invalid(format!("Invalid asset manifest: {error}")))?;
        reject_manifest_conflict(&manifest, kind, &id, &relative)?;
        let new_manifest = insert_manifest_entry(&old_manifest, kind, &id, &relative)?;
        let manifest_path = root.join(&manifest_relative);

        media::import(source, &destination, kind)?;
        let commit = (|| {
            if fs::read_to_string(&manifest_path)? != old_manifest {
                return Err(io::Error::other(
                    "Asset manifest changed during import; import was cancelled",
                ));
            }
            atomic_source(&manifest_path, new_manifest.as_bytes())
        })();
        if let Err(error) = commit {
            if let Err(cleanup) = fs::remove_file(&destination) {
                return Err(io::Error::other(format!(
                    "Manifest update failed: {error}; imported file cleanup failed: {cleanup}; file retained at {}",
                    relative.display()
                )));
            }
            return Err(error);
        }
        return Ok(ImportResult {
            destination: relative,
            manifest_update: Some((manifest_relative, new_manifest)),
            registered: true,
        });
    }

    let destination = confined_destination(root, &relative)?;
    if destination.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", relative.display()),
        ));
    }
    copy_atomic(source, &destination)?;
    Ok(ImportResult {
        destination: relative,
        manifest_update: None,
        registered: false,
    })
}

/// Normalize a registered file beside its read-only source, keeping IDs and tags.
pub fn convert_asset(root: &Path, asset: &AssetEntry) -> io::Result<ImportResult> {
    let writer = manifest_writer(root)?;
    let _transaction = writer
        .lock()
        .map_err(|_| io::Error::other("manifest writer lock poisoned"))?;
    let source = confined_existing(root, &asset.path)?;
    let (manifest_relative, old_manifest) = manifest_source(root)?;
    let manifest = EiyashouAssetManifest::from_yaml(&old_manifest)
        .map_err(|error| invalid(error.to_string()))?;
    if entries_for_kind(&manifest, asset.kind)
        .get(&asset.id)
        .is_none_or(|entry| entry.path() != slash_path(&asset.path))
    {
        return Err(invalid("Asset mapping changed; refresh before converting"));
    }
    let mut relative = asset
        .path
        .with_extension(media::output_extension(&source, asset.kind)?);
    let stem = relative
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let extension = extension(&relative);
    let mut suffix = 0;
    while confined_destination(root, &relative)?.exists() {
        suffix += 1;
        relative.set_file_name(format!("{stem}-{suffix}.{extension}"));
    }
    let destination = confined_destination(root, &relative)?;
    let new_manifest =
        rewrite_manifest_paths(&old_manifest, &[(asset.path.clone(), relative.clone())])?;
    media::import(&source, &destination, asset.kind)?;
    let commit = (|| {
        if fs::read_to_string(root.join(&manifest_relative))? != old_manifest {
            return Err(invalid(
                "Asset manifest changed during conversion; conversion cancelled",
            ));
        }
        atomic_source(&root.join(&manifest_relative), new_manifest.as_bytes())
    })();
    if let Err(error) = commit {
        fs::remove_file(&destination).map_err(|cleanup| {
            io::Error::other(format!("{error}; output cleanup failed: {cleanup}"))
        })?;
        return Err(error);
    }
    Ok(ImportResult {
        destination: relative,
        manifest_update: Some((manifest_relative, new_manifest)),
        registered: true,
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

/// Relocate to an exact relative path, including manifest path rewrites.
pub fn relocate_entry(root: &Path, source: &Path, destination: &Path) -> io::Result<ImportResult> {
    move_or_rename(
        root,
        &checked_relative(source)?,
        &checked_relative(destination)?,
    )
}

pub fn asset_manifest_source(root: &Path) -> io::Result<(PathBuf, String)> {
    manifest_source(root)
}

pub fn replace_asset_manifest(
    root: &Path,
    relative: &Path,
    expected: &str,
    replacement: &str,
) -> io::Result<()> {
    let (actual_relative, current) = manifest_source(root)?;
    if actual_relative != relative || current != expected {
        return Err(invalid("Asset manifest changed; refresh before undoing"));
    }
    atomic_source(&root.join(relative), replacement.as_bytes())
}

pub fn ensure_unmapped_deletion(root: &Path, source: &Path) -> io::Result<()> {
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
    Ok(())
}

pub fn stash_entry(root: &Path, source: &Path) -> io::Result<PathBuf> {
    let source = checked_relative(source)?;
    let source_path = confined_existing(root, &source)?;
    let stash_dir = root.join(".keine/editor-undo");
    fs::create_dir_all(&stash_dir)?;
    let canonical_root = root.canonicalize()?;
    let canonical_stash = stash_dir.canonicalize()?;
    if !canonical_stash.starts_with(&canonical_root) {
        return Err(invalid("Undo storage escapes the workspace"));
    }
    let nonce = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let stash = PathBuf::from(format!(".keine/editor-undo/{}-{nonce}", std::process::id()));
    let stash_path = confined_destination(root, &stash)?;
    if stash_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Undo slot exists",
        ));
    }
    fs::rename(source_path, stash_path)?;
    Ok(stash)
}

pub fn restore_stashed_entry(root: &Path, stash: &Path, destination: &Path) -> io::Result<()> {
    let stash = checked_relative(stash)?;
    if !stash.starts_with(".keine/editor-undo") {
        return Err(invalid("Not an editor undo entry"));
    }
    let source_path = confined_existing(root, &stash)?;
    let destination = confined_destination(root, &checked_relative(destination)?)?;
    if destination.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Undo target exists",
        ));
    }
    fs::rename(source_path, destination)
}

pub fn restash_entry(root: &Path, source: &Path, stash: &Path) -> io::Result<()> {
    let source_path = confined_existing(root, &checked_relative(source)?)?;
    let stash = checked_relative(stash)?;
    if !stash.starts_with(".keine/editor-undo") {
        return Err(invalid("Not an editor undo entry"));
    }
    let stash_path = confined_destination(root, &stash)?;
    if stash_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Undo slot exists",
        ));
    }
    fs::rename(source_path, stash_path)
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
    let destination = confined_destination(root, &new_root)?;
    // Compare resolved paths: an in-project symlink can alias a child of the
    // source even when the two relative paths look unrelated.
    if destination.starts_with(&absolute) {
        return Err(invalid("A folder cannot be copied into itself"));
    }
    if destination.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", new_root.display()),
        ));
    }
    fs::create_dir(&destination)?;
    let manifest_backup = manifest_source(root).ok();
    let mut results = Vec::new();
    if let Err(error) = copy_directory_contents(root, &absolute, &new_root, &mut results) {
        if let Some((relative, source)) = manifest_backup
            && let Err(rollback) = atomic_source(&root.join(relative), source.as_bytes())
        {
            return Err(io::Error::other(format!(
                "Copy failed: {error}; manifest rollback failed: {rollback}; copied files retained at {}",
                new_root.display()
            )));
        }
        if let Err(cleanup) = fs::remove_dir_all(&destination) {
            return Err(io::Error::other(format!(
                "Copy failed: {error}; cleanup failed: {cleanup}; copied files retained at {}",
                new_root.display()
            )));
        }
        return Err(error);
    }
    Ok(results)
}

fn move_or_rename(root: &Path, source: &Path, destination: &Path) -> io::Result<ImportResult> {
    let source_path = confined_existing(root, source)?;
    let destination = checked_relative(destination)?;
    if destination.starts_with(source) {
        return Err(invalid("A folder cannot be moved into itself"));
    }
    let destination_path = confined_destination(root, &destination)?;
    if source_path.is_dir() && destination_path.starts_with(&source_path) {
        return Err(invalid("A folder cannot be moved into itself"));
    }
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
        if let Err(rollback) = fs::rename(&destination_path, &source_path) {
            return Err(io::Error::other(format!(
                "Manifest update failed: {error}; move rollback failed: {rollback}; file retained at {}",
                destination.display()
            )));
        }
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
    let config = fs::read_to_string(confined_existing(root, Path::new("config.yaml"))?)?;
    let config = GameConfig::from_yaml(&config)
        .map_err(|error| invalid(format!("Invalid config.yaml: {error}")))?;
    if config.adapter.script != "keine" {
        return Err(invalid("Asset import requires a native Kēne project"));
    }
    let relative = checked_relative(Path::new(&config.script.assets))?;
    let resolved = confined_existing(root, &relative)?;
    let manifest = root.join(&relative);
    if fs::symlink_metadata(&manifest)?.file_type().is_symlink() {
        return Err(invalid("Asset manifest cannot be a symbolic link"));
    }
    let source = fs::read_to_string(resolved)?;
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
        (AssetKind::Particle, manifest.particles),
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
        AssetKind::Particle => &manifest.particles,
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

pub fn validate_asset_type(root: &Path, relative: &Path, kind: AssetKind) -> io::Result<()> {
    let absolute = confined_existing(root, relative)?;
    let format_path = mapped_path(relative).unwrap_or_else(|| relative.to_owned());
    let ext = extension(&format_path);
    let valid = match kind {
        AssetKind::Background | AssetKind::Figure | AssetKind::Particle => {
            matches!(ext.as_str(), "webp" | "png" | "jpg" | "jpeg")
        }
        AssetKind::Voice | AssetKind::Bgm | AssetKind::Effect => {
            matches!(ext.as_str(), "ogg" | "opus" | "wav" | "mp3" | "flac")
        }
        AssetKind::Video => matches!(ext.as_str(), "mp4" | "m4v"),
    };
    if !valid {
        return Err(invalid("File format does not match the target type"));
    }
    if matches!(ext.as_str(), "webp" | "ogg" | "opus" | "mp4" | "m4v") {
        validate_resource(&absolute, kind, &ext)
    } else {
        let size = fs::metadata(absolute)?.len();
        if size == 0 || size > MAX_MEDIA_BYTES {
            Err(invalid("Invalid resource size"))
        } else {
            Ok(())
        }
    }
}

pub fn edit_manifest_asset(
    source: &str,
    asset: &AssetEntry,
    id: &str,
    kind: AssetKind,
    tags: &[String],
) -> io::Result<String> {
    if !valid_identifier(id) {
        return Err(invalid("Asset ID is not a script identifier"));
    }
    if tags
        .iter()
        .any(|tag| tag.trim().is_empty() || tag.contains(['\n', '\r']))
    {
        return Err(invalid("Invalid asset tag"));
    }
    let manifest = EiyashouAssetManifest::from_yaml(source)
        .map_err(|error| invalid(format!("Invalid asset manifest: {error}")))?;
    let current = entries_for_kind(&manifest, asset.kind)
        .get(&asset.id)
        .ok_or_else(|| invalid("Asset entry changed; refresh Inspector"))?;
    if current.path() != slash_path(&asset.path) || current.tags() != asset.tags {
        return Err(invalid("Asset entry changed; refresh Inspector"));
    }
    if entries_for_kind(&manifest, kind).contains_key(id) && (kind != asset.kind || id != asset.id)
    {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Asset ID already exists",
        ));
    }
    if kind != asset.kind
        && entries_for_kind(&manifest, kind)
            .values()
            .any(|entry| entry.path() == slash_path(&asset.path))
    {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Asset path already exists in target type",
        ));
    }
    let lines = line_ranges(source);
    let mut namespace_active = false;
    let mut start = None;
    let mut end = source.len();
    for (line_start, line_end) in &lines {
        let line = source[*line_start..*line_end].trim_end_matches(['\r', '\n']);
        let indent = line.len() - line.trim_start().len();
        if indent == 0 && !line.trim().is_empty() && !line.trim_start().starts_with('#') {
            if start.is_some() {
                end = *line_start;
                break;
            }
            namespace_active = line
                .split_once(':')
                .is_some_and(|(name, _)| name.trim() == namespace(asset.kind));
        } else if namespace_active && indent == 2 && !line.trim_start().starts_with('#') {
            if start.is_some() {
                end = *line_start;
                break;
            }
            if line
                .split_once(':')
                .is_some_and(|(name, _)| parse_yaml_scalar(name.trim()) == Some(asset.id.as_str()))
            {
                start = Some(*line_start);
            }
        }
    }
    let start = start.ok_or_else(|| invalid("Asset entry changed; refresh Inspector"))?;
    let old = &source[start..end];
    let mut comments = Vec::new();
    for line in old.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            comments.push(line.to_owned());
        } else if line.starts_with("    ")
            && !trimmed.is_empty()
            && !trimmed.starts_with("path:")
            && !trimmed.starts_with("tags:")
        {
            return Err(invalid("Asset entry has unsupported fields"));
        } else if let (_, comment) = split_yaml_comment(line)
            && !comment.is_empty()
        {
            comments.push(format!("  {comment}"));
        }
    }
    let mut replacement = if tags.is_empty() {
        format!(
            "  {id}: '{}'\n",
            yaml_single_quote(&slash_path(&asset.path))
        )
    } else {
        let tags = tags
            .iter()
            .map(|tag| format!("'{}'", yaml_single_quote(tag)))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "  {id}:\n    path: '{}'\n    tags: [{tags}]\n",
            yaml_single_quote(&slash_path(&asset.path))
        )
    };
    for comment in &comments {
        replacement.push_str(comment);
        replacement.push('\n');
    }
    let mut edited = source.to_owned();
    let moved_comments = comments
        .iter()
        .map(|comment| format!("{comment}\n"))
        .collect::<String>();
    edited.replace_range(
        start..end,
        if kind == asset.kind {
            &replacement
        } else {
            &moved_comments
        },
    );
    if kind != asset.kind {
        edited = restore_empty_namespace(edited, namespace(asset.kind));
        edited = insert_manifest_entry(&edited, kind, id, &asset.path)?;
        if !tags.is_empty() {
            let moved = AssetEntry {
                kind,
                id: id.to_owned(),
                path: asset.path.clone(),
                tags: Vec::new(),
                exists: asset.exists,
                reference_count: 0,
            };
            edited = edit_manifest_asset(&edited, &moved, id, kind, tags)?;
        }
    }
    validate_manifest(edited)
}

fn restore_empty_namespace(mut source: String, namespace: &str) -> String {
    let lines = line_ranges(&source);
    let Some((start, end)) = lines.iter().copied().find(|(start, end)| {
        source[*start..*end].trim_end_matches(['\r', '\n']).trim() == format!("{namespace}:")
    }) else {
        return source;
    };
    let has_entry = lines
        .iter()
        .copied()
        .skip_while(|(line_start, _)| *line_start <= start)
        .take_while(|(line_start, line_end)| {
            let line = source[*line_start..*line_end].trim();
            line.is_empty()
                || line.starts_with('#')
                || source[*line_start..*line_end].starts_with(char::is_whitespace)
        })
        .any(|(line_start, line_end)| {
            let line = source[line_start..line_end].trim_end_matches(['\r', '\n']);
            line.starts_with("  ")
                && !line.starts_with("    ")
                && !line.trim_start().starts_with('#')
        });
    if !has_entry {
        source.replace_range(start..end, &format!("{namespace}:\u{20}{{}}\n"));
    }
    source
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
        AssetKind::Background | AssetKind::Figure | AssetKind::Particle => {
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
            "particle" | "particles" => Some(AssetKind::Particle),
            _ => None,
        }
    })
}

pub fn unmapped_candidate_kind(path: &Path) -> Option<AssetKind> {
    let path = mapped_path(path)?;
    let kind = kind_for_path(&path)?;
    let ext = extension(&path);
    match kind {
        AssetKind::Background | AssetKind::Figure | AssetKind::Particle
            if matches!(ext.as_str(), "webp" | "png" | "jpg" | "jpeg") =>
        {
            Some(kind)
        }
        AssetKind::Voice | AssetKind::Bgm | AssetKind::Effect
            if matches!(ext.as_str(), "ogg" | "opus" | "wav" | "mp3" | "flac") =>
        {
            Some(kind)
        }
        AssetKind::Video if matches!(ext.as_str(), "mp4" | "m4v") => Some(kind),
        _ => None,
    }
}

fn namespace(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Background => "backgrounds",
        AssetKind::Figure => "figures",
        AssetKind::Voice => "voices",
        AssetKind::Bgm => "bgm",
        AssetKind::Effect => "se",
        AssetKind::Video => "videos",
        AssetKind::Particle => "particles",
    }
}

fn is_asset_namespace(value: &str) -> bool {
    matches!(
        value,
        "backgrounds" | "figures" | "voices" | "bgm" | "se" | "videos" | "particles"
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

pub(crate) fn confined_existing(root: &Path, relative: &Path) -> io::Result<PathBuf> {
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
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    fn fixture() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "keine-file-ops-{}-{nonce}-{}",
            std::process::id(),
            FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir_all(root.join("assets/background")).unwrap();
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::write(
            root.join("config.yaml"),
            "adapter:\n  script: keine\nscript:\n  version: 2\n  entry: opening\n  assets: assets.yaml\n  characters: characters.yaml\n",
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
    fn asset_property_edit_preserves_other_sections_and_rejects_collisions() {
        let source = "# keep\nbackgrounds:\n  room: assets/room.webp # note\n  second: assets/second.webp\nfigures: {}\n";
        let asset = AssetEntry {
            kind: AssetKind::Background,
            id: "room".into(),
            path: PathBuf::from("assets/room.webp"),
            tags: Vec::new(),
            exists: true,
            reference_count: 0,
        };
        let edited = edit_manifest_asset(
            source,
            &asset,
            "room_new",
            AssetKind::Background,
            &["interior".into()],
        )
        .unwrap();
        assert!(edited.contains("# keep"));
        assert!(edited.contains("# note"));
        assert!(edited.contains("  second: assets/second.webp"));
        let manifest = EiyashouAssetManifest::from_yaml(&edited).unwrap();
        assert!(manifest.backgrounds.contains_key("room_new"));
        assert_eq!(manifest.backgrounds["room_new"].tags(), &["interior"]);
        assert!(edit_manifest_asset(source, &asset, "second", AssetKind::Background, &[]).is_err());
    }

    #[test]
    fn asset_type_edit_moves_entry_without_dropping_tags() {
        let source = "backgrounds:\n  room: assets/room.webp\nfigures: {}\n";
        let asset = AssetEntry {
            kind: AssetKind::Background,
            id: "room".into(),
            path: PathBuf::from("assets/room.webp"),
            tags: Vec::new(),
            exists: true,
            reference_count: 0,
        };
        let edited =
            edit_manifest_asset(source, &asset, "room", AssetKind::Figure, &["hero".into()])
                .unwrap();
        let manifest = EiyashouAssetManifest::from_yaml(&edited).unwrap();
        assert!(!manifest.backgrounds.contains_key("room"));
        assert_eq!(manifest.figures["room"].tags(), &["hero"]);
    }

    #[test]
    fn convert_registered_image_preserves_original_ids_aliases_tags_and_collisions() {
        let root = fixture();
        let path = PathBuf::from("assets/background/room.png");
        let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([20, 40, 60, 80]));
        image.save(root.join(&path)).unwrap();
        let original = fs::read(root.join(&path)).unwrap();
        write_webp(&root.join("assets/background/room.webp"));
        let collision = fs::read(root.join("assets/background/room.webp")).unwrap();
        let before = "# keep\nbackgrounds:\n  room:\n    path: assets/background/room.png # source\n    tags: [day]\nfigures:\n  alias: assets/background/room.png\n";
        fs::write(root.join("assets.yaml"), before).unwrap();
        let asset = AssetEntry {
            kind: AssetKind::Background,
            id: "room".into(),
            path: path.clone(),
            tags: vec!["day".into()],
            exists: true,
            reference_count: 1,
        };
        let result = convert_asset(&root, &asset).unwrap();
        assert_eq!(
            result.destination,
            Path::new("assets/background/room-1.webp")
        );
        assert_eq!(fs::read(root.join(&path)).unwrap(), original);
        assert_eq!(
            fs::read(root.join("assets/background/room.webp")).unwrap(),
            collision
        );
        let after = fs::read_to_string(root.join("assets.yaml")).unwrap();
        let manifest = EiyashouAssetManifest::from_yaml(&after).unwrap();
        assert_eq!(
            manifest.backgrounds["room"].path(),
            "assets/background/room-1.webp"
        );
        assert_eq!(manifest.backgrounds["room"].tags(), ["day"]);
        assert_eq!(
            manifest.figures["alias"].path(),
            manifest.backgrounds["room"].path()
        );
        assert!(after.contains("# keep") && after.contains("# source"));
        let decoded = image::open(root.join(result.destination))
            .unwrap()
            .to_rgba8();
        assert_eq!(decoded.dimensions(), image.dimensions());
        assert!(decoded.pixels().zip(image.pixels()).all(|(a, b)| {
            a[3] == b[3]
                && a.0[..3]
                    .iter()
                    .zip(&b.0[..3])
                    .all(|(a, b)| a.abs_diff(*b) <= 8)
        }));
        assert!(convert_asset(&root, &asset).is_err());
        fs::write(root.join("assets.yaml"), before).unwrap();
        fs::write(root.join(&path), b"broken source").unwrap();
        assert!(convert_asset(&root, &asset).is_err());
        assert_eq!(
            fs::read_to_string(root.join("assets.yaml")).unwrap(),
            before
        );
        assert!(!root.join("assets/background/room-2.webp").exists());
        fs::remove_dir_all(root).unwrap();
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
    fn concurrent_imports_keep_both_manifest_entries() {
        let root = fixture();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let handles = ["first.webp", "second.webp"].map(|name| {
            let source = root.join(name);
            write_webp(&source);
            let root = root.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                import_external(&root, Path::new("assets/background"), &source).unwrap()
            })
        });
        for handle in handles {
            assert!(handle.join().unwrap().registered);
        }
        let manifest = EiyashouAssetManifest::from_yaml(
            &fs::read_to_string(root.join("assets.yaml")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest.backgrounds.len(), 2);
        assert!(manifest.backgrounds.contains_key("first"));
        assert!(manifest.backgrounds.contains_key("second"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn png_import_in_its_original_folder_preserves_source_and_alpha() {
        use image::{ImageDecoder, ImageEncoder};
        let root = fixture();
        let source = root.join("assets/background/portrait.png");
        let image = image::RgbaImage::from_raw(
            2,
            2,
            vec![240, 80, 30, 255, 10, 40, 90, 128, 90, 10, 40, 0, 1, 2, 3, 1],
        )
        .unwrap();
        let profile = b"source ICC profile".to_vec();
        let mut encoder = image::codecs::png::PngEncoder::new(File::create(&source).unwrap());
        encoder.set_icc_profile(profile.clone()).unwrap();
        encoder
            .write_image(image.as_raw(), 2, 2, image::ExtendedColorType::Rgba8)
            .unwrap();
        let before = fs::read(&source).unwrap();
        let original_permissions = fs::metadata(&source).unwrap().permissions();
        let mut readonly = original_permissions.clone();
        readonly.set_readonly(true);
        fs::set_permissions(&source, readonly).unwrap();
        let result = import_external(&root, Path::new("assets/background"), &source).unwrap();
        assert_eq!(
            result.destination,
            Path::new("assets/background/portrait.webp")
        );
        let destination = root.join(&result.destination);
        let decoded = image::open(&destination).unwrap().into_rgba8();
        assert_eq!(decoded.dimensions(), image.dimensions());
        assert_eq!(
            decoded.pixels().map(|p| p[3]).collect::<Vec<_>>(),
            image.pixels().map(|p| p[3]).collect::<Vec<_>>()
        );
        let mut decoder = ImageReader::open(&destination)
            .unwrap()
            .with_guessed_format()
            .unwrap()
            .into_decoder()
            .unwrap();
        assert_eq!(decoder.icc_profile().unwrap(), Some(profile));
        assert!(
            fs::read(&destination)
                .unwrap()
                .windows(4)
                .any(|tag| tag == b"VP8 ")
        );
        // The same PNG remains pixel-exact when imported as a sprite/particle.
        for kind in [AssetKind::Figure, AssetKind::Particle] {
            let target = root.join(format!("{}.webp", kind.label()));
            media::import(&source, &target, kind).unwrap();
            assert_eq!(image::open(target).unwrap().into_rgba8(), image);
        }
        assert_eq!(fs::read(&source).unwrap(), before);
        assert!(fs::metadata(&source).unwrap().permissions().readonly());
        let manifest = EiyashouAssetManifest::from_yaml(
            &fs::read_to_string(root.join("assets.yaml")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            manifest.backgrounds["portrait"].path(),
            "assets/background/portrait.webp"
        );
        // A second conversion cannot replace the existing output or change the source.
        assert_eq!(
            import_external(&root, Path::new("assets/background"), &source)
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&source).unwrap(), before);
        fs::set_permissions(source, original_permissions).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn jpeg_background_import_applies_orientation_before_encoding() {
        use image::ImageEncoder;
        let root = fixture();
        let source = root.join("photo.jpg");
        let image = image::RgbImage::from_pixel(3, 2, image::Rgb([200, 90, 30]));
        let mut encoder =
            image::codecs::jpeg::JpegEncoder::new_with_quality(File::create(&source).unwrap(), 95);
        // TIFF IFD0 Orientation = 6 (90 degrees clockwise).
        encoder
            .set_exif_metadata(vec![
                b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0,
                0, 0, 0,
            ])
            .unwrap();
        encoder.encode_image(&image).unwrap();
        let before = fs::read(&source).unwrap();
        let mut expected = image::open(&source).unwrap();
        expected.apply_orientation(image::metadata::Orientation::Rotate90);
        let result = import_external(&root, Path::new("assets/background"), &source).unwrap();
        let decoded = image::open(root.join(result.destination))
            .unwrap()
            .into_rgba8();
        let expected = expected.into_rgba8();
        assert_eq!(decoded.dimensions(), expected.dimensions());
        assert!(
            decoded
                .iter()
                .zip(expected.iter())
                .all(|(a, b)| a.abs_diff(*b) <= 8)
        );
        assert_eq!(fs::read(source).unwrap(), before);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn animated_image_import_does_not_discard_frames_or_leave_output() {
        let root = fixture();
        let source = root.join("animation.gif");
        let frame = image::Frame::new(image::RgbaImage::from_pixel(
            2,
            2,
            image::Rgba([1, 2, 3, 255]),
        ));
        let mut encoder = image::codecs::gif::GifEncoder::new(File::create(&source).unwrap());
        encoder.encode_frames([frame.clone(), frame]).unwrap();
        drop(encoder);
        let before = fs::read(&source).unwrap();
        let manifest = fs::read_to_string(root.join("assets.yaml")).unwrap();
        assert!(
            import_external(&root, Path::new("assets/background"), &source)
                .unwrap_err()
                .to_string()
                .contains("Animated")
        );
        assert_eq!(fs::read(&source).unwrap(), before);
        assert_eq!(
            fs::read_to_string(root.join("assets.yaml")).unwrap(),
            manifest
        );
        assert_eq!(
            fs::read_dir(root.join("assets/background"))
                .unwrap()
                .count(),
            0
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "requires FFmpeg with libopus and audio fixture encoders"]
    fn audio_import_converts_real_formats_and_preserves_duration_and_source() {
        use std::process::{Command, Stdio};
        let root = fixture();
        fs::create_dir_all(root.join("assets/bgm")).unwrap();
        let ffmpeg = media::ffmpeg_executable().unwrap();
        for (extension, codec) in [
            ("wav", "pcm_s16le"),
            ("flac", "flac"),
            ("mp3", "libmp3lame"),
            ("ogg", "vorbis"),
        ] {
            let source = root.join(format!("tone_{extension}.{extension}"));
            assert!(
                Command::new(&ffmpeg)
                    .args([
                        "-nostdin",
                        "-loglevel",
                        "error",
                        "-f",
                        "lavfi",
                        "-i",
                        "sine=frequency=440:sample_rate=48000:duration=2",
                        "-ac",
                        "2",
                        "-c:a",
                        codec,
                        "-strict",
                        "experimental"
                    ])
                    .arg(&source)
                    .status()
                    .unwrap()
                    .success()
            );
            let before = fs::read(&source).unwrap();
            let imported = import_external(&root, Path::new("assets/bgm"), &source).unwrap();
            assert_eq!(imported.destination.extension().unwrap(), "opus");
            let converted = root.join(&imported.destination);
            let duration = asset_media_info(&root, &imported.destination)
                .unwrap()
                .duration
                .unwrap();
            assert!((duration - 2.0).abs() < 0.025, "{extension}: {duration}");
            let decode = |path: &Path| {
                let output = Command::new(&ffmpeg)
                    .args(["-nostdin", "-loglevel", "error", "-i"])
                    .arg(path)
                    .args([
                        "-map", "0:a:0", "-ar", "48000", "-ac", "1", "-f", "s16le", "pipe:1",
                    ])
                    .stdin(Stdio::null())
                    .output()
                    .unwrap();
                assert!(output.status.success());
                output.stdout
            };
            assert_eq!(
                decode(&source).len(),
                decode(&converted).len(),
                "encoder padding changed duration: {extension}"
            );
            assert_eq!(fs::read(&source).unwrap(), before);
        }
        // An already normalized stream is copied byte-for-byte, without another lossy encode.
        fs::create_dir_all(root.join("assets/voices")).unwrap();
        let canonical = root.join("assets/bgm/tone_wav.opus");
        let copy = import_external(&root, Path::new("assets/voices"), &canonical).unwrap();
        assert_eq!(
            fs::read(&canonical).unwrap(),
            fs::read(root.join(copy.destination)).unwrap()
        );
        let ogg = root.join("copy.ogg");
        fs::copy(&canonical, &ogg).unwrap();
        assert_eq!(
            media::output_extension(&ogg, AssetKind::Bgm).unwrap(),
            "opus"
        );
        let manifest = fs::read_to_string(root.join("assets.yaml")).unwrap();
        let corrupt = root.join("corrupt.wav");
        fs::write(&corrupt, b"invalid audio").unwrap();
        assert!(import_external(&root, Path::new("assets/bgm"), &corrupt).is_err());
        assert_eq!(fs::read(&corrupt).unwrap(), b"invalid audio");
        assert_eq!(
            fs::read_to_string(root.join("assets.yaml")).unwrap(),
            manifest
        );
        assert!(fs::read_dir(root.join("assets/bgm")).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with('.')
        }));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "requires FFmpeg with libx264"]
    fn video_import_converts_mov_to_decodable_h264_mp4_without_changing_source() {
        use std::process::{Command, Stdio};
        let root = fixture();
        fs::create_dir_all(root.join("assets/videos")).unwrap();
        let ffmpeg = media::ffmpeg_executable().unwrap();
        let source = root.join("movie.mov");
        assert!(
            Command::new(&ffmpeg)
                .args([
                    "-nostdin",
                    "-loglevel",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=128x72:rate=10:duration=0.3",
                    "-c:v",
                    "mpeg4"
                ])
                .arg(&source)
                .status()
                .unwrap()
                .success()
        );
        let before = fs::read(&source).unwrap();
        let imported = import_external(&root, Path::new("assets/videos"), &source).unwrap();
        assert_eq!(imported.destination, Path::new("assets/videos/movie.mp4"));
        let converted = root.join(imported.destination);
        assert!(contains_marker(&fs::read(&converted).unwrap(), b"avc1"));
        assert!(
            Command::new(&ffmpeg)
                .args(["-nostdin", "-loglevel", "error", "-i"])
                .arg(converted)
                .args(["-f", "null", "-"])
                .stdout(Stdio::null())
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(fs::read(source).unwrap(), before);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn noncanonical_image_leaves_no_destination_or_mapping() {
        let root = fixture();
        let source = root.parent().unwrap().join("wrong.png");
        fs::write(&source, b"not a png").unwrap();
        let error = import_external(&root, Path::new("assets/background"), &source).unwrap_err();
        assert!(error.to_string().contains("image"));
        assert!(!root.join("assets/background/wrong.png").exists());
        assert!(!root.join("assets/background/wrong.webp").exists());
        assert!(
            !fs::read_to_string(root.join("assets.yaml"))
                .unwrap()
                .contains("wrong")
        );
        fs::remove_file(source).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn folder_copy_rejects_its_own_descendant() {
        let root = fixture();
        fs::create_dir(root.join("assets/background/nested")).unwrap();
        let result = copy_entry(
            &root,
            Path::new("assets/background"),
            Path::new("assets/background/nested"),
        );
        assert!(result.is_err());
        assert!(!root.join("assets/background/nested/background").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_folder_copy_restores_manifest_before_removing_files() {
        let root = fixture();
        fs::create_dir(root.join("incoming")).unwrap();
        write_webp(&root.join("incoming/a.webp"));
        fs::write(root.join("incoming/z.png"), b"invalid image").unwrap();
        let original_manifest = fs::read_to_string(root.join("assets.yaml")).unwrap();

        assert!(copy_entry(&root, Path::new("incoming"), Path::new("assets/background")).is_err());
        assert_eq!(
            fs::read_to_string(root.join("assets.yaml")).unwrap(),
            original_manifest
        );
        assert!(!root.join("assets/background/incoming").exists());
        assert!(root.join("incoming/a.webp").is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn folder_copy_rejects_an_outside_symlink_target() {
        use std::os::unix::fs::symlink;

        let root = fixture();
        let outside = root.with_extension("outside");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, root.join("outside-link")).unwrap();
        let result = copy_entry(&root, Path::new("scripts"), Path::new("outside-link"));
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert!(!outside.join("scripts").exists());
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn manifest_source_rejects_symlinks_outside_the_project() {
        use std::os::unix::fs::symlink;

        let root = fixture();
        let outside = root.with_extension("manifest.yaml");
        fs::write(&outside, "backgrounds: {}\n").unwrap();
        fs::remove_file(root.join("assets.yaml")).unwrap();
        symlink(&outside, root.join("assets.yaml")).unwrap();
        assert_eq!(
            manifest_source(&root).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(fs::read_to_string(&outside).unwrap(), "backgrounds: {}\n");
        fs::remove_dir_all(root).unwrap();
        fs::remove_file(outside).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn manifest_source_rejects_an_in_project_file_symlink() {
        use std::os::unix::fs::symlink;

        let root = fixture();
        fs::rename(root.join("assets.yaml"), root.join("source.yaml")).unwrap();
        symlink("source.yaml", root.join("assets.yaml")).unwrap();
        assert_eq!(
            manifest_source(&root).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn folder_copy_rejects_an_in_project_alias_of_its_descendant() {
        use std::os::unix::fs::symlink;

        let root = fixture();
        fs::create_dir(root.join("assets/background/nested")).unwrap();
        symlink("assets/background/nested", root.join("nested-link")).unwrap();
        let result = copy_entry(
            &root,
            Path::new("assets/background"),
            Path::new("nested-link"),
        );
        assert!(result.is_err());
        assert!(!root.join("assets/background/nested/background").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn folder_move_rejects_an_in_project_alias_of_its_descendant() {
        use std::os::unix::fs::symlink;

        let root = fixture();
        fs::create_dir(root.join("assets/background/nested")).unwrap();
        symlink("assets/background/nested", root.join("nested-link")).unwrap();
        let result = move_entry(
            &root,
            Path::new("assets/background"),
            Path::new("nested-link"),
        );
        assert!(result.is_err());
        assert!(root.join("assets/background").is_dir());
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
        let error =
            ensure_unmapped_deletion(&root, Path::new("assets/background/day.webp")).unwrap_err();
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

    #[test]
    fn deleted_file_can_be_restored_and_redone_without_overwrite() {
        let root = fixture();
        fs::write(root.join("notes.md"), "keep me").unwrap();
        let stash = stash_entry(&root, Path::new("notes.md")).unwrap();
        assert!(!root.join("notes.md").exists());
        assert_eq!(fs::read_to_string(root.join(&stash)).unwrap(), "keep me");
        fs::write(root.join("notes.md"), "new file").unwrap();
        assert!(restore_stashed_entry(&root, &stash, Path::new("notes.md")).is_err());
        assert_eq!(
            fs::read_to_string(root.join("notes.md")).unwrap(),
            "new file"
        );
        fs::remove_file(root.join("notes.md")).unwrap();
        restore_stashed_entry(&root, &stash, Path::new("notes.md")).unwrap();
        restash_entry(&root, Path::new("notes.md"), &stash).unwrap();
        restore_stashed_entry(&root, &stash, Path::new("notes.md")).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("notes.md")).unwrap(),
            "keep me"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
