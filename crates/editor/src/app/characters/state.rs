//! Characters interaction state; document mutations stay in edits.
use crate::app::*;

pub(in crate::app) struct CharactersState {
    pub(in crate::app) tool_inputs: Vec<Entity<InputState>>,
    pub(in crate::app) character_id: Option<String>,
    pub(in crate::app) character_expression: Option<String>,
    pub(in crate::app) character_script: Option<(PathBuf, usize)>,
    pub(in crate::app) character_images: Option<Entity<SelectState<Vec<SourceOption>>>>,
    pub(in crate::app) character_image_options: Vec<SourceOption>,
}
