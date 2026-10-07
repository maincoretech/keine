//! Assets interaction state; document mutations stay in edits.
use crate::app::*;

pub(in crate::app) struct AssetsState {
    pub(in crate::app) asset_search: Entity<InputState>,
    pub(in crate::app) asset_kind: Option<AssetKind>,
    pub(in crate::app) asset_folder: Option<PathBuf>,
    pub(in crate::app) asset_sort: AssetSort,
    pub(in crate::app) asset_tag: Option<String>,
    pub(in crate::app) asset_status: crate::authoring::AssetStatus,
    pub(in crate::app) asset_size: crate::authoring::AssetSize,
    pub(in crate::app) asset_modified: Option<Duration>,
    pub(in crate::app) asset_grid: Option<bool>,
    pub(in crate::app) asset_large: bool,
    pub(in crate::app) asset_statistics_expanded: bool,
    pub(in crate::app) asset_browser: RefCell<crate::app::resource::browse::Cache>,
    pub(in crate::app) asset_thumbnails: Entity<crate::app::resource::thumbnail::Thumbnails>,
    pub(in crate::app) asset_unmapped: bool,
    pub(in crate::app) asset_anchor: Option<AssetKey>,
    pub(in crate::app) asset_filter_menu: Option<AssetFilterMenu>,
    pub(in crate::app) asset_filter_epoch: u64,
}
