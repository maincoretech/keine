pub(crate) mod commands;
mod edit;
pub(crate) mod fields;
mod index;
pub mod projection;
pub mod syntax;

pub use commands::{InsertKind, insertion_statement};
pub use edit::{
    AuthoringEditError, append_character, append_scene, delete_character, delete_scene,
    edit_character, escape_eiyashou_string, insert_source_statement, insert_statement, move_scene,
    rename_character_asset, rename_scene, rename_scene_references, replace_dialogue_text,
    scene_references,
};
pub(crate) use index::confined_existing_file;
pub use index::{
    AssetEntry, AssetKey, AssetKind, AssetQuery, AssetReference, AssetSize, AssetSort, AssetStatus,
    AuthoringIndex, AuthoringProblem, AuthoringSelection, CharacterEntry, DialogueEntry,
    ProblemSeverity, SceneEntry, UnmappedAsset, dialogues_for_source,
};

pub(crate) fn valid_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    (first == '_' || first.is_alphabetic())
        && characters.all(|character| character == '_' || character.is_alphanumeric())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::index::confined_relative;
    use super::*;
    use crate::projection::EiyashouProjection;
    use crate::workspace::WorkspaceFile;
    use keine_loader::{DiagnosticLevel, parse_native_document};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn explorer_scripts_follow_entry_and_scene_numbers_without_moving_folders() {
        use crate::workspace::WorkspaceEntryKind;
        let base = AuthoringIndex {
            native: true,
            entry_scene: "start".into(),
            ..Default::default()
        };
        let sources = BTreeMap::from([
            (PathBuf::from("scripts/a.shou"), "scene chapter10 {}".into()),
            (PathBuf::from("scripts/b.shou"), "scene chapter2 {}".into()),
            (
                PathBuf::from("scripts/dual.shou"),
                "scene chapter20 {} scene chapter1 {}".into(),
            ),
            (PathBuf::from("scripts/z.shou"), "scene start {}".into()),
            (
                PathBuf::from("scripts/sub/aa.shou"),
                "scene chapter10 {}".into(),
            ),
            (
                PathBuf::from("scripts/sub/zz.shou"),
                "scene chapter2 {}".into(),
            ),
        ]);
        let index = base.with_sources(&sources);
        let mut files = [
            "assets",
            "scripts",
            "scripts/a.shou",
            "scripts/b.shou",
            "scripts/dual.shou",
            "scripts/sub",
            "scripts/sub/aa.shou",
            "scripts/sub/zz.shou",
            "scripts/z.shou",
            "config.yaml",
        ]
        .into_iter()
        .map(|path| WorkspaceFile {
            relative_path: path.into(),
            size: 0,
            kind: if path.contains('.') {
                WorkspaceEntryKind::File
            } else {
                WorkspaceEntryKind::Directory
            },
        })
        .collect::<Vec<_>>();
        index.order_script_files(&mut files);
        let ordered = files
            .iter()
            .map(|file| file.relative_path.to_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            ordered,
            [
                "assets",
                "scripts",
                "scripts/z.shou",
                "scripts/dual.shou",
                "scripts/b.shou",
                "scripts/sub",
                "scripts/sub/zz.shou",
                "scripts/sub/aa.shou",
                "scripts/a.shou",
                "config.yaml"
            ]
        );
        let updated = index.with_sources(&BTreeMap::from([(
            PathBuf::from("scripts/b.shou"),
            "scene chapter30 {}".into(),
        )]));
        assert_eq!(updated.entry_scene, "start");
        updated.order_script_files(&mut files);
        assert_eq!(files[4].relative_path, Path::new("scripts/a.shou"));
        assert_eq!(files[8].relative_path, Path::new("scripts/b.shou"));
    }

    #[test]
    fn checked_in_native_fixture_populates_editor_views() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/native-smoke");
        let workspace = crate::workspace::WorkspaceSession::open(&root).unwrap();
        let files = workspace.files();
        assert!(
            files
                .iter()
                .any(|file| file.relative_path == Path::new("scripts/main.shou"))
        );
        let index = AuthoringIndex::load(&root, files, &BTreeMap::new());
        assert!(index.native);
        assert_eq!(index.scenes.len(), 1);
        assert!(index.assets.is_empty());
        assert!(index.problems.is_empty(), "{:?}", index.problems);
    }

    #[test]
    fn incremental_script_index_matches_a_fresh_load() {
        let root = fixture();
        let session = crate::workspace::WorkspaceSession::open(&root).unwrap();
        let original = AuthoringIndex::load(&root, session.files(), &BTreeMap::new());
        let changes = BTreeMap::from([(
            PathBuf::from("scripts/main.shou"),
            "scene opening { background(room), \"changed\", wait(1s) }".into(),
        )]);
        let next = original.with_sources(&changes);
        let fresh = AuthoringIndex::load(&root, session.files(), &changes);
        assert_eq!(next.scenes, fresh.scenes);
        assert_eq!(next.dialogues, fresh.dialogues);
        assert_eq!(next.variables, fresh.variables);
        assert_eq!(next.assets, fresh.assets);
        assert_eq!(next.asset_references, fresh.asset_references);
        assert_eq!(next.problems, fresh.problems);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn source_refresh_preserves_other_files_and_recovers_errors() {
        let root = fixture();
        fs::write(
            root.join("scripts/other.shou"),
            "scene other { background(room), \"other\" }",
        )
        .unwrap();
        let session = crate::workspace::WorkspaceSession::open(&root).unwrap();
        let mut current = AuthoringIndex::load(&root, session.files(), &BTreeMap::new());
        for source in [
            "scene opening { background(room), \"new\" }",
            "scene broken {",
            "scene fixed { \"fixed\" }",
        ] {
            let changes = BTreeMap::from([(PathBuf::from("scripts/main.shou"), source.into())]);
            let next = current.with_sources(&changes);
            let fresh = AuthoringIndex::load(&root, session.files(), &changes);
            assert_eq!(next.scenes, fresh.scenes);
            assert_eq!(next.dialogues, fresh.dialogues);
            assert_eq!(next.variables, fresh.variables);
            assert_eq!(next.assets, fresh.assets);
            assert_eq!(next.problems, fresh.problems);
            // Resource references are queried by location, not contribution insertion order.
            let mut actual = next.asset_references.clone();
            let mut expected = fresh.asset_references.clone();
            actual.sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));
            expected.sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));
            assert_eq!(actual, expected);
            let cancelled = std::cell::Cell::new(0);
            assert!(
                current
                    .with_sources_cancellable(&changes, || {
                        cancelled.set(cancelled.get() + 1);
                        cancelled.get() > 1
                    })
                    .is_none()
            );
            current = next;
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn background_sprite_references_keep_the_declared_category() {
        let root = fixture();
        let path = PathBuf::from("scripts/main.shou");
        let source = "scene start { sprite(layer, room, layout: scene()), background(room) }";
        fs::write(root.join(&path), source).unwrap();
        let session = crate::workspace::WorkspaceSession::open(&root).unwrap();
        let index = AuthoringIndex::load(&root, session.files(), &BTreeMap::new());
        let room = index
            .assets
            .iter()
            .find(|asset| asset.id == "room")
            .unwrap();
        assert_eq!(room.kind, AssetKind::Background);
        assert_eq!(room.reference_count, 2);
        assert!(
            index
                .asset_references
                .iter()
                .all(|reference| reference.key == room.key())
        );
        assert!(
            !index
                .problems
                .iter()
                .any(|problem| problem.message.contains("Unknown"))
        );
        let changes = BTreeMap::from([(path, "scene start { sprite(layer, room) }".into())]);
        let incremental = index.with_sources(&changes);
        let fresh = AuthoringIndex::load(&root, session.files(), &changes);
        assert_eq!(incremental.asset_references, fresh.asset_references);
        assert_eq!(incremental.assets, fresh.assets);
        assert_eq!(
            incremental
                .assets
                .iter()
                .find(|asset| asset.id == "room")
                .unwrap()
                .reference_count,
            1
        );
        fs::remove_dir_all(root).unwrap();
    }

    fn fixture() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "keine-authoring-index-{}-{nonce}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
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
        let reference = index
            .asset_references
            .iter()
            .find(|reference| reference.key.id == "room")
            .unwrap();
        let source = fs::read_to_string(root.join(&reference.path)).unwrap();
        assert_eq!(source.get(reference.range.clone().unwrap()), Some("room"));
        assert!(
            index
                .problems
                .iter()
                .any(|problem| problem.message.contains("missing.webp"))
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_resource_errors_follow_exact_tokens_and_incremental_repairs() {
        let root = fixture();
        let path = PathBuf::from("scripts/main.shou");
        let source = "scene start {\n  \"中文\",\n  sprite(hero, rin),\n  background(\n    unknown\n  )\n}\n";
        let files = vec![WorkspaceFile {
            relative_path: path.clone(),
            size: 0,
            kind: crate::workspace::WorkspaceEntryKind::File,
        }];
        let index = AuthoringIndex::load(
            &root,
            &files,
            &BTreeMap::from([(path.clone(), source.into())]),
        );
        let errors = index
            .problems
            .iter()
            .filter(|problem| problem.path == path)
            .collect::<Vec<_>>();
        assert_eq!(errors.len(), 2, "{errors:?}");
        for (id, line, column) in [("rin", 3, 16), ("unknown", 5, 5)] {
            let reference = index
                .asset_references
                .iter()
                .find(|reference| reference.key.id == id)
                .unwrap();
            assert_eq!(source.get(reference.range.clone().unwrap()), Some(id));
            assert_eq!((reference.line, reference.column), (line, column));
            assert!(
                errors
                    .iter()
                    .any(|error| (error.line, error.column) == (line, column))
            );
        }
        let repaired = index.with_sources(&BTreeMap::from([(
            path.clone(),
            "scene start { background(room) }".into(),
        )]));
        assert!(!repaired.problems.iter().any(|problem| problem.path == path));
        assert!(
            repaired
                .problems
                .iter()
                .any(|problem| problem.path == Path::new("assets.yaml")
                    && problem.message.contains("missing.webp"))
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn asset_query_keeps_stable_identity_across_sort_and_filter() {
        let assets = [
            AssetEntry {
                kind: AssetKind::Background,
                id: "room".into(),
                path: "assets/background/room.webp".into(),
                tags: vec!["interior".into()],
                exists: true,
                reference_count: 2,
            },
            AssetEntry {
                kind: AssetKind::Figure,
                id: "rin".into(),
                path: "assets/figure/rin.webp".into(),
                tags: vec!["hero".into()],
                exists: true,
                reference_count: 1,
            },
        ];
        let mut query = AssetQuery {
            search: "hero".into(),
            ..Default::default()
        };
        assert_eq!(query.results(&assets)[0].key(), assets[1].key());
        query.search.clear();
        query.sort = AssetSort::References;
        assert_eq!(
            query
                .results(&assets)
                .iter()
                .map(|asset| asset.key())
                .collect::<Vec<_>>(),
            vec![assets[0].key(), assets[1].key()]
        );
        query.kind = Some(AssetKind::Figure);
        assert_eq!(query.results(&assets).len(), 1);
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
    fn empty_text_blocks_remain_editable_after_insertion() {
        let source = "scene start {\n  \"\",\n  \"\"\n}\n";
        let dialogues = dialogues_for_source(Path::new("scripts/main.shou"), source);
        assert_eq!(dialogues.len(), 2);
        assert!(dialogues.iter().all(|dialogue| dialogue.editable));
        assert_eq!(dialogues[0].line, 2);
        assert_eq!(dialogues[1].line, 3);
        assert_eq!(
            replace_dialogue_text(source, &dialogues[1], "Next").unwrap(),
            "scene start {\n  \"\",\n  \"Next\"\n}\n"
        );
    }

    #[test]
    fn block_line_break_round_trips_through_source() {
        let source = "scene start {\n  \"Line one\\nLine two\"\n}\n";
        let dialogues = dialogues_for_source(Path::new("scripts/main.shou"), source);
        assert_eq!(dialogues.len(), 1);
        assert_eq!(dialogues[0].text, "Line one\nLine two");
    }

    #[test]
    fn character_edits_preserve_unknown_fields_comments_and_other_roles() {
        let source = "# keep\ncharacters:\n  hero:\n    name: \"Old\" # author note\n    color: \"#BAEBFF\"\n    custom: keep\n    # between\n  friend:\n    name: \"Friend\"\nmetadata: keep\n";
        let character = CharacterEntry {
            id: "hero".into(),
            name: "少女".into(),
            avatar: Some("portrait".into()),
            expressions: BTreeMap::from([
                ("smile".into(), vec!["face".into()]),
                ("blink".into(), vec!["rest".into(), "closed".into()]),
            ]),
            ..Default::default()
        };
        let edited = edit_character(source, &character).unwrap();
        assert!(
            edited.contains("# author note")
                && edited.contains("custom: keep")
                && edited.contains("# between")
        );
        assert!(edited.ends_with("  friend:\n    name: \"Friend\"\nmetadata: keep\n"));
        let manifest = keine_core::config::EiyashouCharacterManifest::from_yaml(&edited).unwrap();
        assert_eq!(
            manifest.characters["hero"].expressions["blink"],
            ["rest", "closed"]
        );
        assert!(manifest.characters["hero"].color.is_none());
        let renamed = rename_character_asset(&edited, "rest", "neutral").unwrap();
        let parsed = keine_core::config::EiyashouCharacterManifest::from_yaml(&renamed).unwrap();
        assert_eq!(
            parsed.characters["hero"].expressions["blink"],
            ["neutral", "closed"]
        );
        assert!(renamed.contains("custom: keep") && renamed.contains("# author note"));
        let deleted = delete_character(&edited, "hero").unwrap();
        assert!(deleted.starts_with("# keep\ncharacters:\n"));
        assert!(deleted.contains("friend:") && !deleted.contains("hero:"));
        assert_eq!(
            delete_character(
                "characters:\n  hero:\n    name: Hero\nmetadata: keep\n",
                "hero"
            )
            .unwrap(),
            "characters: {}\nmetadata: keep\n"
        );
        assert!(edit_character("characters: { hero: { name: Hero } }", &character).is_err());
    }

    #[test]
    fn character_insertion_after_nested_block_preserves_following_sibling() {
        let source = "scene start {\n  if (true) {\n    \"inside\"\n  },\n  \"after\"\n}\n";
        let edited = insert_source_statement(source, 3, "sprite(hero, face)").unwrap();
        assert!(edited.contains("  },\n  sprite(hero, face),\n  \"after\""));
        assert!(
            parse_native_document(&edited).diagnostics.is_empty(),
            "{edited}: {:?}",
            parse_native_document(&edited).diagnostics
        );
        assert!(insert_source_statement(source, 5, "sprite(hero, face)").is_err());
    }

    #[test]
    fn character_creation_accepts_an_empty_root_manifest() {
        let source = append_character("{}\n", "hero", "Hero", None).unwrap();
        let source = edit_character(
            &source,
            &CharacterEntry {
                id: "hero".into(),
                name: "Hero".into(),
                avatar: Some("face".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let manifest = keine_core::config::EiyashouCharacterManifest::from_yaml(&source).unwrap();
        assert_eq!(manifest.characters["hero"].avatar.as_deref(), Some("face"));
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
        assert!(edited.ends_with("scene next {\n}\n"));
        let projected = EiyashouProjection::parse(&edited);
        assert_eq!(projected.scenes.len(), 2);
        assert!(projected.read_only.is_empty());
    }

    #[test]
    fn scene_edits_preserve_other_source_and_identify_real_targets() {
        let source =
            "// keep\nscene first { goto(second) }\n\n// between\nscene second { \"Hello\" }\n";
        let parsed = parse_native_document(source);
        let first = parsed.scenes[0].range.start;
        let second = parsed.scenes[1].range.start;
        let renamed = rename_scene(source, second, "ending").unwrap();
        assert!(renamed.contains("scene ending { \"Hello\" }"));
        assert!(renamed.contains("goto(ending)"));
        assert_eq!(scene_references(source, "second").len(), 1);
        assert!(scene_references("scene x { \"goto(second)\" }", "second").is_empty());
        let moved = move_scene(source, second, crate::projection::MoveDirection::Up).unwrap();
        assert!(moved.find("scene second").unwrap() < moved.find("scene first").unwrap());
        assert!(moved.contains("// between"));
        let deleted = delete_scene(source, first).unwrap();
        assert!(!deleted.contains("scene first"));
        assert!(deleted.contains("scene second"));
        let self_ref = "scene second { call(second) }";
        let renamed = rename_scene(self_ref, 0, "longer_ending").unwrap();
        assert_eq!(renamed, "scene longer_ending { call(longer_ending) }");
    }

    #[test]
    fn palette_inserts_after_a_stable_line_boundary() {
        let source = "scene start {\n  \"First\",\n}\n";
        let mut index = AuthoringIndex::default();
        index.characters.push(CharacterEntry {
            id: "rin".into(),
            name: "Rin".into(),
            color: None,
            ..Default::default()
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
    fn all_palette_templates_parse_and_project_as_editable_blocks() {
        let source = "scene start {\n  \"Ready\"\n}\n";
        let mut index = AuthoringIndex::default();
        index.characters.push(CharacterEntry {
            id: "rin".into(),
            name: "Rin".into(),
            color: None,
            ..Default::default()
        });
        index.scenes.push(SceneEntry {
            path: "scripts/main.shou".into(),
            name: "start".into(),
            line: 1,
            name_range: 6..11,
            source_range: 0..source.len(),
        });
        for (kind, id) in [
            (AssetKind::Background, "room"),
            (AssetKind::Figure, "hero"),
            (AssetKind::Voice, "voice"),
            (AssetKind::Bgm, "theme"),
            (AssetKind::Effect, "sound"),
            (AssetKind::Video, "movie"),
        ] {
            index.assets.push(AssetEntry {
                kind,
                id: id.into(),
                path: format!("assets/{id}").into(),
                tags: Vec::new(),
                exists: true,
                reference_count: 0,
            });
        }
        for kind in InsertKind::ALL {
            let edited = insert_statement(source, 1, kind, &index).unwrap();
            let document = parse_native_document(&edited);
            assert!(
                document
                    .diagnostics
                    .iter()
                    .all(|diagnostic| diagnostic.level != DiagnosticLevel::Error),
                "{kind:?}: {:?}\n{edited}",
                document.diagnostics
            );
            let projection = EiyashouProjection::parse(&edited);
            let inserted = projection.scenes[0].blocks.last().unwrap();
            assert!(!inserted.read_only, "{kind:?}: {inserted:?}");
            for block in &projection.scenes[0].blocks {
                if block.kind != projection::BlockKind::Command {
                    continue;
                }
                let command = block.summary.split('(').next().unwrap().trim();
                if commands::is_native_command(command)
                    || keine_loader::is_native_dotted_command(command)
                {
                    assert!(
                        keine_loader::native_command_argument_names(command).is_some(),
                        "{kind:?}: command must expose the Loader field inventory"
                    );
                }
            }
            assert!(projection.scenes[0].blocks.len() > 1, "{kind:?}");
            if kind == InsertKind::Native("sprite.update") {
                let parsed = keine_loader::parse_native_scenes(&edited);
                assert!(
                    parsed[0].report.actions.iter().any(|action| matches!(
                        action,
                        keine_core::Action::PatchSprite {
                            position: None,
                            layout: None,
                            scale: None,
                            ..
                        }
                    )),
                    "palette image updates must preserve the current pose"
                );
            }
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
