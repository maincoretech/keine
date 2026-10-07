//! Picker interaction state; document mutations stay in edits.
use crate::app::*;

pub(in crate::app) struct PickerState {
    pub(in crate::app) picker_drag: Option<crate::app::blocks::PickerDragSession>,
    pub(in crate::app) picker_row_bounds: Rc<RefCell<HashMap<usize, Bounds<Pixels>>>>,
    pub(in crate::app) picker_scroll: ScrollHandle,
    pub(in crate::app) block_picker_open: bool,
    pub(in crate::app) block_picker_closing: bool,
    pub(in crate::app) block_picker_epoch: usize,
    pub(in crate::app) block_picker_index: usize,
    pub(in crate::app) block_picker_category: Option<&'static str>,
    pub(in crate::app) block_picker_customize: bool,
    pub(in crate::app) block_picker_input: Entity<InputState>,
}
