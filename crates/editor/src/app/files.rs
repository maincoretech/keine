use super::*;

#[derive(Default)]
pub(super) struct FileHistory {
    undo: VecDeque<FileEdit>,
    redo: Vec<FileEdit>,
}

struct ManifestChange {
    path: PathBuf,
    before: String,
    after: String,
}

impl ManifestChange {
    fn between(
        before: Option<(PathBuf, String)>,
        after: Option<(PathBuf, String)>,
    ) -> Option<Self> {
        match (before, after) {
            (Some((path, before)), Some((after_path, after)))
                if path == after_path && before != after =>
            {
                Some(Self {
                    path,
                    before,
                    after,
                })
            }
            _ => None,
        }
    }
}

enum FileEdit {
    Relocate {
        before: PathBuf,
        after: PathBuf,
    },
    Toggle {
        paths: Vec<(PathBuf, Option<PathBuf>)>,
        after_present: bool,
        present: bool,
        manifest: Option<ManifestChange>,
    },
}

impl FileEdit {
    fn affected_paths(&self) -> Vec<&Path> {
        match self {
            Self::Relocate { before, after } => vec![before, after],
            Self::Toggle { paths, .. } => paths.iter().map(|(path, _)| path.as_path()).collect(),
        }
    }

    fn replay(&mut self, root: &Path, undo: bool) -> io::Result<ImportResult> {
        match self {
            Self::Relocate { before, after } => {
                let (source, destination) = if undo {
                    (after, before)
                } else {
                    (before, after)
                };
                file_ops::relocate_entry(root, source, destination)
            }
            Self::Toggle {
                paths,
                after_present,
                present,
                manifest,
            } => {
                let target_present = if undo {
                    !*after_present
                } else {
                    *after_present
                };
                if target_present == *present {
                    return Err(io::Error::other("File history is out of sequence"));
                }
                if let Some(change) = manifest.as_ref() {
                    let (_, current) = file_ops::asset_manifest_source(root)?;
                    let expected = if *present {
                        &change.after
                    } else {
                        &change.before
                    };
                    if &current != expected {
                        return Err(io::Error::other(
                            "Asset manifest changed; refresh before undoing",
                        ));
                    }
                }
                for (moved, (path, stash)) in paths.iter_mut().enumerate() {
                    let result = if target_present {
                        file_ops::restore_stashed_entry(
                            root,
                            stash
                                .as_deref()
                                .ok_or_else(|| io::Error::other("Undo data missing"))?,
                            path,
                        )
                    } else if let Some(stash) = stash.as_deref() {
                        file_ops::restash_entry(root, path, stash)
                    } else {
                        file_ops::stash_entry(root, path).map(|new_stash| {
                            *stash = Some(new_stash);
                        })
                    };
                    if let Err(error) = result {
                        rollback_toggled_paths(root, &paths[..moved], target_present);
                        return Err(error);
                    }
                }
                let manifest_update = if let Some(change) = manifest.as_ref() {
                    let (expected, replacement) = if target_present {
                        (&change.before, &change.after)
                    } else {
                        (&change.after, &change.before)
                    };
                    if let Err(error) =
                        file_ops::replace_asset_manifest(root, &change.path, expected, replacement)
                    {
                        rollback_toggled_paths(root, paths, target_present);
                        return Err(error);
                    }
                    Some((change.path.clone(), replacement.clone()))
                } else {
                    None
                };
                *present = target_present;
                Ok(ImportResult {
                    destination: paths
                        .first()
                        .map_or_else(PathBuf::new, |(path, _)| path.clone()),
                    manifest_update,
                    registered: false,
                })
            }
        }
    }
}

fn rollback_toggled_paths(root: &Path, paths: &[(PathBuf, Option<PathBuf>)], became_present: bool) {
    for (path, stash) in paths.iter().rev() {
        if let Some(stash) = stash {
            let _ = if became_present {
                file_ops::restash_entry(root, path, stash)
            } else {
                file_ops::restore_stashed_entry(root, stash, path)
            };
        }
    }
}

impl FileHistory {
    fn record(&mut self, edit: FileEdit) {
        self.undo.push_back(edit);
        if self.undo.len() > 32 {
            self.undo.pop_front();
        }
        self.redo.clear();
    }

    fn step(&mut self, root: &Path, undo: bool) -> io::Result<Option<ImportResult>> {
        let Some(mut edit) = (if undo {
            self.undo.pop_back()
        } else {
            self.redo.pop()
        }) else {
            return Ok(None);
        };
        match edit.replay(root, undo) {
            Ok(result) => {
                if undo {
                    self.redo.push(edit);
                } else {
                    self.undo.push_back(edit);
                }
                Ok(Some(result))
            }
            Err(error) => {
                if undo {
                    self.undo.push_back(edit);
                } else {
                    self.redo.push(edit);
                }
                Err(error)
            }
        }
    }

    fn next_requires_manifest(&self, undo: bool) -> bool {
        let edit = if undo {
            self.undo.back()
        } else {
            self.redo.last()
        };
        match edit {
            Some(FileEdit::Relocate { .. }) => true,
            Some(FileEdit::Toggle { manifest, .. }) => manifest.is_some(),
            None => false,
        }
    }

    fn next_affected_paths(&self, undo: bool) -> Vec<&Path> {
        (if undo {
            self.undo.back()
        } else {
            self.redo.last()
        })
        .map_or_else(Vec::new, FileEdit::affected_paths)
    }
}

impl WorkbenchPanel {
    pub(super) fn undo_files(
        &mut self,
        _: &UndoFiles,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replay_file_history(true, window, cx);
    }

    pub(super) fn redo_files(
        &mut self,
        _: &RedoFiles,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replay_file_history(false, window, cx);
    }

    fn replay_file_history(&mut self, undo: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.focus.is_focused(window) {
            return;
        }
        let Some(root) = self.explorer_root() else {
            return;
        };
        if self.file_history.next_requires_manifest(undo)
            && !self.manifest_mutation_ready(&root, window, cx)
        {
            return;
        }
        if !self.affected_files_closed(
            &root,
            &self.file_history.next_affected_paths(undo),
            window,
            cx,
        ) {
            return;
        }
        match self.file_history.step(&root, undo) {
            Ok(Some(result)) => {
                self.accept_file_result(&root, result, window, cx);
                self.file_selection = None;
                cx.global_mut::<EditorDocuments>()
                    .set_notice(&root, if undo { "File undo" } else { "File redo" });
                cx.notify();
            }
            Ok(None) => {}
            Err(error) => window.push_notification(Notification::error(short_error(&error)), cx),
        }
    }

    pub(super) fn explorer_root(&self) -> Option<PathBuf> {
        match &self.content {
            PanelContent::Explorer { root, .. } => Some(root.clone()),
            _ => None,
        }
    }

    pub(super) fn selected_directory(&self) -> PathBuf {
        let PanelContent::Explorer { files, .. } = &self.content else {
            return PathBuf::new();
        };
        let Some(selected) = self.file_selection.as_ref() else {
            return PathBuf::new();
        };
        if files
            .iter()
            .find(|file| &file.relative_path == selected)
            .is_some_and(WorkspaceFile::is_dir)
        {
            selected.clone()
        } else {
            selected
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .to_owned()
        }
    }

    pub(super) fn begin_file_edit(
        &mut self,
        mode: FileEditMode,
        initial: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.file_name_input
            .update(cx, |input, cx| input.set_value(initial, window, cx));
        self.file_edit = Some(mode);
        self.file_name_input
            .update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    pub(super) fn open_file_context_menu(
        &mut self,
        path: PathBuf,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.file_context_epoch = self.file_context_epoch.wrapping_add(1);
        self.file_selection = Some(path.clone());
        self.file_context_menu = Some(FileContextMenu {
            path: Some(path),
            position,
            epoch: self.file_context_epoch,
            closing: false,
        });
        cx.notify();
    }

    pub(super) fn open_explorer_menu(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.file_context_epoch = self.file_context_epoch.wrapping_add(1);
        self.file_context_menu = Some(FileContextMenu {
            path: None,
            position,
            epoch: self.file_context_epoch,
            closing: false,
        });
        cx.notify();
    }

    pub(super) fn close_file_context_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.file_context_menu.as_mut() else {
            return;
        };
        if menu.closing {
            return;
        }
        self.file_context_epoch = self.file_context_epoch.wrapping_add(1);
        menu.epoch = self.file_context_epoch;
        menu.closing = true;
        let epoch = menu.epoch;
        let delay = if cx.reduce_motion() {
            Duration::ZERO
        } else {
            Duration::from_millis(90)
        };
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update_in(cx, |this, _, cx| {
                if this
                    .file_context_menu
                    .as_ref()
                    .is_some_and(|menu| menu.epoch == epoch && menu.closing)
                {
                    this.file_context_menu = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn commit_file_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.explorer_root() else {
            return;
        };
        let Some(mode) = self.file_edit.clone() else {
            return;
        };
        let name = self.file_name_input.read(cx).value().trim().to_owned();
        let result = match mode {
            FileEditMode::NewFile { parent } => {
                file_ops::create_file(&root, &parent, &name).map(|path| {
                    (
                        None,
                        FileEdit::Toggle {
                            paths: vec![(path, None)],
                            after_present: true,
                            present: true,
                            manifest: None,
                        },
                    )
                })
            }
            FileEditMode::NewFolder { parent } => file_ops::create_directory(&root, &parent, &name)
                .map(|path| {
                    (
                        None,
                        FileEdit::Toggle {
                            paths: vec![(path, None)],
                            after_present: true,
                            present: true,
                            manifest: None,
                        },
                    )
                }),
            FileEditMode::Rename { path } => {
                if !self.manifest_mutation_ready(&root, window, cx) {
                    return;
                }
                if !self.affected_files_closed(&root, &[&path], window, cx) {
                    return;
                }
                file_ops::rename_entry(&root, &path, &name).map(|result| {
                    let edit = FileEdit::Relocate {
                        before: path,
                        after: result.destination.clone(),
                    };
                    (Some(result), edit)
                })
            }
        };
        match result {
            Ok((update, edit)) => {
                self.file_edit = None;
                self.file_history.record(edit);
                if let Some(update) = update {
                    self.accept_file_result(&root, update, window, cx);
                } else {
                    self.refresh_explorer(&root, cx);
                }
                self.focus.focus(window, cx);
            }
            Err(error) => window.push_notification(Notification::error(short_error(&error)), cx),
        }
        cx.notify();
    }

    pub(super) fn manifest_mutation_ready(
        &self,
        root: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if cx
            .global_mut::<EditorDocuments>()
            .asset_manifest_is_clean(root)
        {
            true
        } else {
            window.push_notification(Notification::warning("Save assets.yaml first"), cx);
            false
        }
    }

    fn affected_files_closed(
        &self,
        root: &Path,
        paths: &[&Path],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let documents = cx.global_mut::<EditorDocuments>();
        if paths
            .iter()
            .any(|path| documents.has_open_documents_under(root, path))
        {
            window.push_notification(Notification::warning("Save and close affected files"), cx);
            false
        } else {
            true
        }
    }

    pub(super) fn accept_file_result(
        &mut self,
        root: &Path,
        result: ImportResult,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((relative, source)) = result.manifest_update {
            match cx.global_mut::<EditorDocuments>().adopt_manifest_update(
                root,
                &relative,
                source.clone(),
            ) {
                Ok(Some(editor)) => {
                    let _ = editor.update(cx, |editor, cx| {
                        editor.replace_all(source, window, cx);
                    });
                }
                Ok(None) => {}
                Err(error) => {
                    window.push_notification(Notification::error(short_error(&error)), cx)
                }
            }
        }
        self.refresh_explorer(root, cx);
    }

    pub(super) fn refresh_explorer(&mut self, root: &Path, cx: &mut Context<Self>) {
        match cx.global_mut::<EditorDocuments>().refresh_files(root) {
            Ok(refreshed) => {
                if let PanelContent::Explorer { files, .. } = &mut self.content {
                    *files = refreshed;
                }
            }
            Err(error) => cx
                .global_mut::<EditorDocuments>()
                .set_notice(root, format!("File refresh failed: {error}")),
        }
        cx.refresh_windows();
    }

    pub(super) fn paste_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.explorer_root() else {
            return;
        };
        let Some(source) = self.file_clipboard.clone() else {
            return;
        };
        if !self.manifest_mutation_ready(&root, window, cx) {
            return;
        }
        if !self.affected_files_closed(&root, &[&source], window, cx) {
            return;
        }
        let target = self.selected_directory();
        let manifest_before = match file_ops::asset_manifest_source(&root) {
            Ok(source) => source,
            Err(error) => {
                window.push_notification(Notification::error(short_error(&error)), cx);
                return;
            }
        };
        match file_ops::copy_entry(&root, &source, &target) {
            Ok(results) => {
                let copied = target.join(source.file_name().unwrap_or_default());
                let manifest_after = results
                    .iter()
                    .rev()
                    .find_map(|result| result.manifest_update.clone());
                self.file_history.record(FileEdit::Toggle {
                    paths: vec![(copied, None)],
                    after_present: true,
                    present: true,
                    manifest: ManifestChange::between(Some(manifest_before), manifest_after),
                });
                for result in results {
                    self.accept_file_result(&root, result, window, cx);
                }
                self.refresh_explorer(&root, cx);
                self.focus.focus(window, cx);
                window.push_notification(Notification::success("Copied"), cx);
            }
            Err(error) => window.push_notification(Notification::error(short_error(&error)), cx),
        }
    }

    pub(super) fn move_file(
        &mut self,
        source: &Path,
        target: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.explorer_root() else {
            return;
        };
        if !self.manifest_mutation_ready(&root, window, cx) {
            return;
        }
        if !self.affected_files_closed(&root, &[source], window, cx) {
            return;
        }
        match file_ops::move_entry(&root, source, target) {
            Ok(result) => {
                self.file_history.record(FileEdit::Relocate {
                    before: source.to_owned(),
                    after: result.destination.clone(),
                });
                self.file_collapsed.remove(target);
                self.accept_file_result(&root, result, window, cx);
                self.focus.focus(window, cx);
            }
            Err(error) => window.push_notification(Notification::error(short_error(&error)), cx),
        }
    }

    pub(super) fn confirm_delete_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.explorer_root() else {
            return;
        };
        let Some(path) = self.file_selection.clone() else {
            return;
        };
        let receiver = window.prompt(
            PromptLevel::Warning,
            "Delete selected item?",
            Some(&path.display().to_string()),
            &[
                PromptButton::Other("Delete".into()),
                PromptButton::Cancel("Cancel".into()),
            ],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if receiver.await.ok() != Some(0) {
                return;
            }
            let _ = this.update_in(cx, |this, window, cx| {
                if !this.affected_files_closed(&root, &[&path], window, cx) {
                    return;
                }
                match file_ops::stage_deleted_entry(&root, &path) {
                    Ok(stash) => {
                        this.file_history.record(FileEdit::Toggle {
                            paths: vec![(path, Some(stash))],
                            after_present: false,
                            present: false,
                            manifest: None,
                        });
                        this.file_selection = None;
                        this.refresh_explorer(&root, cx);
                        this.focus.focus(window, cx);
                        window.push_notification(Notification::success("Deleted"), cx);
                    }
                    Err(error) => {
                        window.push_notification(Notification::error(short_error(&error)), cx)
                    }
                }
            });
        })
        .detach();
    }

    pub(super) fn start_external_import(
        &mut self,
        paths: Vec<PathBuf>,
        target: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.explorer_root() else {
            return;
        };
        if paths.is_empty() || !self.manifest_mutation_ready(&root, window, cx) {
            return;
        }
        self.file_progress = Some(FileProgress {
            completed: 0,
            total: paths.len(),
        });
        let total = paths.len();
        let manifest_before = match file_ops::asset_manifest_source(&root) {
            Ok(source) => source,
            Err(error) => {
                self.file_progress = None;
                window.push_notification(Notification::error(short_error(&error)), cx);
                return;
            }
        };
        let background = cx.background_executor().clone();
        cx.spawn_in(window, async move |this, cx| {
            let mut succeeded = 0;
            let mut failed = 0;
            let mut last_manifest = None;
            let mut imported = Vec::new();
            for (index, source) in paths.into_iter().enumerate() {
                let root_for_task = root.clone();
                let target_for_task = target.clone();
                let result = background
                    .spawn(async move {
                        file_ops::import_external(&root_for_task, &target_for_task, &source)
                    })
                    .await;
                match result {
                    Ok(result) => {
                        succeeded += 1;
                        imported.push((result.destination.clone(), None));
                        if result.manifest_update.is_some() {
                            last_manifest = result.manifest_update;
                        }
                    }
                    Err(_) => failed += 1,
                }
                let _ = this.update_in(cx, |this, _, cx| {
                    this.file_progress = Some(FileProgress {
                        completed: index + 1,
                        total,
                    });
                    cx.notify();
                });
            }
            let _ = this.update_in(cx, |this, window, cx| {
                this.file_progress = None;
                if !imported.is_empty() {
                    this.file_history.record(FileEdit::Toggle {
                        paths: imported,
                        after_present: true,
                        present: true,
                        manifest: ManifestChange::between(
                            Some(manifest_before),
                            last_manifest.clone(),
                        ),
                    });
                }
                if let Some((relative, source)) = last_manifest {
                    let result = ImportResult {
                        destination: PathBuf::new(),
                        manifest_update: Some((relative, source)),
                        registered: true,
                    };
                    this.accept_file_result(&root, result, window, cx);
                } else {
                    this.refresh_explorer(&root, cx);
                }
                if failed == 0 {
                    window.push_notification(
                        Notification::success(format!("Imported {succeeded}")),
                        cx,
                    );
                } else {
                    window.push_notification(
                        Notification::warning(format!("Imported {succeeded} · {failed} failed")),
                        cx,
                    );
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

#[cfg(test)]
mod history_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    fn fixture() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "keine-file-history-{}-{nonce}-{}",
            std::process::id(),
            FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir_all(root.join("destination")).unwrap();
        fs::write(
            root.join("config.yaml"),
            "adapter:\n  script: keine\nscript:\n  assets: assets.yaml\n  characters: characters.yaml\n",
        )
        .unwrap();
        fs::write(root.join("assets.yaml"), "backgrounds: {}\n").unwrap();
        fs::write(root.join("characters.yaml"), "characters: {}\n").unwrap();
        root
    }

    #[test]
    fn moved_file_undoes_and_redoes() {
        let root = fixture();
        fs::write(root.join("note.md"), "text").unwrap();
        let moved =
            file_ops::move_entry(&root, Path::new("note.md"), Path::new("destination")).unwrap();
        let mut history = FileHistory::default();
        history.record(FileEdit::Relocate {
            before: "note.md".into(),
            after: moved.destination,
        });
        history.step(&root, true).unwrap();
        assert!(root.join("note.md").exists());
        history.step(&root, false).unwrap();
        assert!(root.join("destination/note.md").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn created_file_is_stashed_and_restored() {
        let root = fixture();
        fs::write(root.join("note.md"), "draft").unwrap();
        let mut history = FileHistory::default();
        history.record(FileEdit::Toggle {
            paths: vec![("note.md".into(), None)],
            after_present: true,
            present: true,
            manifest: None,
        });
        history.step(&root, true).unwrap();
        assert!(!root.join("note.md").exists());
        history.step(&root, false).unwrap();
        assert_eq!(fs::read_to_string(root.join("note.md")).unwrap(), "draft");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn imported_file_and_asset_manifest_undo_as_one_step() {
        let root = fixture();
        let before = fs::read_to_string(root.join("assets.yaml")).unwrap();
        let after = "backgrounds:\n  sky: assets/sky.webp\n".to_owned();
        fs::create_dir(root.join("assets")).unwrap();
        fs::write(root.join("assets/sky.webp"), "media").unwrap();
        fs::write(root.join("assets.yaml"), &after).unwrap();
        let mut history = FileHistory::default();
        history.record(FileEdit::Toggle {
            paths: vec![("assets/sky.webp".into(), None)],
            after_present: true,
            present: true,
            manifest: ManifestChange::between(
                Some(("assets.yaml".into(), before.clone())),
                Some(("assets.yaml".into(), after.clone())),
            ),
        });

        history.step(&root, true).unwrap();
        assert!(!root.join("assets/sky.webp").exists());
        assert_eq!(
            fs::read_to_string(root.join("assets.yaml")).unwrap(),
            before
        );
        history.step(&root, false).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("assets/sky.webp")).unwrap(),
            "media"
        );
        assert_eq!(fs::read_to_string(root.join("assets.yaml")).unwrap(), after);

        fs::write(
            root.join("assets.yaml"),
            "backgrounds: {}\n# external edit\n",
        )
        .unwrap();
        assert!(history.step(&root, true).is_err());
        assert!(root.join("assets/sky.webp").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
