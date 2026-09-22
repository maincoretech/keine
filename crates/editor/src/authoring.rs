use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
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

use crate::projection::EiyashouProjection;
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
            ResourceKind::Particle | ResourceKind::Lut => None,
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
pub struct CharacterEntry {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
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
    pub assets_manifest: Option<PathBuf>,
    pub characters_manifest: Option<PathBuf>,
    pub assets: Vec<AssetEntry>,
    pub characters: Vec<CharacterEntry>,
    pub scenes: Vec<SceneEntry>,
    pub dialogues: Vec<DialogueEntry>,
    pub problems: Vec<AuthoringProblem>,
}

impl AuthoringIndex {
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
                        })
                        .collect();
                    index
                        .characters
                        .sort_by(|left, right| left.id.cmp(&right.id));
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
            file.size <= MAX_INDEXED_SOURCE_BYTES
                && file
                    .relative_path
                    .extension()
                    .and_then(|value| value.to_str())
                    == Some("shou")
        }) {
            let Some(source) = read_text(root, &file.relative_path, overrides) else {
                index.problems.push(problem(
                    ProblemSeverity::Error,
                    file.relative_path.clone(),
                    1,
                    1,
                    "Source is missing or is not UTF-8",
                ));
                continue;
            };
            index_source(
                &file.relative_path,
                &source,
                &mut index,
                &mut referenced,
                &asset_lookup,
            );
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

#[derive(Clone, Copy, Debug)]
pub enum AuthoringSelection<'a> {
    Source,
    Scene(&'a SceneEntry),
    Dialogue(&'a DialogueEntry),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InsertKind {
    Narration,
    Dialogue,
    Background,
    Figure,
    Choice,
    Conditional,
    Loop,
    Variable,
    Goto,
    Call,
    Wait,
    Hide,
    Move,
    Bgm,
    Effect,
    Video,
    Return,
}

impl InsertKind {
    pub const ALL: [Self; 17] = [
        Self::Narration,
        Self::Dialogue,
        Self::Background,
        Self::Figure,
        Self::Hide,
        Self::Move,
        Self::Bgm,
        Self::Effect,
        Self::Video,
        Self::Choice,
        Self::Conditional,
        Self::Loop,
        Self::Goto,
        Self::Call,
        Self::Return,
        Self::Wait,
        Self::Variable,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Narration => "Narration",
            Self::Dialogue => "Dialogue",
            Self::Background => "Background",
            Self::Figure => "Figure",
            Self::Choice => "Choice",
            Self::Conditional => "If",
            Self::Loop => "Loop",
            Self::Variable => "Variable",
            Self::Goto => "Goto",
            Self::Call => "Call",
            Self::Wait => "Wait",
            Self::Hide => "Hide",
            Self::Move => "Move",
            Self::Bgm => "BGM",
            Self::Effect => "Sound",
            Self::Video => "Video",
            Self::Return => "Return",
        }
    }

    pub const fn category(self) -> &'static str {
        match self {
            Self::Narration | Self::Dialogue => "Text",
            Self::Background | Self::Figure | Self::Hide | Self::Move => "Scene",
            Self::Bgm | Self::Effect | Self::Video => "Media",
            Self::Choice
            | Self::Conditional
            | Self::Loop
            | Self::Goto
            | Self::Call
            | Self::Return
            | Self::Wait => "Flow",
            Self::Variable => "Data",
        }
    }

    pub const fn search_terms(self) -> &'static str {
        match self {
            Self::Narration => "narration text narrator",
            Self::Dialogue => "dialogue speaker character text",
            Self::Background => "background scene image",
            Self::Figure => "figure sprite character show",
            Self::Choice => "choice branch option",
            Self::Conditional => "if conditional branch",
            Self::Loop => "loop repeat",
            Self::Variable => "variable let data",
            Self::Goto => "goto scene jump",
            Self::Call => "call scene",
            Self::Wait => "wait delay time",
            Self::Hide => "hide sprite figure",
            Self::Move => "move sprite figure position",
            Self::Bgm => "bgm music audio",
            Self::Effect => "sound effect se audio",
            Self::Video => "video movie",
            Self::Return => "return flow",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthoringEditError {
    InvalidIdentifier,
    DuplicateIdentifier,
    EmptyName,
    InvalidColor,
    MissingInsertionPoint,
    InvalidManifest,
    UnsupportedDynamicText,
    StaleRange,
}

impl fmt::Display for AuthoringEditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentifier => formatter.write_str("identifier is invalid"),
            Self::DuplicateIdentifier => formatter.write_str("identifier is already used"),
            Self::EmptyName => formatter.write_str("display name cannot be empty"),
            Self::InvalidColor => formatter.write_str("color must use #RRGGBB"),
            Self::MissingInsertionPoint => formatter.write_str("no stable insertion point exists"),
            Self::InvalidManifest => formatter.write_str("manifest is not valid Eiyashou YAML"),
            Self::UnsupportedDynamicText => {
                formatter.write_str("dynamic dialogue must be edited in Text view")
            }
            Self::StaleRange => formatter.write_str("source changed; refresh this view"),
        }
    }
}

impl std::error::Error for AuthoringEditError {}

pub fn replace_dialogue_text(
    source: &str,
    dialogue: &DialogueEntry,
    text: &str,
) -> Result<String, AuthoringEditError> {
    if !dialogue.editable {
        return Err(AuthoringEditError::UnsupportedDynamicText);
    }
    if source.get(dialogue.source_range.clone()).is_none()
        || source.get(dialogue.text_range.clone()).is_none()
    {
        return Err(AuthoringEditError::StaleRange);
    }
    let mut edited = source.to_owned();
    edited.replace_range(dialogue.text_range.clone(), &escape_eiyashou_string(text));
    Ok(edited)
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
    index.dialogues
}

pub fn insert_statement(
    source: &str,
    line: usize,
    kind: InsertKind,
    index: &AuthoringIndex,
) -> Result<String, AuthoringEditError> {
    let line_start =
        line_start_offset(source, line).ok_or(AuthoringEditError::MissingInsertionPoint)?;
    let line_end = source[line_start..]
        .find('\n')
        .map(|offset| line_start + offset + 1)
        .unwrap_or(source.len());
    let content_end = line_end.saturating_sub(usize::from(
        line_end > line_start && source.as_bytes().get(line_end - 1) == Some(&b'\n'),
    ));
    let current = source[line_start..content_end].trim();
    if current.starts_with('}') {
        return Err(AuthoringEditError::MissingInsertionPoint);
    }
    let next = source[line_end..].trim_start();
    let statement_has_follower = !next.is_empty() && !next.starts_with('}');
    let current_needs_separator = !current.is_empty()
        && !current.ends_with('{')
        && !current.ends_with(',')
        && !current.ends_with('}');
    let indent = source[line_start..]
        .chars()
        .take_while(|character| character.is_whitespace() && *character != '\n')
        .collect::<String>();
    let statement_indent = if indent.is_empty() {
        "  ".to_owned()
    } else {
        indent
    };
    let first_character = index.characters.first().map(|entry| entry.id.as_str());
    let first_background = index
        .assets
        .iter()
        .find(|entry| entry.kind == AssetKind::Background)
        .map(|entry| entry.id.as_str());
    let first_figure = index
        .assets
        .iter()
        .find(|entry| entry.kind == AssetKind::Figure)
        .map(|entry| entry.id.as_str());
    let first_scene = index.scenes.first().map(|entry| entry.name.as_str());
    let statement = match kind {
        InsertKind::Narration => "\"New narration\"".to_owned(),
        InsertKind::Dialogue => format!(
            "{}: \"New dialogue\"",
            first_character.ok_or(AuthoringEditError::MissingInsertionPoint)?
        ),
        InsertKind::Background => format!(
            "background({})",
            first_background.ok_or(AuthoringEditError::MissingInsertionPoint)?
        ),
        InsertKind::Figure => {
            let figure = first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?;
            format!("sprite({figure}_slot, {figure}, position: center)")
        }
        InsertKind::Choice => {
            let scene = first_scene.ok_or(AuthoringEditError::MissingInsertionPoint)?;
            format!(
                "choice {{\n{statement_indent}  \"Continue\": goto({scene})\n{statement_indent}}}"
            )
        }
        InsertKind::Conditional => {
            format!("if (true) {{\n{statement_indent}  \"New narration\"\n{statement_indent}}}")
        }
        InsertKind::Loop => "loop { break }".to_owned(),
        InsertKind::Variable => format!("let {} = 0", unique_variable_name(source)),
        InsertKind::Goto => format!(
            "goto({})",
            first_scene.ok_or(AuthoringEditError::MissingInsertionPoint)?
        ),
        InsertKind::Call => format!(
            "call({})",
            first_scene.ok_or(AuthoringEditError::MissingInsertionPoint)?
        ),
        InsertKind::Wait => "wait(500ms)".to_owned(),
        InsertKind::Hide => {
            let figure = first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?;
            format!("hide({figure}_slot)")
        }
        InsertKind::Move => {
            let figure = first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?;
            format!("move({figure}_slot, center)")
        }
        InsertKind::Bgm => format!(
            "bgm({})",
            index
                .assets
                .iter()
                .find(|entry| entry.kind == AssetKind::Bgm)
                .map(|entry| entry.id.as_str())
                .ok_or(AuthoringEditError::MissingInsertionPoint)?
        ),
        InsertKind::Effect => format!(
            "se({})",
            index
                .assets
                .iter()
                .find(|entry| entry.kind == AssetKind::Effect)
                .map(|entry| entry.id.as_str())
                .ok_or(AuthoringEditError::MissingInsertionPoint)?
        ),
        InsertKind::Video => format!(
            "video({})",
            index
                .assets
                .iter()
                .find(|entry| entry.kind == AssetKind::Video)
                .map(|entry| entry.id.as_str())
                .ok_or(AuthoringEditError::MissingInsertionPoint)?
        ),
        InsertKind::Return => "return".to_owned(),
    };
    let mut edited = source.to_owned();
    let mut insertion = line_end;
    if current_needs_separator {
        edited.insert(content_end, ',');
        insertion += 1;
    }
    let trailing = if statement_has_follower { "," } else { "" };
    edited.insert_str(
        insertion,
        &format!("{statement_indent}{statement}{trailing}\n"),
    );
    Ok(edited)
}

pub fn append_scene(source: &str, id: &str) -> Result<String, AuthoringEditError> {
    if !valid_identifier(id) {
        return Err(AuthoringEditError::InvalidIdentifier);
    }
    if EiyashouProjection::parse(source)
        .scenes
        .iter()
        .any(|scene| scene.name == id)
    {
        return Err(AuthoringEditError::DuplicateIdentifier);
    }
    let separator = if source.is_empty() || source.ends_with("\n\n") {
        ""
    } else if source.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    Ok(format!("{source}{separator}scene {id} {{\n  \"\"\n}}\n"))
}

pub fn append_character(
    source: &str,
    id: &str,
    name: &str,
    color: Option<&str>,
) -> Result<String, AuthoringEditError> {
    if !valid_identifier(id) {
        return Err(AuthoringEditError::InvalidIdentifier);
    }
    if name.trim().is_empty() {
        return Err(AuthoringEditError::EmptyName);
    }
    if let Some(color) = color.filter(|value| !value.trim().is_empty())
        && !valid_color(color)
    {
        return Err(AuthoringEditError::InvalidColor);
    }
    let manifest = EiyashouCharacterManifest::from_yaml(source)
        .map_err(|_| AuthoringEditError::InvalidManifest)?;
    if manifest.characters.contains_key(id) {
        return Err(AuthoringEditError::DuplicateIdentifier);
    }
    let mut entry = format!("  {id}:\n    name: \"{}\"\n", escape_yaml_string(name));
    if let Some(color) = color.filter(|value| !value.trim().is_empty()) {
        entry.push_str(&format!("    color: \"{}\"\n", escape_yaml_string(color)));
    }
    let mut edited = source.to_owned();
    if let Some(empty_mapping) = source.find("characters: {}") {
        let range = empty_mapping..empty_mapping + "characters: {}".len();
        edited.replace_range(range, &format!("characters:\n{entry}"));
    } else {
        let insertion =
            character_insertion_offset(source).ok_or(AuthoringEditError::MissingInsertionPoint)?;
        edited.insert_str(insertion, &entry);
    }
    EiyashouCharacterManifest::from_yaml(&edited)
        .map_err(|_| AuthoringEditError::InvalidManifest)?;
    Ok(edited)
}

pub fn escape_eiyashou_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            '$' if characters.peek() == Some(&'{') => escaped.push_str("/$"),
            _ => escaped.push(character),
        }
    }
    escaped
}

fn unique_variable_name(source: &str) -> String {
    let document = parse_native_document(source);
    let identifiers = document
        .tokens
        .iter()
        .filter(|token| token.kind == NativeTokenKind::Identifier)
        .filter_map(|token| source.get(token.range.clone()))
        .collect::<HashSet<_>>();
    if !identifiers.contains("value") {
        return "value".to_owned();
    }
    (2..)
        .map(|suffix| format!("value_{suffix}"))
        .find(|candidate| !identifiers.contains(candidate.as_str()))
        .expect("an unbounded numeric suffix always yields a unique identifier")
}

fn index_source(
    path: &Path,
    source: &str,
    index: &mut AuthoringIndex,
    referenced: &mut HashMap<(AssetKind, String), usize>,
    asset_lookup: &HashSet<(AssetKind, String)>,
) {
    let document = parse_native_document(source);
    for scene in &document.scenes {
        let (line, _) = line_column(source, scene.name_range.start);
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
            let (line, column) = line_column(source, token.range.start);
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
                    .find(|(token_index, (_, token_line, _))| {
                        *token_line == span.line && !used_strings.contains(token_index)
                    })
                    .or_else(|| {
                        strings
                            .iter()
                            .enumerate()
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
            let Some(kind) = AssetKind::from_resource(resource.kind) else {
                continue;
            };
            if resource.is_dynamic() {
                continue;
            }
            *referenced.entry((kind, resource.path.clone())).or_default() += 1;
            if !asset_lookup.contains(&(kind, resource.path.clone())) {
                index.problems.push(problem(
                    ProblemSeverity::Error,
                    path.to_owned(),
                    resource.span.line,
                    resource.span.column,
                    format!("Unknown {} asset `{}`", kind.label(), resource.path),
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

fn confined_relative(value: &str) -> Option<PathBuf> {
    let path = Path::new(value);
    (!path.as_os_str().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_))))
    .then(|| path.to_owned())
}

fn confined_existing_file(root: &Path, relative: &Path) -> Option<PathBuf> {
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

fn line_column(source: &str, offset: usize) -> (usize, usize) {
    let prefix = source.get(..offset).unwrap_or(source);
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix.chars().count(), |(_, tail)| tail.chars().count())
        + 1;
    (line, column)
}

fn line_start_offset(source: &str, line: usize) -> Option<usize> {
    if line == 0 {
        return Some(0);
    }
    source
        .match_indices('\n')
        .nth(line.saturating_sub(1))
        .map(|(offset, _)| offset + 1)
}

fn character_insertion_offset(source: &str) -> Option<usize> {
    let mut offset = 0;
    let mut in_characters = false;
    for segment in source.split_inclusive('\n') {
        let trimmed = segment.trim_end_matches(['\r', '\n']);
        if !in_characters {
            if trimmed.trim() == "characters:" && !trimmed.starts_with(char::is_whitespace) {
                in_characters = true;
            }
        } else if !trimmed.is_empty() && !trimmed.starts_with(char::is_whitespace) {
            return Some(offset);
        }
        offset += segment.len();
    }
    in_characters.then_some(source.len())
}

fn escape_yaml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn valid_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    (first == '_' || first.is_alphabetic())
        && characters.all(|character| character == '_' || character.is_alphanumeric())
}

fn valid_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
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
        let root = std::env::temp_dir().join(format!("keine-authoring-index-{nonce}"));
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(
            root.join("config.yaml"),
            "adapter:\n  script: keine\nscript:\n  assets: assets.yaml\n  characters: characters.yaml\n",
        )
        .unwrap();
        fs::write(
            root.join("assets.yaml"),
            "backgrounds:\n  room:\n    path: assets/room.webp\n    tags: [interior, chapter-1]\nfigures:\n  rin: assets/missing.webp\n",
        )
        .unwrap();
        fs::write(root.join("assets/room.webp"), b"image").unwrap();
        fs::write(
            root.join("characters.yaml"),
            "characters:\n  rin:\n    name: \"Rin\"\n",
        )
        .unwrap();
        fs::write(
            root.join("scripts/main.shou"),
            "scene start {\n  background(room),\n  rin: \"Hello\",\n}\n",
        )
        .unwrap();
        root
    }

    #[test]
    fn index_is_deterministic_and_reports_confined_missing_assets() {
        let root = fixture();
        let files = vec![WorkspaceFile {
            relative_path: PathBuf::from("scripts/main.shou"),
            size: fs::metadata(root.join("scripts/main.shou")).unwrap().len(),
            kind: crate::workspace::WorkspaceEntryKind::File,
        }];
        let index = AuthoringIndex::load(&root, &files, &BTreeMap::new());
        assert!(index.native);
        assert_eq!(index.scenes[0].name, "start");
        assert_eq!(index.dialogues[0].speaker, "rin");
        assert_eq!(index.assets[0].id, "room");
        assert_eq!(index.assets[0].tags, ["interior", "chapter-1"]);
        assert_eq!(index.assets[0].reference_count, 1);
        assert!(
            index
                .problems
                .iter()
                .any(|problem| problem.message.contains("missing.webp"))
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn index_reports_same_type_duplicate_files_but_allows_cross_type_sharing() {
        let root = fixture();
        fs::write(
            root.join("assets.yaml"),
            concat!(
                "backgrounds:\n",
                "  first: assets/room.webp\n",
                "  second: assets/room.webp\n",
                "figures:\n",
                "  shared: assets/room.webp\n",
            ),
        )
        .unwrap();
        let index = AuthoringIndex::load(&root, &[], &BTreeMap::new());
        let duplicate = index
            .problems
            .iter()
            .filter(|problem| problem.message.contains("map to the same file"))
            .collect::<Vec<_>>();

        assert_eq!(duplicate.len(), 1);
        assert!(
            duplicate[0]
                .message
                .contains("Background assets `first` and `second`")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dialogue_replacement_is_bounded_and_escapes_literal_interpolation() {
        let source = "scene start {\n  rin: \"Hello\",\n}\n";
        let root = fixture();
        let path = PathBuf::from("scripts/main.shou");
        fs::write(root.join(&path), source).unwrap();
        let files = vec![WorkspaceFile {
            relative_path: path,
            size: source.len() as u64,
            kind: crate::workspace::WorkspaceEntryKind::File,
        }];
        let index = AuthoringIndex::load(&root, &files, &BTreeMap::new());
        let edited = replace_dialogue_text(source, &index.dialogues[0], "你说 \"${x}\"").unwrap();
        assert_eq!(edited, "scene start {\n  rin: \"你说 \\\"/${x}\\\"\",\n}\n");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn continuous_dialogue_block_is_indexed_in_source_order() {
        let source = concat!(
            "scene start {\n",
            "  rin: {\n",
            "    \"First\",\n",
            "    \"Second\"\n",
            "  }\n",
            "}\n",
        );
        let dialogues = dialogues_for_source(Path::new("scripts/main.shou"), source);
        assert_eq!(
            dialogues
                .iter()
                .map(|dialogue| dialogue.text.as_str())
                .collect::<Vec<_>>(),
            ["First", "Second"]
        );
        assert_eq!(dialogues[0].line, 3);
        assert_eq!(dialogues[1].line, 4);
    }

    #[test]
    fn character_and_scene_creation_preserve_surrounding_source() {
        let characters = "characters:\n  rin:\n    name: \"Rin\"\nmetadata: keep\n";
        let edited = append_character(characters, "yui", "Yui", Some("#BAEBFF")).unwrap();
        assert!(
            edited.contains("  yui:\n    name: \"Yui\"\n    color: \"#BAEBFF\"\nmetadata: keep")
        );
        let script = "// keep\nscene start { \"Hi\" }\n";
        let edited = append_scene(script, "next").unwrap();
        assert!(edited.starts_with(script));
        assert!(edited.ends_with("scene next {\n  \"\"\n}\n"));
    }

    #[test]
    fn palette_inserts_after_a_stable_line_boundary() {
        let source = "scene start {\n  \"First\",\n}\n";
        let mut index = AuthoringIndex::default();
        index.characters.push(CharacterEntry {
            id: "rin".into(),
            name: "Rin".into(),
            color: None,
        });
        let edited = insert_statement(source, 1, InsertKind::Dialogue, &index).unwrap();
        assert_eq!(
            edited,
            "scene start {\n  \"First\",\n  rin: \"New dialogue\"\n}\n"
        );

        index.assets.push(AssetEntry {
            kind: AssetKind::Background,
            id: "room".into(),
            path: "assets/room.webp".into(),
            tags: Vec::new(),
            exists: true,
            reference_count: 0,
        });
        index.assets.push(AssetEntry {
            kind: AssetKind::Figure,
            id: "rin_smile".into(),
            path: "assets/rin.webp".into(),
            tags: Vec::new(),
            exists: true,
            reference_count: 0,
        });
        index.scenes.push(SceneEntry {
            path: "scripts/main.shou".into(),
            name: "start".into(),
            line: 1,
            name_range: 6..11,
            source_range: 0..source.len(),
        });
        for kind in [
            InsertKind::Narration,
            InsertKind::Background,
            InsertKind::Figure,
            InsertKind::Choice,
        ] {
            let edited = insert_statement(source, 1, kind, &index).unwrap();
            let diagnostics = parse_native_document(&edited).diagnostics;
            assert!(
                diagnostics
                    .iter()
                    .all(|diagnostic| diagnostic.level != DiagnosticLevel::Error),
                "{kind:?}: {diagnostics:?}\n{edited}"
            );
        }
    }

    #[test]
    fn rejects_escape_paths_and_duplicate_identifiers() {
        assert!(confined_relative("../outside").is_none());
        assert_eq!(
            append_scene("scene start {}", "start"),
            Err(AuthoringEditError::DuplicateIdentifier)
        );
        assert_eq!(
            append_character("characters: {}\n", "not valid", "Name", None),
            Err(AuthoringEditError::InvalidIdentifier)
        );
        assert_eq!(
            append_character("characters: {}\n", "rin", "", None),
            Err(AuthoringEditError::EmptyName)
        );
        assert_eq!(
            append_character("characters: {}\n", "rin", "Rin", Some("blue")),
            Err(AuthoringEditError::InvalidColor)
        );
        assert!(
            append_character("characters: {}\n", "rin", "Rin", Some("#BAEBFF"))
                .unwrap()
                .contains("characters:\n  rin:")
        );
    }
}
