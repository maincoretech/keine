//! Gallery state and components.
use super::*;

#[derive(Resource)]
pub(crate) struct ExtraUi {
    pub(crate) open: bool,
    pub(super) page: usize,
    pub(super) section: ExtraSection,
    pub(super) paused_stage_bgm: Vec<Entity>,
    pub(super) selected_bgm: Option<String>,
}

impl Default for ExtraUi {
    fn default() -> Self {
        Self {
            open: false,
            page: 1,
            section: ExtraSection::Cg,
            paused_stage_bgm: Vec::new(),
            selected_bgm: None,
        }
    }
}

#[derive(Component)]
pub(crate) struct ExtraRoot;

#[derive(Component)]
pub(crate) struct ExtraBlurProxy;

#[derive(Component)]
pub(crate) struct ExtraWatermark;

#[derive(Component)]
pub(crate) struct ExtraClose;

#[derive(Component)]
pub(crate) struct ExtraPage(pub(super) i32);

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ExtraSection {
    #[default]
    Cg,
    Music,
}

#[derive(Component)]
pub(crate) struct ExtraTab {
    pub(super) section: ExtraSection,
}

#[derive(Component)]
pub(crate) struct ExtraButtonVisual;

#[derive(Component)]
pub(crate) struct ExtraPanel(pub(super) ExtraSection);
#[derive(Component)]
pub(crate) struct ExtraPageLabel;
#[derive(Resource, Default)]
pub(crate) struct ExtraPageTransition {
    pub(super) from: Option<ExtraSection>,
    pub(super) elapsed: f32,
    pub(super) cg_direction: f32,
}
impl ExtraPageTransition {
    pub(crate) fn is_animating(&self) -> bool {
        self.from.is_some() || self.cg_direction != 0.0
    }
}
#[derive(Component, Default)]
pub(crate) struct ExtraBgmList {
    pub(super) target: f32,
}
impl ExtraBgmList {
    pub(crate) fn is_animating(&self, position: &ScrollPosition) -> bool {
        (self.target - position.y).abs() > 0.01
    }
}
#[derive(Component)]
pub(crate) struct ExtraBgmStatus;
#[derive(Component)]
pub(crate) struct ExtraImageStatus(pub(super) bevy::asset::UntypedHandle);

#[derive(Component)]
pub(crate) struct ExtraThumbnail(pub(super) Handle<crate::scene::images::GalleryThumbnail>);
#[derive(Component)]
pub(crate) struct ExtraBgmBubble;
#[derive(Component)]
pub(crate) struct ExtraBgmBubbleText;

#[derive(Component)]
pub(crate) struct ExtraCg(pub(super) String);

#[derive(Component)]
pub(crate) struct ExtraCgGrid {
    pub(super) outgoing: bool,
}

#[derive(Component)]
pub(crate) struct ExtraFullCg(pub(super) String);

#[derive(Component, Clone, Copy)]
pub(crate) enum ExtraCgControl {
    Previous,
    Next,
    Close,
}

#[derive(Component)]
pub(crate) struct ExtraBgm(pub(super) String);

#[derive(Component)]
pub(crate) struct ExtraBgmPlayer {
    pub(super) duration: Option<Duration>,
    pub(super) observed_audio: bool,
    pub(super) failed: bool,
}

impl ExtraBgmPlayer {
    pub(crate) fn needs_frames(&self, sink: Option<&AudioSink>) -> bool {
        !self.failed && sink.is_none_or(|sink| !sink.is_paused())
    }
}

#[derive(Component)]
pub(crate) struct ExtraBgmTime;

#[derive(Component)]
pub(crate) struct ExtraBgmName;

#[derive(Component)]
pub(crate) struct ExtraBgmProgressThumb(pub(super) f32);
impl ExtraBgmProgressThumb {
    pub(crate) fn is_animating(&self, active: bool) -> bool {
        (self.0 - if active { 12.0 } else { 10.0 }).abs() > 0.001
    }
}

#[derive(Component)]
pub(crate) struct ExtraBgmPlayIcon;

#[derive(Component, Default)]
pub(crate) struct ExtraBgmSeekBar {
    pub(super) dragging: bool,
    pub(super) touch: bool,
    pub(super) preview: Option<Duration>,
}

impl ExtraBgmSeekBar {
    pub(crate) fn is_dragging(&self) -> bool {
        self.dragging
    }

    pub(super) fn reset(&mut self) {
        self.dragging = false;
        self.touch = false;
        self.preview = None;
    }
}

#[derive(Component, Clone, Copy)]
pub(crate) enum ExtraBgmControl {
    Previous,
    Play,
    Next,
    Stop,
}

#[derive(Component)]
pub(crate) struct ExtraMotion {
    pub(super) current: f32,
    pub(super) target: f32,
}

impl ExtraMotion {
    pub(crate) fn is_animating(&self) -> bool {
        (self.current - self.target).abs() > 0.001
    }
}

pub(super) fn ordered_cg(state: &GameState) -> Vec<(String, String)> {
    let mut ordered = state
        .unlocked_cg
        .iter()
        .map(|(file, name)| (file.clone(), name.clone()))
        .collect::<Vec<_>>();
    ordered.sort_unstable_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
    ordered
}
