//! Inspector interaction state; document mutations stay in edits.
use crate::app::*;

pub(in crate::app) struct InspectorState {
    pub(in crate::app) inspector_key: Option<InspectorEditKey>,
    pub(in crate::app) inspector_inputs: Vec<Entity<InputState>>,
    pub(in crate::app) text_lifetime_inputs: Vec<Entity<InputState>>,
    pub(in crate::app) inspector_selects: Vec<Entity<SelectState<Vec<SourceOption>>>>,
    pub(in crate::app) inspector_subscriptions: Vec<Subscription>,
    pub(in crate::app) source_inspector_key: Option<SourceInspectorKey>,
    pub(in crate::app) source_inspector_inputs: Vec<Entity<InputState>>,
    pub(in crate::app) source_inspector_texts: Vec<Entity<TextareaState>>,
    pub(in crate::app) source_inspector_sliders: HashMap<String, Entity<SliderState>>,
    pub(in crate::app) source_inspector_selects:
        HashMap<String, Entity<SelectState<Vec<SourceOption>>>>,
    pub(in crate::app) source_inspector_subscriptions: Vec<Subscription>,
    pub(in crate::app) source_inspector_effect: Option<&'static str>,
    pub(in crate::app) source_position_bounds: Rc<RefCell<Bounds<Pixels>>>,
    pub(in crate::app) source_position_draft: Option<(usize, f32, f32)>,
    pub(in crate::app) asset_inspector_key: Option<(AssetKey, PathBuf, Vec<String>)>,
    pub(in crate::app) asset_inspector_inputs: Vec<Entity<InputState>>,
    pub(in crate::app) asset_rename_file: bool,
    pub(in crate::app) asset_batch_tags: Entity<InputState>,
    pub(in crate::app) batch_block_field: Option<String>,
    pub(in crate::app) batch_block_input: Entity<InputState>,
    pub(in crate::app) asset_inspector_subscriptions: Vec<Subscription>,
}
