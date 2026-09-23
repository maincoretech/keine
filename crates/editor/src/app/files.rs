use super::*;

impl WorkbenchPanel {
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
                file_ops::create_file(&root, &parent, &name).map(|_| None)
            }
            FileEditMode::NewFolder { parent } => {
                file_ops::create_directory(&root, &parent, &name).map(|_| None)
            }
            FileEditMode::Rename { path } => {
                if !self.manifest_mutation_ready(&root, window, cx) {
                    return;
                }
                file_ops::rename_entry(&root, &path, &name).map(Some)
            }
        };
        match result {
            Ok(update) => {
                self.file_edit = None;
                if let Some(update) = update {
                    self.accept_file_result(&root, update, window, cx);
                } else {
                    self.refresh_explorer(&root, cx);
                }
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
        let target = self.selected_directory();
        match file_ops::copy_entry(&root, &source, &target) {
            Ok(results) => {
                for result in results {
                    self.accept_file_result(&root, result, window, cx);
                }
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
        match file_ops::move_entry(&root, source, target) {
            Ok(result) => self.accept_file_result(&root, result, window, cx),
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
                match file_ops::delete_entry(&root, &path) {
                    Ok(()) => {
                        this.file_selection = None;
                        this.refresh_explorer(&root, cx);
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
        let background = cx.background_executor().clone();
        cx.spawn_in(window, async move |this, cx| {
            let mut succeeded = 0;
            let mut failed = 0;
            let mut last_manifest = None;
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
