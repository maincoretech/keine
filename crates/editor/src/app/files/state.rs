//! Explorer interaction state; document mutations stay in edits.
use super::FileHistory;
use crate::app::*;

pub(in crate::app) struct ExplorerState {
    pub(in crate::app) file_selection: Option<PathBuf>,
    pub(in crate::app) file_drop_target: Option<(PathBuf, Bounds<Pixels>)>,
    pub(in crate::app) file_clipboard: Option<PathBuf>,
    pub(in crate::app) file_history: FileHistory,
    pub(in crate::app) file_edit: Option<FileEditMode>,
    pub(in crate::app) file_name_input: Entity<InputState>,
    pub(in crate::app) file_progress: Option<FileProgress>,
    pub(in crate::app) file_context_menu: Option<FileContextMenu>,
    pub(in crate::app) file_context_epoch: u64,
}
