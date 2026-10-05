//! Project resources, references, dialogue and scene indexes derived from source.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};

use keine_core::config::{
    EiyashouAssetEntry, EiyashouAssetManifest, EiyashouCharacterManifest, GameConfig,
};
use keine_core::{Action, EiyashouTextPart};
use keine_loader::{
    DiagnosticLevel, NativeTokenKind, ResourceKind, parse_native_document, parse_native_scenes,
};

use crate::projection::{BlockKind, EiyashouProjection};
use crate::workspace::WorkspaceFile;

const MAX_INDEXED_SOURCE_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AssetKind {
    Background,
    Figure,
    Voice,
    Bgm,
    Effect,
    Video,
    Particle,
}

impl AssetKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Background => "Background",
            Self::Figure => "Figure",
            Self::Voice => "Voice",
            Self::Bgm => "BGM",
            Self::Effect => "Effect",
            Self::Video => "Video",
            Self::Particle => "Particle",
        }
    }

    fn from_resource(kind: ResourceKind) -> Option<Self> {
        match kind {
            ResourceKind::Background => Some(Self::Background),
            ResourceKind::Figure | ResourceKind::MiniAvatar => Some(Self::Figure),
            ResourceKind::Voice => Some(Self::Voice),
            ResourceKind::Bgm => Some(Self::Bgm),
            ResourceKind::Effect => Some(Self::Effect),
            ResourceKind::Video => Some(Self::Video),
            ResourceKind::Particle => Some(Self::Particle),
            ResourceKind::Lut => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetEntry {
    pub kind: AssetKind,
    pub id: String,
    pub path: PathBuf,
    pub tags: Vec<String>,
    pub exists: bool,
    pub reference_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetReference {
    pub key: AssetKey,
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub range: Option<Range<usize>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnmappedAsset {
    pub kind: AssetKind,
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AssetKey {
    pub kind: AssetKind,
    pub id: String,
}

impl AssetEntry {
    pub fn canonical_format(
        &self,
        media: Option<&crate::file_ops::AssetMediaInfo>,
    ) -> Option<&'static str> {
        let format = media?.canonical_format?;
        match (self.kind, format) {
            (AssetKind::Background | AssetKind::Figure | AssetKind::Particle, "WebP") => {
                Some(format)
            }
            (AssetKind::Voice | AssetKind::Bgm | AssetKind::Effect, "Opus")
                if self
                    .path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("opus")) =>
            {
                Some(format)
            }
            (AssetKind::Video, "H.264 MP4")
                if self
                    .path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("mp4")) =>
            {
                Some(format)
            }
            _ => None,
        }
    }

    pub fn key(&self) -> AssetKey {
        AssetKey {
            kind: self.kind,
            id: self.id.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AssetSort {
    #[default]
    Name,
    Path,
    References,
    Modified,
    Size,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AssetQuery {
    pub search: String,
    pub kind: Option<AssetKind>,
    pub folder: Option<PathBuf>,
    pub sort: AssetSort,
    pub tag: Option<String>,
    pub status: AssetStatus,
    pub size: AssetSize,
    pub modified: Option<std::time::Duration>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AssetStatus {
    #[default]
    All,
    Used,
    Unused,
    Missing,
    Canonical,
    NeedsConversion,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AssetSize {
    #[default]
    All,
    Small,
    Medium,
    Large,
}

fn search_matches(search: &str, fields: &[String]) -> bool {
    search.split_whitespace().all(|token| {
        fields.iter().any(|field| {
            if field.contains(token) {
                return true;
            }
            let mut remaining = token.chars();
            let mut next = remaining.next();
            for character in field.chars() {
                if next == Some(character) {
                    next = remaining.next();
                }
                if next.is_none() {
                    return true;
                }
            }
            false
        })
    })
}

impl AssetQuery {
    pub fn results<'a>(&self, assets: &'a [AssetEntry]) -> Vec<&'a AssetEntry> {
        self.results_with_media(assets, &BTreeMap::new())
    }

    pub fn results_with_media<'a>(
        &self,
        assets: &'a [AssetEntry],
        media: &BTreeMap<PathBuf, crate::file_ops::AssetMediaInfo>,
    ) -> Vec<&'a AssetEntry> {
        let search = self.search.trim().to_lowercase();
        let mut results = assets
            .iter()
            .filter(|asset| self.kind.is_none_or(|kind| asset.kind == kind))
            .filter(|asset| {
                self.folder
                    .as_ref()
                    .is_none_or(|folder| asset.path.starts_with(folder))
            })
            .filter(|asset| self.tag.as_ref().is_none_or(|tag| asset.tags.contains(tag)))
            .filter(|asset| match self.status {
                AssetStatus::All => true,
                AssetStatus::Used => asset.reference_count > 0,
                AssetStatus::Unused => asset.reference_count == 0,
                AssetStatus::Missing => !asset.exists,
                AssetStatus::Canonical => {
                    asset.exists && asset.canonical_format(media.get(&asset.path)).is_some()
                }
                AssetStatus::NeedsConversion => {
                    asset.exists && asset.canonical_format(media.get(&asset.path)).is_none()
                }
            })
            .filter(|asset| {
                let info = media.get(&asset.path);
                let size = match self.size {
                    AssetSize::All => true,
                    AssetSize::Small => info.is_some_and(|info| info.bytes < 1024 * 1024),
                    AssetSize::Medium => info
                        .is_some_and(|info| (1024 * 1024..10 * 1024 * 1024).contains(&info.bytes)),
                    AssetSize::Large => info.is_some_and(|info| info.bytes >= 10 * 1024 * 1024),
                };
                size && self.modified.is_none_or(|limit| {
                    info.and_then(|info| info.modified)
                        .and_then(|time| time.elapsed().ok())
                        .is_some_and(|age| age <= limit)
                })
            })
            .filter(|asset| {
                if search.is_empty() {
                    return true;
                }
                let mut fields = vec![
                    asset.id.to_lowercase(),
                    asset.path.to_string_lossy().to_lowercase(),
                ];
                fields.extend(asset.tags.iter().map(|tag| tag.to_lowercase()));
                search_matches(&search, &fields)
            })
            .collect::<Vec<_>>();
        results.sort_by(|left, right| {
            let primary = match self.sort {
                AssetSort::Name => left.id.cmp(&right.id),
                AssetSort::Path => left.path.cmp(&right.path),
                AssetSort::References => right.reference_count.cmp(&left.reference_count),
                AssetSort::Size => media
                    .get(&right.path)
                    .map(|info| info.bytes)
                    .cmp(&media.get(&left.path).map(|info| info.bytes)),
                AssetSort::Modified => media
                    .get(&right.path)
                    .and_then(|info| info.modified)
                    .cmp(&media.get(&left.path).and_then(|info| info.modified)),
            };
            primary
                .then_with(|| left.kind.cmp(&right.kind))
                .then_with(|| left.id.cmp(&right.id))
                .then_with(|| left.path.cmp(&right.path))
        });
        results
    }

    pub fn unmapped_results<'a>(&self, assets: &'a [UnmappedAsset]) -> Vec<&'a UnmappedAsset> {
        self.unmapped_results_with_media(assets, &BTreeMap::new())
    }

    pub fn unmapped_results_with_media<'a>(
        &self,
        assets: &'a [UnmappedAsset],
        media: &BTreeMap<PathBuf, crate::file_ops::AssetMediaInfo>,
    ) -> Vec<&'a UnmappedAsset> {
        let entries = assets
            .iter()
            .map(|asset| AssetEntry {
                kind: asset.kind,
                id: asset
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into(),
                path: asset.path.clone(),
                tags: Vec::new(),
                exists: true,
                reference_count: 0,
            })
            .collect::<Vec<_>>();
        let by_path = assets
            .iter()
            .map(|asset| (&asset.path, asset))
            .collect::<BTreeMap<_, _>>();
        self.results_with_media(&entries, media)
            .into_iter()
            .filter_map(|entry| by_path.get(&entry.path).copied())
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct CharacterEntry {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
    pub avatar: Option<String>,
    pub expressions: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneEntry {
    pub path: PathBuf,
    pub name: String,
    pub line: usize,
    pub name_range: Range<usize>,
    pub source_range: Range<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogueEntry {
    pub path: PathBuf,
    pub scene: String,
    pub speaker: String,
    pub text: String,
    pub editable: bool,
    pub line: usize,
    pub column: usize,
    pub source_range: Range<usize>,
    pub text_range: Range<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProblemSeverity {
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthoringProblem {
    pub severity: ProblemSeverity,
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub message: String,
}

#[derive(Clone, Debug, Default)]
pub struct AuthoringIndex {
    pub native: bool,
    pub entry_scene: String,
    pub assets_manifest: Option<PathBuf>,
    pub characters_manifest: Option<PathBuf>,
    pub assets: Vec<AssetEntry>,
    pub media: BTreeMap<PathBuf, crate::file_ops::AssetMediaInfo>,
    pub asset_references: Vec<AssetReference>,
    pub unindexed_sources: Vec<PathBuf>,
    pub unmapped: Vec<UnmappedAsset>,
    pub characters: Vec<CharacterEntry>,
    pub scenes: Vec<SceneEntry>,
    pub dialogues: Vec<DialogueEntry>,
    pub variables: Vec<(PathBuf, String)>,
    pub problems: Vec<AuthoringProblem>,
}

impl AuthoringIndex {
    /// Reorder sibling scripts only; directory subtrees and other files keep
    /// their positions. The entry scene wins, then scene names use numeric order.
    pub(crate) fn order_script_files(&self, files: &mut [WorkspaceFile]) {
        let mut ranks = BTreeMap::<&Path, (bool, &str)>::new();
        for scene in &self.scenes {
            let rank = (scene.name != self.entry_scene, scene.name.as_str());
            let old = ranks.entry(&scene.path).or_insert(rank);
            if !rank.0 && old.0 || rank.0 == old.0 && natural_cmp(rank.1, old.1).is_lt() {
                *old = rank;
            }
        }
        let mut siblings = BTreeMap::<PathBuf, Vec<usize>>::new();
        for (position, file) in files.iter().enumerate() {
            if !file.is_dir()
                && file
                    .relative_path
                    .extension()
                    .is_some_and(|ext| ext == "shou")
            {
                siblings
                    .entry(
                        file.relative_path
                            .parent()
                            .unwrap_or(Path::new(""))
                            .to_owned(),
                    )
                    .or_default()
                    .push(position);
            }
        }
        for positions in siblings.values() {
            let mut scripts = positions
                .iter()
                .map(|&position| files[position].clone())
                .collect::<Vec<_>>();
            scripts.sort_by(|left, right| {
                let left_name = left.relative_path.to_string_lossy();
                let right_name = right.relative_path.to_string_lossy();
                let left_rank = ranks
                    .get(left.relative_path.as_path())
                    .copied()
                    .unwrap_or((true, &left_name));
                let right_rank = ranks
                    .get(right.relative_path.as_path())
                    .copied()
                    .unwrap_or((true, &right_name));
                left_rank
                    .0
                    .cmp(&right_rank.0)
                    .then_with(|| natural_cmp(left_rank.1, right_rank.1))
                    .then_with(|| left.relative_path.cmp(&right.relative_path))
            });
            for (&position, script) in positions.iter().zip(scripts) {
                files[position] = script;
            }
        }
    }

    /// Replace only changed script contributions; config/manifests invalidate
    /// the project namespace and use a full load instead.
    pub fn with_sources(&self, sources: &BTreeMap<PathBuf, String>) -> Self {
        self.with_sources_cancellable(sources, || false).unwrap()
    }

    pub(crate) fn with_sources_cancellable(
        &self,
        sources: &BTreeMap<PathBuf, String>,
        cancelled: impl Fn() -> bool,
    ) -> Option<Self> {
        if cancelled() {
            return None;
        }
        // Copy only surviving script contributions. The public Vec-based index
        // stays stable; replaced dialogue bodies are never cloned and discarded.
        let mut index = Self {
            native: self.native,
            entry_scene: self.entry_scene.clone(),
            assets_manifest: self.assets_manifest.clone(),
            characters_manifest: self.characters_manifest.clone(),
            assets: self.assets.clone(),
            media: self.media.clone(),
            unmapped: self.unmapped.clone(),
            characters: self.characters.clone(),
            scenes: self
                .scenes
                .iter()
                .filter(|entry| !sources.contains_key(&entry.path))
                .cloned()
                .collect(),
            dialogues: self
                .dialogues
                .iter()
                .filter(|entry| !sources.contains_key(&entry.path))
                .cloned()
                .collect(),
            variables: self
                .variables
                .iter()
                .filter(|(path, _)| !sources.contains_key(path))
                .cloned()
                .collect(),
            asset_references: self
                .asset_references
                .iter()
                .filter(|entry| !sources.contains_key(&entry.path))
                .cloned()
                .collect(),
            problems: self
                .problems
                .iter()
                .filter(|entry| !sources.contains_key(&entry.path))
                .cloned()
                .collect(),
            unindexed_sources: self
                .unindexed_sources
                .iter()
                .filter(|path| !sources.contains_key(*path))
                .cloned()
                .collect(),
        };
        let lookup = index
            .assets
            .iter()
            .map(|asset| (asset.kind, asset.id.clone()))
            .collect::<HashSet<_>>();
        for (path, source) in sources {
            if cancelled() {
                return None;
            }
            index_source(path, source, &mut index, &mut HashMap::new(), &lookup);
            if index
                .problems
                .iter()
                .any(|problem| &problem.path == path && problem.severity == ProblemSeverity::Error)
            {
                index.unindexed_sources.push(path.clone());
            }
        }
        let mut counts = HashMap::<AssetKey, usize>::new();
        for reference in &index.asset_references {
            *counts.entry(reference.key.clone()).or_default() += 1;
        }
        for asset in &mut index.assets {
            asset.reference_count = counts.get(&asset.key()).copied().unwrap_or(0);
        }
        index.scenes.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then_with(|| a.source_range.start.cmp(&b.source_range.start))
        });
        index.dialogues.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then_with(|| a.source_range.start.cmp(&b.source_range.start))
        });
        index.variables.sort();
        index.variables.dedup();
        index.problems.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(a.line.cmp(&b.line))
                .then(a.column.cmp(&b.column))
                .then(a.message.cmp(&b.message))
        });
        index.problems.dedup();
        if cancelled() { None } else { Some(index) }
    }

    pub fn load(
        root: &Path,
        files: &[WorkspaceFile],
        overrides: &BTreeMap<PathBuf, String>,
    ) -> Self {
        let mut index = Self::default();
        let config_path = PathBuf::from("config.yaml");
        let Some(config_source) = read_text(root, &config_path, overrides) else {
            index.problems.push(problem(
                ProblemSeverity::Error,
                config_path,
                1,
                1,
                "config.yaml is missing or is not UTF-8",
            ));
            return index;
        };
        let config = match GameConfig::from_yaml(&config_source) {
            Ok(config) => config,
            Err(error) => {
                index.problems.push(problem(
                    ProblemSeverity::Error,
                    config_path,
                    1,
                    1,
                    format!("Invalid project configuration: {error}"),
                ));
                return index;
            }
        };
        if config.adapter.script != "keine" {
            return index;
        }
        index.native = true;
        index.entry_scene = config.script.entry.clone();

        let assets_path = confined_relative(&config.script.assets);
        let characters_path = confined_relative(&config.script.characters);
        index.assets_manifest = assets_path.clone();
        index.characters_manifest = characters_path.clone();

        let mut asset_lookup = HashSet::new();
        if let Some(path) = assets_path {
            match read_text(root, &path, overrides)
                .ok_or_else(|| "manifest is missing or is not UTF-8".to_owned())
                .and_then(|source| {
                    EiyashouAssetManifest::from_yaml(&source).map_err(|error| error.to_string())
                }) {
                Ok(manifest) => {
                    let unknown_namespaces = manifest
                        .unknown_namespaces()
                        .map(str::to_owned)
                        .collect::<Vec<_>>();
                    push_assets(
                        root,
                        &mut index,
                        &mut asset_lookup,
                        AssetKind::Background,
                        manifest.backgrounds,
                    );
                    push_assets(
                        root,
                        &mut index,
                        &mut asset_lookup,
                        AssetKind::Figure,
                        manifest.figures,
                    );
                    push_assets(
                        root,
                        &mut index,
                        &mut asset_lookup,
                        AssetKind::Voice,
                        manifest.voices,
                    );
                    push_assets(
                        root,
                        &mut index,
                        &mut asset_lookup,
                        AssetKind::Bgm,
                        manifest.bgm,
                    );
                    push_assets(
                        root,
                        &mut index,
                        &mut asset_lookup,
                        AssetKind::Effect,
                        manifest.effects,
                    );
                    push_assets(
                        root,
                        &mut index,
                        &mut asset_lookup,
                        AssetKind::Video,
                        manifest.videos,
                    );
                    push_assets(
                        root,
                        &mut index,
                        &mut asset_lookup,
                        AssetKind::Particle,
                        manifest.particles,
                    );
                    for namespace in unknown_namespaces {
                        index.problems.push(problem(
                            ProblemSeverity::Warning,
                            path.clone(),
                            1,
                            1,
                            format!("Unknown asset namespace `{namespace}` is preserved"),
                        ));
                    }
                }
                Err(error) => index.problems.push(problem(
                    ProblemSeverity::Error,
                    path,
                    1,
                    1,
                    format!("Invalid asset manifest: {error}"),
                )),
            }
        } else {
            index.problems.push(problem(
                ProblemSeverity::Error,
                config_path.clone(),
                1,
                1,
                "script.assets must be a confined relative path",
            ));
        }

        let mapped = index
            .assets
            .iter()
            .map(|asset| asset.path.as_path())
            .collect::<HashSet<_>>();
        index.unmapped = files
            .iter()
            .filter(|file| {
                !file.is_dir() && file.size > 0 && !mapped.contains(file.relative_path.as_path())
            })
            .filter_map(|file| {
                crate::file_ops::unmapped_candidate_kind(&file.relative_path).map(|kind| {
                    UnmappedAsset {
                        kind,
                        path: file.relative_path.clone(),
                    }
                })
            })
            .collect();
        for path in index
            .assets
            .iter()
            .map(|asset| &asset.path)
            .chain(index.unmapped.iter().map(|asset| &asset.path))
        {
            if let Some(info) = crate::file_ops::asset_media_info(root, path) {
                index.media.insert(path.clone(), info);
            }
        }
        index
            .unmapped
            .sort_by(|left, right| left.path.cmp(&right.path));

        if let Some(path) = characters_path {
            match read_text(root, &path, overrides)
                .ok_or_else(|| "manifest is missing or is not UTF-8".to_owned())
                .and_then(|source| {
                    EiyashouCharacterManifest::from_yaml(&source).map_err(|error| error.to_string())
                }) {
                Ok(manifest) => {
                    let unknown_fields = manifest
                        .unknown_fields()
                        .map(str::to_owned)
                        .collect::<Vec<_>>();
                    index.characters = manifest
                        .characters
                        .into_iter()
                        .map(|(id, character)| CharacterEntry {
                            id,
                            name: character.name,
                            color: character.color,
                            avatar: character.avatar,
                            expressions: character.expressions,
                        })
                        .collect();
                    index
                        .characters
                        .sort_by(|left, right| left.id.cmp(&right.id));
                    for character in &index.characters {
                        for id in character
                            .avatar
                            .iter()
                            .chain(character.expressions.values().flatten())
                        {
                            if !index.assets.iter().any(|asset| {
                                &asset.id == id && asset.kind == AssetKind::Figure && asset.exists
                            }) {
                                index.problems.push(problem(
                                    ProblemSeverity::Error,
                                    path.clone(),
                                    1,
                                    1,
                                    format!(
                                        "Character `{}` references missing figure `{id}`",
                                        character.id
                                    ),
                                ));
                            }
                        }
                        if character.expressions.values().any(Vec::is_empty) {
                            index.problems.push(problem(
                                ProblemSeverity::Error,
                                path.clone(),
                                1,
                                1,
                                format!("Character `{}` has an empty expression", character.id),
                            ));
                        }
                    }
                    for field in unknown_fields {
                        index.problems.push(problem(
                            ProblemSeverity::Warning,
                            path.clone(),
                            1,
                            1,
                            format!("Unknown character manifest field `{field}` is preserved"),
                        ));
                    }
                }
                Err(error) => index.problems.push(problem(
                    ProblemSeverity::Error,
                    path,
                    1,
                    1,
                    format!("Invalid character manifest: {error}"),
                )),
            }
        } else {
            index.problems.push(problem(
                ProblemSeverity::Error,
                config_path,
                1,
                1,
                "script.characters must be a confined relative path",
            ));
        }

        let mut referenced = HashMap::<(AssetKind, String), usize>::new();
        for file in files.iter().filter(|file| {
            file.relative_path
                .extension()
                .and_then(|value| value.to_str())
                == Some("shou")
        }) {
            if file.size > MAX_INDEXED_SOURCE_BYTES {
                index.unindexed_sources.push(file.relative_path.clone());
                continue;
            }
            let Some(source) = read_text(root, &file.relative_path, overrides) else {
                index.unindexed_sources.push(file.relative_path.clone());
                index.problems.push(problem(
                    ProblemSeverity::Error,
                    file.relative_path.clone(),
                    1,
                    1,
                    "Source is missing or is not UTF-8",
                ));
                continue;
            };
            if source.len() as u64 > MAX_INDEXED_SOURCE_BYTES {
                index.unindexed_sources.push(file.relative_path.clone());
                continue;
            }
            index_source(
                &file.relative_path,
                &source,
                &mut index,
                &mut referenced,
                &asset_lookup,
            );
            if index.problems.iter().any(|problem| {
                problem.path == file.relative_path && problem.severity == ProblemSeverity::Error
            }) {
                index.unindexed_sources.push(file.relative_path.clone());
            }
        }

        for asset in &mut index.assets {
            asset.reference_count = referenced
                .get(&(asset.kind, asset.id.clone()))
                .copied()
                .unwrap_or_default();
        }
        index.assets.sort_by(|left, right| {
            left.kind
                .cmp(&right.kind)
                .then_with(|| left.id.cmp(&right.id))
        });
        index.scenes.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.source_range.start.cmp(&right.source_range.start))
        });
        index.dialogues.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.source_range.start.cmp(&right.source_range.start))
        });
        index.variables.sort();
        index.variables.dedup();
        index.problems.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.line.cmp(&right.line))
                .then_with(|| left.column.cmp(&right.column))
                .then_with(|| left.message.cmp(&right.message))
        });
        index.problems.dedup();
        index
    }

    pub fn selection(&self, path: &Path, line: usize) -> AuthoringSelection<'_> {
        if let Some(dialogue) = self
            .dialogues
            .iter()
            .find(|dialogue| dialogue.path == path && dialogue.line.saturating_sub(1) == line)
        {
            return AuthoringSelection::Dialogue(dialogue);
        }
        self.scenes
            .iter()
            .filter(|scene| scene.path == path && scene.line.saturating_sub(1) <= line)
            .max_by_key(|scene| scene.line)
            .map_or(AuthoringSelection::Source, AuthoringSelection::Scene)
    }
}

fn natural_cmp(mut left: &str, mut right: &str) -> std::cmp::Ordering {
    while !left.is_empty() && !right.is_empty() {
        let l = left.chars().next().unwrap();
        let r = right.chars().next().unwrap();
        let order = if l.is_ascii_digit() && r.is_ascii_digit() {
            let l_end = left
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(left.len());
            let r_end = right
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(right.len());
            let l_number = left[..l_end].trim_start_matches('0');
            let r_number = right[..r_end].trim_start_matches('0');
            let order = l_number
                .len()
                .cmp(&r_number.len())
                .then_with(|| l_number.cmp(r_number));
            left = &left[l_end..];
            right = &right[r_end..];
            order
        } else {
            left = &left[l.len_utf8()..];
            right = &right[r.len_utf8()..];
            l.cmp(&r)
        };
        if !order.is_eq() {
            return order;
        }
    }
    left.len().cmp(&right.len())
}

#[derive(Clone, Copy, Debug)]
pub enum AuthoringSelection<'a> {
    Source,
    Scene(&'a SceneEntry),
    Dialogue(&'a DialogueEntry),
}

fn index_source(
    path: &Path,
    source: &str,
    index: &mut AuthoringIndex,
    referenced: &mut HashMap<(AssetKind, String), usize>,
    asset_lookup: &HashSet<(AssetKind, String)>,
) {
    let document = parse_native_document(source);
    let tokens = document
        .tokens
        .iter()
        .filter(|token| {
            !matches!(
                token.kind,
                NativeTokenKind::Whitespace | NativeTokenKind::Comment
            )
        })
        .collect::<Vec<_>>();
    index.variables.extend(
        tokens
            .windows(2)
            .filter(|pair| {
                source.get(pair[0].range.clone()) == Some("let")
                    && pair[1].kind == NativeTokenKind::Identifier
            })
            .map(|pair| (path.to_owned(), source[pair[1].range.clone()].to_owned())),
    );
    let lines = keine_loader::SourceLineIndex::new(source);
    let line_column = |offset| {
        let span = lines.span(source, offset);
        (span.line, span.column)
    };
    for scene in &document.scenes {
        let (line, _) = line_column(scene.name_range.start);
        index.scenes.push(SceneEntry {
            path: path.to_owned(),
            name: scene.name.clone(),
            line,
            name_range: scene.name_range.clone(),
            source_range: scene.range.clone(),
        });
    }
    for diagnostic in &document.diagnostics {
        index.problems.push(problem(
            match diagnostic.level {
                DiagnosticLevel::Warning => ProblemSeverity::Warning,
                DiagnosticLevel::Error => ProblemSeverity::Error,
            },
            path.to_owned(),
            diagnostic.span.line,
            diagnostic.span.column,
            diagnostic.message.clone(),
        ));
    }

    let strings = document
        .tokens
        .iter()
        .filter(|token| token.kind == NativeTokenKind::String)
        .map(|token| {
            let (line, column) = line_column(token.range.start);
            (token.range.clone(), line, column)
        })
        .collect::<Vec<_>>();
    let mut used_strings = HashSet::new();
    for (scene_index, parsed) in parse_native_scenes(source).into_iter().enumerate() {
        let scene_name = parsed.name.unwrap_or_else(|| "scene".to_owned());
        let scene_range = document
            .scenes
            .get(scene_index)
            .map(|scene| scene.range.clone())
            .unwrap_or(0..source.len());
        for (action_index, action) in parsed.report.actions.iter().enumerate() {
            let Some(span) = parsed.report.spans.get(action_index) else {
                continue;
            };
            if let Action::EiyashouSay(dialogue) = action
                && let Some((string_index, (range, line, column))) = strings
                    .iter()
                    .enumerate()
                    .skip(strings.partition_point(|(_, line, _)| *line < span.line))
                    .find(|(token_index, (_, token_line, _))| {
                        *token_line == span.line && !used_strings.contains(token_index)
                    })
                    .or_else(|| {
                        strings
                            .iter()
                            .enumerate()
                            .skip(strings.partition_point(|(_, line, _)| *line < span.line))
                            .find(|(token_index, (range, token_line, _))| {
                                scene_range.contains(&range.start)
                                    && *token_line >= span.line
                                    && !used_strings.contains(token_index)
                            })
                    })
            {
                used_strings.insert(string_index);
                let raw = source.get(range.clone()).unwrap_or("");
                let inner = range.start.saturating_add(1)..range.end.saturating_sub(1);
                let editable = !raw.contains("${");
                let text = if editable {
                    dialogue
                        .text
                        .parts
                        .iter()
                        .filter_map(|part| match part {
                            EiyashouTextPart::Literal(value) => Some(value.as_str()),
                            EiyashouTextPart::Expression(_) => None,
                        })
                        .collect::<String>()
                } else {
                    source.get(inner.clone()).unwrap_or("").to_owned()
                };
                index.dialogues.push(DialogueEntry {
                    path: path.to_owned(),
                    scene: scene_name.clone(),
                    speaker: dialogue.speaker.clone(),
                    text,
                    editable,
                    line: *line,
                    column: *column,
                    source_range: range.clone(),
                    text_range: inner,
                });
            }
        }
        for diagnostic in parsed.report.diagnostics {
            index.problems.push(problem(
                match diagnostic.level {
                    DiagnosticLevel::Warning => ProblemSeverity::Warning,
                    DiagnosticLevel::Error => ProblemSeverity::Error,
                },
                path.to_owned(),
                diagnostic.span.line,
                diagnostic.span.column,
                diagnostic.message,
            ));
        }
        for resource in parsed.report.resources {
            let Some(mut kind) = AssetKind::from_resource(resource.kind) else {
                continue;
            };
            if resource.is_dynamic() {
                continue;
            }
            if kind == AssetKind::Figure
                && !asset_lookup.contains(&(kind, resource.path.clone()))
                && asset_lookup.contains(&(AssetKind::Background, resource.path.clone()))
            {
                kind = AssetKind::Background;
            }
            let start = lines.offset(
                source,
                resource.span.line.saturating_sub(1),
                resource.span.column.saturating_sub(1),
            );
            let end = parsed
                .report
                .spans
                .iter()
                .skip(resource.action_index + 1)
                .map(|span| {
                    lines.offset(
                        source,
                        span.line.saturating_sub(1),
                        span.column.saturating_sub(1),
                    )
                })
                .find(|offset| *offset > start)
                .unwrap_or(scene_range.end);
            let first = document
                .tokens
                .partition_point(|token| token.range.start < start);
            let matches = document.tokens[first..]
                .iter()
                .take_while(|token| token.range.start < end)
                .filter(|token| {
                    matches!(
                        token.kind,
                        NativeTokenKind::Identifier | NativeTokenKind::String
                    )
                })
                .filter_map(|token| {
                    let raw = source.get(token.range.clone())?;
                    (raw == resource.path || raw == format!("\"{}\"", resource.path))
                        .then_some(token.range.clone())
                })
                .collect::<Vec<_>>();
            let range = (matches.len() == 1).then(|| matches[0].clone());
            let (line, column) = range
                .as_ref()
                .map(|range| line_column(range.start))
                .unwrap_or((resource.span.line, resource.span.column));
            index.asset_references.push(AssetReference {
                key: AssetKey {
                    kind,
                    id: resource.path.clone(),
                },
                path: path.to_owned(),
                line,
                column,
                range,
            });
            *referenced.entry((kind, resource.path.clone())).or_default() += 1;
            if !asset_lookup.contains(&(kind, resource.path.clone())) {
                index.problems.push(problem(
                    ProblemSeverity::Error,
                    path.to_owned(),
                    line,
                    column,
                    format!("Unknown {} asset `{}`", kind.label(), resource.path),
                ));
            } else if let Some(asset) = index
                .assets
                .iter()
                .find(|asset| asset.kind == kind && asset.id == resource.path)
                && !asset.exists
            {
                index.problems.push(problem(
                    ProblemSeverity::Error,
                    path.to_owned(),
                    line,
                    column,
                    format!(
                        "{} asset `{}` has no file: {}",
                        kind.label(),
                        resource.path,
                        asset.path.display()
                    ),
                ));
            }
        }
    }
}

fn push_assets(
    root: &Path,
    index: &mut AuthoringIndex,
    lookup: &mut HashSet<(AssetKind, String)>,
    kind: AssetKind,
    values: HashMap<String, EiyashouAssetEntry>,
) {
    let mut values = values.into_iter().collect::<Vec<_>>();
    values.sort_by(|left, right| left.0.cmp(&right.0));
    let mut physical_files = HashMap::<PathBuf, String>::new();
    for (id, entry) in values {
        let value = entry.path();
        let tags = entry.tags().to_vec();
        lookup.insert((kind, id.clone()));
        let relative = confined_relative(value);
        let existing = relative
            .as_deref()
            .and_then(|path| confined_existing_file(root, path));
        let exists = existing.is_some();
        let path = relative.unwrap_or_else(|| PathBuf::from(value));
        if id.is_empty() {
            index.problems.push(problem(
                ProblemSeverity::Error,
                index
                    .assets_manifest
                    .clone()
                    .unwrap_or_else(|| PathBuf::from("assets.yaml")),
                1,
                1,
                format!("{} asset identifiers must not be empty", kind.label()),
            ));
        }
        let identity = existing.unwrap_or_else(|| path.clone());
        if let Some(previous) = physical_files.insert(identity, id.clone()) {
            index.problems.push(problem(
                ProblemSeverity::Error,
                index
                    .assets_manifest
                    .clone()
                    .unwrap_or_else(|| PathBuf::from("assets.yaml")),
                1,
                1,
                format!(
                    "{} assets `{previous}` and `{id}` map to the same file: {}",
                    kind.label(),
                    path.display()
                ),
            ));
        }
        if !exists {
            index.problems.push(problem(
                ProblemSeverity::Error,
                index
                    .assets_manifest
                    .clone()
                    .unwrap_or_else(|| PathBuf::from("assets.yaml")),
                1,
                1,
                format!(
                    "{} asset `{id}` is missing or escapes the project: {}",
                    kind.label(),
                    path.display()
                ),
            ));
        }
        index.assets.push(AssetEntry {
            kind,
            id,
            path,
            tags,
            exists,
            reference_count: 0,
        });
    }
}

fn read_text(
    root: &Path,
    relative: &Path,
    overrides: &BTreeMap<PathBuf, String>,
) -> Option<String> {
    overrides
        .get(relative)
        .cloned()
        .or_else(|| fs::read_to_string(root.join(relative)).ok())
}

pub(super) fn confined_relative(value: &str) -> Option<PathBuf> {
    let path = Path::new(value);
    (!path.as_os_str().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_))))
    .then(|| path.to_owned())
}

pub(crate) fn confined_existing_file(root: &Path, relative: &Path) -> Option<PathBuf> {
    let Ok(canonical_root) = root.canonicalize() else {
        return None;
    };
    let Ok(canonical) = root.join(relative).canonicalize() else {
        return None;
    };
    (canonical.starts_with(canonical_root) && canonical.is_file()).then_some(canonical)
}

fn problem(
    severity: ProblemSeverity,
    path: PathBuf,
    line: usize,
    column: usize,
    message: impl Into<String>,
) -> AuthoringProblem {
    AuthoringProblem {
        severity,
        path,
        line,
        column,
        message: message.into(),
    }
}

pub fn dialogues_for_source(path: &Path, source: &str) -> Vec<DialogueEntry> {
    let mut index = AuthoringIndex::default();
    index_source(
        path,
        source,
        &mut index,
        &mut HashMap::new(),
        &HashSet::new(),
    );
    // The runtime parser omits empty speech, but the editor must keep those
    // source-backed blocks editable so Enter can continue a blank sequence.
    if source.contains("\"\"")
        && path
            .extension()
            .is_some_and(|extension| extension == "shou")
    {
        for scene in EiyashouProjection::parse(source).scenes {
            for block in scene.blocks {
                let Some(range) = block.text_range.clone() else {
                    continue;
                };
                if range.start != range.end || block.read_only {
                    continue;
                }
                let speaker = match block.kind {
                    BlockKind::Narration => String::new(),
                    BlockKind::Dialogue { speaker } => speaker,
                    _ => continue,
                };
                if index
                    .dialogues
                    .iter()
                    .any(|dialogue| dialogue.text_range == range)
                {
                    continue;
                }
                index.dialogues.push(DialogueEntry {
                    path: path.to_owned(),
                    scene: scene.name.clone(),
                    speaker,
                    text: String::new(),
                    editable: true,
                    line: block.line + 1,
                    column: block.column + 1,
                    source_range: block.source_range,
                    text_range: range,
                });
            }
        }
        index
            .dialogues
            .sort_by_key(|dialogue| dialogue.source_range.start);
    }
    index.dialogues
}

#[cfg(test)]
#[path = "../../../../tests/bench/editor/index.rs"]
mod benchmark;
