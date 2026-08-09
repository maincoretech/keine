// Shared types for the VN engine.

use serde::{Deserialize, Serialize};

/// Design resolution (fixed, everything is drawn in this space).
/// Native logical canvas used by scene layout, UI and render effects.
pub const DESIGN_WIDTH: f32 = 1920.0;
pub const DESIGN_HEIGHT: f32 = 1080.0;

/// Position anchors, inspired by WebGAL's -left/-right/-center system.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Anchor {
    /// Offset from left edge (in design pixels).
    Left(f32),
    /// Offset from right edge.
    Right(f32),
    /// Centered horizontally with optional offset.
    Center(f32),
}

impl Anchor {
    /// Resolve anchor to an absolute x position within the design width.
    pub fn resolve(self, object_width: f32) -> f32 {
        match self {
            Anchor::Left(offset) => offset,
            Anchor::Right(offset) => DESIGN_WIDTH - object_width - offset,
            Anchor::Center(offset) => (DESIGN_WIDTH - object_width) / 2.0 + offset,
        }
    }
}

/// A 2D position in design-space coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub x: Anchor,
    pub y: f32,
}

impl Position {
    pub fn left(y: f32) -> Self {
        Self {
            x: Anchor::Left(0.0),
            y,
        }
    }
    pub fn right(y: f32) -> Self {
        Self {
            x: Anchor::Right(0.0),
            y,
        }
    }
    pub fn center(y: f32) -> Self {
        Self {
            x: Anchor::Center(0.0),
            y,
        }
    }
}

/// How a sprite is laid out before its authored transform is applied.
///
/// Normal figures use keine's character baseline. Editor scene layers use
/// the source editor's explicit canvas fitting and anchor rules instead.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum SpriteLayout {
    #[default]
    Natural,
    /// Height as a fraction of the fixed design viewport.
    ViewportHeight(f32),
    Scene(SceneLayerLayout),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SceneFit {
    Cover,
    Contain,
    ByWidth,
    #[default]
    ByHeight,
    Stretch,
    Center,
}

/// Layout shared by every image in one editor-authored scene.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SceneLayerLayout {
    pub fit: SceneFit,
    /// Top-left-origin position in the 1920x1080 design canvas.
    pub position: [f32; 2],
    /// Normalized pivot: top-left is `[0, 0]`, center is `[0.5, 0.5]`.
    pub anchor: [f32; 2],
    /// Optional explicit design-space size.
    pub size: Option<[f32; 2]>,
}

impl Default for SceneLayerLayout {
    fn default() -> Self {
        Self {
            fit: SceneFit::ByHeight,
            position: [0.0, 0.0],
            anchor: [0.0, 0.0],
            size: None,
        }
    }
}

/// Transition / enter animation kind.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum Transition {
    /// Instant (no animation).
    #[default]
    Instant,
    /// Fade in/out over `duration` seconds.
    Fade(f32),
    /// Slide from left over `duration` seconds.
    SlideFromLeft(f32),
    /// Slide from right over `duration` seconds.
    SlideFromRight(f32),
    /// Crossfade between scenes over `duration` seconds.
    Crossfade(f32),
    /// Reveal from left to right without stretching the source image.
    Wipe(f32),
    /// Noise-threshold dissolve over `duration` seconds.
    Dissolve(f32),
}

impl Transition {
    pub const fn duration(self) -> Option<f32> {
        match self {
            Self::Instant => None,
            Self::Fade(duration)
            | Self::SlideFromLeft(duration)
            | Self::SlideFromRight(duration)
            | Self::Crossfade(duration)
            | Self::Wipe(duration)
            | Self::Dissolve(duration) => Some(duration),
        }
    }
}

/// Compact animation vocabulary shared by script adapters and presentation backends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AnimationPreset {
    #[default]
    Enter,
    Exit,
    Shake,
    EnterFromBottom,
    EnterFromLeft,
    EnterFromRight,
    MoveFrontAndBack,
    Blur,
    OldFilm,
    DotFilm,
    ReflectionFilm,
    GlitchFilm,
    RgbFilm,
    GodrayFilm,
    RemoveFilm,
    ShockwaveIn,
    ShockwaveOut,
    Custom(String),
}

/// Persistent film effects attached to one stage object.
///
/// WebGAL models these as independent boolean properties, so several effects
/// may be enabled at once and `removeFilm` clears the complete set. Keeping the
/// state as a compact bit set preserves those semantics without allocating a
/// collection for every background and sprite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct FilmEffects(u8);

impl FilmEffects {
    pub const OLD_FILM: u8 = 1 << 0;
    pub const DOT_FILM: u8 = 1 << 1;
    pub const REFLECTION_FILM: u8 = 1 << 2;
    pub const GLITCH_FILM: u8 = 1 << 3;
    pub const RGB_FILM: u8 = 1 << 4;
    pub const GODRAY_FILM: u8 = 1 << 5;

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Returns whether this set changes as presentation time advances.
    ///
    /// Dot film is intentionally absent: it is a static screen pattern and
    /// does not justify keeping an otherwise idle renderer awake.
    pub const fn is_time_varying(self) -> bool {
        const TIME_VARYING: u8 = FilmEffects::OLD_FILM
            | FilmEffects::REFLECTION_FILM
            | FilmEffects::GLITCH_FILM
            | FilmEffects::RGB_FILM
            | FilmEffects::GODRAY_FILM;
        self.0 & TIME_VARYING != 0
    }

    pub fn clear(&mut self) {
        self.0 = 0;
    }

    /// Applies one built-in film preset. Returns `false` for non-film presets.
    pub fn apply(&mut self, preset: &AnimationPreset) -> bool {
        let bit = match preset {
            AnimationPreset::OldFilm => Self::OLD_FILM,
            AnimationPreset::DotFilm => Self::DOT_FILM,
            AnimationPreset::ReflectionFilm => Self::REFLECTION_FILM,
            AnimationPreset::GlitchFilm => Self::GLITCH_FILM,
            AnimationPreset::RgbFilm => Self::RGB_FILM,
            AnimationPreset::GodrayFilm => Self::GODRAY_FILM,
            AnimationPreset::RemoveFilm => {
                self.clear();
                return true;
            }
            _ => return false,
        };
        self.0 |= bit;
        true
    }
}

/// Per-image color processing. Values are deliberately backend-neutral.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VisualFilter {
    pub blur: f32,
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
}

impl VisualFilter {
    pub fn is_identity(self) -> bool {
        self.blur <= f32::EPSILON
            && (self.brightness - 1.0).abs() <= f32::EPSILON
            && (self.contrast - 1.0).abs() <= f32::EPSILON
            && (self.saturation - 1.0).abs() <= f32::EPSILON
    }
}

impl Default for VisualFilter {
    fn default() -> Self {
        Self {
            blur: 0.0,
            brightness: 1.0,
            contrast: 1.0,
            saturation: 1.0,
        }
    }
}

/// Adapter-neutral visual state used when a VN highlights the speaking
/// portrait and de-emphasizes the other visible characters.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PortraitStyle {
    pub scale: f32,
    pub brightness: f32,
    pub saturation: f32,
    pub contrast: f32,
    pub blur: f32,
    pub alpha: f32,
}

impl Default for PortraitStyle {
    fn default() -> Self {
        Self {
            scale: 1.0,
            brightness: 1.0,
            saturation: 1.0,
            contrast: 1.0,
            blur: 0.0,
            alpha: 1.0,
        }
    }
}

/// Adapter-neutral dialogue presentation presets.
///
/// The named variants cover the engine's built-in dialogue styles. Unknown
/// editor-defined styles keep their stable identifier so a presentation host
/// can opt into richer rendering without making the script VM depend on CSS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DialogueStyle {
    #[default]
    Default,
    Cinematic,
    CinematicCentered,
    Literary,
    Sharp,
    Handwritten,
    Custom(String),
}

impl DialogueStyle {
    pub fn from_id(id: impl Into<String>) -> Self {
        let id = id.into();
        match id.as_str() {
            "" | "default" => Self::Default,
            "cinematic" => Self::Cinematic,
            "cinematic-centered" => Self::CinematicCentered,
            "literary" => Self::Literary,
            "sharp" => Self::Sharp,
            "handwritten" => Self::Handwritten,
            _ => Self::Custom(id),
        }
    }

    pub fn is_centered(&self) -> bool {
        matches!(self, Self::CinematicCentered)
    }
}

/// A color with alpha.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const WHITE: Self = Self {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };
    pub const BLACK: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }
}

/// Game variable value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Array(Vec<Value>),
}

impl Value {
    pub fn display(&self) -> String {
        match self {
            Self::Int(value) => value.to_string(),
            Self::Float(value) => {
                let value = value.to_string();
                value.strip_suffix(".0").unwrap_or(&value).to_owned()
            }
            Self::Str(value) => value.clone(),
            Self::Bool(value) => value.to_string(),
            Self::Array(values) => format!(
                "[{}]",
                values
                    .iter()
                    .map(Self::display)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }

    pub fn truthy(&self) -> bool {
        match self {
            Self::Bool(value) => *value,
            Self::Int(value) => *value != 0,
            Self::Float(value) => *value != 0.0,
            Self::Str(value) => !value.is_empty(),
            Self::Array(values) => !values.is_empty(),
        }
    }
}

/// Per-sprite transform applied on top of the base anchor position.
/// Mirrors WebGAL's setTransform JSON fields.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpriteTransform {
    pub offset_x: f32,
    pub offset_y: f32,
    pub alpha: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub rotation: f32,
    pub blur: f32,
    /// Optional absolute design-space size. Zero preserves the natural size.
    pub width: f32,
    pub height: f32,
}

impl SpriteTransform {
    pub fn lerp(self, target: Self, factor: f32) -> Self {
        let factor = factor.clamp(0.0, 1.0);
        Self {
            offset_x: self.offset_x + (target.offset_x - self.offset_x) * factor,
            offset_y: self.offset_y + (target.offset_y - self.offset_y) * factor,
            alpha: self.alpha + (target.alpha - self.alpha) * factor,
            scale_x: self.scale_x + (target.scale_x - self.scale_x) * factor,
            scale_y: self.scale_y + (target.scale_y - self.scale_y) * factor,
            rotation: self.rotation + (target.rotation - self.rotation) * factor,
            blur: self.blur + (target.blur - self.blur) * factor,
            width: self.width + (target.width - self.width) * factor,
            height: self.height + (target.height - self.height) * factor,
        }
    }
}

/// Sparse update for an existing [`SpriteTransform`].
///
/// WebGAL's `setTransform` keeps every field that is absent from the command.
/// A compact presence mask avoids the per-field overhead of `Option<f32>` while
/// keeping application allocation-free in the runtime hot path.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct TransformPatch {
    values: SpriteTransform,
    fields: u16,
}

impl TransformPatch {
    const OFFSET_X: u16 = 1 << 0;
    const OFFSET_Y: u16 = 1 << 1;
    const ALPHA: u16 = 1 << 2;
    const SCALE_X: u16 = 1 << 3;
    const SCALE_Y: u16 = 1 << 4;
    const ROTATION: u16 = 1 << 5;
    const BLUR: u16 = 1 << 6;
    const WIDTH: u16 = 1 << 7;
    const HEIGHT: u16 = 1 << 8;

    pub fn is_empty(self) -> bool {
        self.fields == 0
    }

    pub fn set_offset_x(&mut self, value: f32) {
        self.values.offset_x = value;
        self.fields |= Self::OFFSET_X;
    }

    pub fn set_offset_y(&mut self, value: f32) {
        self.values.offset_y = value;
        self.fields |= Self::OFFSET_Y;
    }

    pub fn set_alpha(&mut self, value: f32) {
        self.values.alpha = value;
        self.fields |= Self::ALPHA;
    }

    pub fn set_scale_x(&mut self, value: f32) {
        self.values.scale_x = value;
        self.fields |= Self::SCALE_X;
    }

    pub fn set_scale_y(&mut self, value: f32) {
        self.values.scale_y = value;
        self.fields |= Self::SCALE_Y;
    }

    pub fn set_rotation(&mut self, value: f32) {
        self.values.rotation = value;
        self.fields |= Self::ROTATION;
    }

    pub fn set_blur(&mut self, value: f32) {
        self.values.blur = value;
        self.fields |= Self::BLUR;
    }

    pub fn set_width(&mut self, value: f32) {
        self.values.width = value;
        self.fields |= Self::WIDTH;
    }

    pub fn set_height(&mut self, value: f32) {
        self.values.height = value;
        self.fields |= Self::HEIGHT;
    }

    /// Applies only the fields present in this patch to `base`.
    pub fn apply_to(self, mut base: SpriteTransform) -> SpriteTransform {
        if self.fields & Self::OFFSET_X != 0 {
            base.offset_x = self.values.offset_x;
        }
        if self.fields & Self::OFFSET_Y != 0 {
            base.offset_y = self.values.offset_y;
        }
        if self.fields & Self::ALPHA != 0 {
            base.alpha = self.values.alpha;
        }
        if self.fields & Self::SCALE_X != 0 {
            base.scale_x = self.values.scale_x;
        }
        if self.fields & Self::SCALE_Y != 0 {
            base.scale_y = self.values.scale_y;
        }
        if self.fields & Self::ROTATION != 0 {
            base.rotation = self.values.rotation;
        }
        if self.fields & Self::BLUR != 0 {
            base.blur = self.values.blur;
        }
        if self.fields & Self::WIDTH != 0 {
            base.width = self.values.width;
        }
        if self.fields & Self::HEIGHT != 0 {
            base.height = self.values.height;
        }
        base
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum Easing {
    #[default]
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum InputValueType {
    #[default]
    String,
    Number,
    Bool,
}

/// Backend-neutral input contract used by visual editors. Presentation layers
/// may choose different widgets, but validation and the stored value remain
/// deterministic in core.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserInputSpec {
    pub variable: String,
    pub value_type: InputValueType,
    pub title: String,
    pub description: String,
    pub placeholder: String,
    pub confirm_text: String,
    pub required_text: String,
    pub required: bool,
    pub min_length: usize,
    /// Zero means unlimited.
    pub max_length: usize,
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
    pub step: f64,
    pub true_text: String,
    pub false_text: String,
}

impl Default for UserInputSpec {
    fn default() -> Self {
        Self {
            variable: String::new(),
            value_type: InputValueType::String,
            title: "请输入".into(),
            description: String::new(),
            placeholder: "请输入…".into(),
            confirm_text: "确认".into(),
            required_text: "请填写后再继续".into(),
            required: true,
            min_length: 0,
            max_length: 0,
            min_value: None,
            max_value: None,
            step: 1.0,
            true_text: "是".into(),
            false_text: "否".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BlendMode {
    #[default]
    Alpha,
    Add,
    Multiply,
    Screen,
}

/// Adapter-neutral description of one particle emitter.
///
/// The renderer deliberately derives the full simulation from a small preset
/// plus optional source texture. This keeps save data compact while allowing
/// project adapters to retain authored density and fade timing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParticleEffect {
    pub texture: Option<String>,
    pub preset: String,
    /// Zero asks the renderer to use the preset's tuned native density.
    pub count: u16,
    /// Optional editor-authored horizontal velocity override in design px/s.
    #[serde(default)]
    pub wind: Option<f32>,
    /// Optional editor-authored downward acceleration in design px/s².
    #[serde(default)]
    pub gravity: Option<f32>,
    pub fade_in: f32,
}

impl ParticleEffect {
    pub fn preset(name: impl Into<String>) -> Self {
        Self {
            texture: None,
            preset: name.into(),
            count: 0,
            wind: None,
            gravity: None,
            fade_in: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum VideoMode {
    #[default]
    Fullscreen,
    Mixed,
}

/// Adapter-neutral video playback request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoSpec {
    pub id: String,
    pub file: String,
    pub looped: bool,
    pub muted: bool,
    pub alpha: f32,
    pub skippable: bool,
    pub wait_for_finished: bool,
    pub mode: VideoMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CameraShakeAxis {
    X,
    Y,
    #[default]
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CameraShakeFalloff {
    #[default]
    Linear,
    Exponential,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CameraShakeSpec {
    pub amplitude: f32,
    pub frequency: f32,
    pub duration: f32,
    pub axis: CameraShakeAxis,
    pub falloff: CameraShakeFalloff,
}

/// Compact camera target mask shared by adapters, state and render backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct CameraTargets(u8);

impl CameraTargets {
    const SCENE_BIT: u8 = 1;
    const CHARACTERS_BIT: u8 = 2;

    pub const NONE: Self = Self(0);
    pub const SCENE: Self = Self(Self::SCENE_BIT);
    pub const CHARACTERS: Self = Self(Self::CHARACTERS_BIT);
    pub const ALL: Self = Self(Self::SCENE_BIT | Self::CHARACTERS_BIT);

    pub const fn new(scene: bool, characters: bool) -> Self {
        Self(((scene as u8) * Self::SCENE_BIT) | ((characters as u8) * Self::CHARACTERS_BIT))
    }

    pub const fn scene(self) -> bool {
        self.0 & Self::SCENE_BIT != 0
    }

    pub const fn characters(self) -> bool {
        self.0 & Self::CHARACTERS_BIT != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ColorToneMode {
    #[default]
    None,
    Grayscale,
    Sepia,
}

/// Full camera/filter state shared by editor adapters and the renderer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PostProcessEffect {
    pub focal_distance: Option<f32>,
    pub blur_strength: f32,
    pub distortion_strength: f32,
    pub vignette_intensity: f32,
    pub vignette_size: f32,
    pub blur_amount: f32,
    pub color_tone: ColorToneMode,
    pub color_tone_intensity: f32,
    pub color_exposure: f32,
    pub color_brightness: f32,
    pub color_contrast: f32,
    pub color_saturation: f32,
    pub color_temperature: f32,
    pub old_film_intensity: f32,
    pub shock_intensity: f32,
    pub godray_intensity: f32,
    pub godray_angle: f32,
    pub godray_gain: f32,
    pub godray_lacunarity: f32,
    pub godray_speed: f32,
    pub godray_parallel: bool,
    pub godray_center_x: f32,
    pub godray_center_y: f32,
    pub lut_preset: Option<String>,
    pub lut_intensity: f32,
    pub bloom_intensity: f32,
    pub chromatic_aberration: f32,
    pub pixelate_size: f32,
    pub glitch_intensity: f32,
    pub crt_intensity: f32,
    pub sharpen_strength: f32,
    pub radial_blur_strength: f32,
    pub radial_blur_center_x: f32,
    pub radial_blur_center_y: f32,
    pub motion_blur_strength: f32,
    pub motion_blur_angle: f32,
    pub zoom_blur_strength: f32,
    pub zoom_blur_center_x: f32,
    pub zoom_blur_center_y: f32,
    pub light_leak_intensity: f32,
    pub light_leak_angle: f32,
    pub lens_flare_intensity: f32,
    pub lens_flare_center_x: f32,
    pub lens_flare_center_y: f32,
    pub film_grain_intensity: f32,
    pub film_grain_size: f32,
    pub heat_haze_intensity: f32,
    pub heat_haze_speed: f32,
    pub heat_haze_scale: f32,
    pub water_ripple_intensity: f32,
    pub water_ripple_frequency: f32,
    pub water_ripple_speed: f32,
    pub water_ripple_center_x: f32,
    pub water_ripple_center_y: f32,
    pub fog_intensity: f32,
    pub fog_speed: f32,
    pub fog_scale: f32,
    pub vhs_intensity: f32,
    pub vhs_jitter: f32,
    pub vhs_noise: f32,
    pub halftone_intensity: f32,
    pub halftone_scale: f32,
    pub halftone_angle: f32,
    pub dither_intensity: f32,
    pub dither_levels: f32,
    pub outline_intensity: f32,
    pub outline_thickness: f32,
    pub eyelid_openness: f32,
    pub eyelid_width: f32,
    pub eyelid_curvature: f32,
    pub eyelid_softness: f32,
    pub eyelid_center_x: f32,
    pub eyelid_center_y: f32,
}

impl Default for PostProcessEffect {
    fn default() -> Self {
        Self {
            focal_distance: None,
            blur_strength: 0.0,
            distortion_strength: 0.0,
            vignette_intensity: 0.0,
            vignette_size: 0.7,
            blur_amount: 0.0,
            color_tone: ColorToneMode::None,
            color_tone_intensity: 0.0,
            color_exposure: 0.0,
            color_brightness: 0.0,
            color_contrast: 0.0,
            color_saturation: 1.0,
            color_temperature: 0.0,
            old_film_intensity: 0.0,
            shock_intensity: 0.0,
            godray_intensity: 0.0,
            godray_angle: 30.0,
            godray_gain: 0.5,
            godray_lacunarity: 2.5,
            godray_speed: 1.0,
            godray_parallel: true,
            godray_center_x: 0.5,
            godray_center_y: 0.0,
            lut_preset: None,
            lut_intensity: 0.0,
            bloom_intensity: 0.0,
            chromatic_aberration: 0.0,
            pixelate_size: 1.0,
            glitch_intensity: 0.0,
            crt_intensity: 0.0,
            sharpen_strength: 0.0,
            radial_blur_strength: 0.0,
            radial_blur_center_x: 0.5,
            radial_blur_center_y: 0.5,
            motion_blur_strength: 0.0,
            motion_blur_angle: 0.0,
            zoom_blur_strength: 0.0,
            zoom_blur_center_x: 0.5,
            zoom_blur_center_y: 0.5,
            light_leak_intensity: 0.0,
            light_leak_angle: -35.0,
            lens_flare_intensity: 0.0,
            lens_flare_center_x: 0.75,
            lens_flare_center_y: 0.25,
            film_grain_intensity: 0.0,
            film_grain_size: 1.5,
            heat_haze_intensity: 0.0,
            heat_haze_speed: 1.0,
            heat_haze_scale: 4.0,
            water_ripple_intensity: 0.0,
            water_ripple_frequency: 12.0,
            water_ripple_speed: 1.0,
            water_ripple_center_x: 0.5,
            water_ripple_center_y: 0.5,
            fog_intensity: 0.0,
            fog_speed: 0.2,
            fog_scale: 3.0,
            vhs_intensity: 0.0,
            vhs_jitter: 0.45,
            vhs_noise: 0.35,
            halftone_intensity: 0.0,
            halftone_scale: 6.0,
            halftone_angle: 45.0,
            dither_intensity: 0.0,
            dither_levels: 6.0,
            outline_intensity: 0.0,
            outline_thickness: 1.0,
            eyelid_openness: 1.0,
            eyelid_width: 1.0,
            eyelid_curvature: 0.55,
            eyelid_softness: 0.015,
            eyelid_center_x: 0.5,
            eyelid_center_y: 0.5,
        }
    }
}

impl PostProcessEffect {
    /// Returns whether advancing presentation time can change the output.
    ///
    /// Keeping this decision beside the effect model prevents platform loops
    /// from duplicating shader-specific knowledge.
    pub fn is_time_varying(&self) -> bool {
        const ACTIVE: f32 = 0.001;
        const MOVING: f32 = f32::EPSILON;

        self.old_film_intensity > ACTIVE
            || self.glitch_intensity > ACTIVE
            || self.film_grain_intensity > ACTIVE
            || self.light_leak_intensity > ACTIVE
            || (self.godray_intensity > ACTIVE && self.godray_speed.abs() > MOVING)
            || (self.heat_haze_intensity > ACTIVE && self.heat_haze_speed.abs() > MOVING)
            || (self.water_ripple_intensity > ACTIVE && self.water_ripple_speed.abs() > MOVING)
            || (self.fog_intensity > ACTIVE && self.fog_speed.abs() > MOVING)
            || (self.vhs_intensity > ACTIVE
                && (self.vhs_jitter > ACTIVE || self.vhs_noise > ACTIVE))
    }
}

impl PostProcessEffect {
    pub fn is_identity(&self) -> bool {
        self.focal_distance.is_none()
            && self.blur_strength.abs() <= f32::EPSILON
            && self.distortion_strength.abs() <= f32::EPSILON
            && self.vignette_intensity <= f32::EPSILON
            && self.blur_amount <= f32::EPSILON
            && self.color_tone == ColorToneMode::None
            && self.color_tone_intensity <= f32::EPSILON
            && self.color_exposure.abs() <= f32::EPSILON
            && self.color_brightness.abs() <= f32::EPSILON
            && self.color_contrast.abs() <= f32::EPSILON
            && (self.color_saturation - 1.0).abs() <= f32::EPSILON
            && self.color_temperature.abs() <= f32::EPSILON
            && self.old_film_intensity <= f32::EPSILON
            && self.shock_intensity <= f32::EPSILON
            && self.godray_intensity <= f32::EPSILON
            && self.lut_preset.is_none()
            && self.lut_intensity <= f32::EPSILON
            && self.bloom_intensity <= f32::EPSILON
            && self.chromatic_aberration <= f32::EPSILON
            && (self.pixelate_size - 1.0).abs() <= f32::EPSILON
            && self.glitch_intensity <= f32::EPSILON
            && self.crt_intensity <= f32::EPSILON
            && self.sharpen_strength <= f32::EPSILON
            && self.radial_blur_strength <= f32::EPSILON
            && self.motion_blur_strength <= f32::EPSILON
            && self.zoom_blur_strength <= f32::EPSILON
            && self.light_leak_intensity <= f32::EPSILON
            && self.lens_flare_intensity <= f32::EPSILON
            && self.film_grain_intensity <= f32::EPSILON
            && self.heat_haze_intensity <= f32::EPSILON
            && self.water_ripple_intensity <= f32::EPSILON
            && self.fog_intensity <= f32::EPSILON
            && self.vhs_intensity <= f32::EPSILON
            && self.halftone_intensity <= f32::EPSILON
            && self.dither_intensity <= f32::EPSILON
            && self.outline_intensity <= f32::EPSILON
            && (self.eyelid_openness - 1.0).abs() <= f32::EPSILON
    }

    pub fn interpolate(&self, target: &Self, progress: f32) -> Self {
        let progress = progress.clamp(0.0, 1.0);
        let lerp = |from: f32, to: f32| from + (to - from) * progress;
        Self {
            focal_distance: match (self.focal_distance, target.focal_distance) {
                (Some(from), Some(to)) => Some(lerp(from, to)),
                (_, target) => target,
            },
            blur_strength: lerp(self.blur_strength, target.blur_strength),
            distortion_strength: lerp(self.distortion_strength, target.distortion_strength),
            vignette_intensity: lerp(self.vignette_intensity, target.vignette_intensity),
            vignette_size: lerp(self.vignette_size, target.vignette_size),
            blur_amount: lerp(self.blur_amount, target.blur_amount),
            color_tone: if target.color_tone != ColorToneMode::None || progress >= 1.0 {
                target.color_tone
            } else {
                self.color_tone
            },
            color_tone_intensity: lerp(self.color_tone_intensity, target.color_tone_intensity),
            color_exposure: lerp(self.color_exposure, target.color_exposure),
            color_brightness: lerp(self.color_brightness, target.color_brightness),
            color_contrast: lerp(self.color_contrast, target.color_contrast),
            color_saturation: lerp(self.color_saturation, target.color_saturation),
            color_temperature: lerp(self.color_temperature, target.color_temperature),
            old_film_intensity: lerp(self.old_film_intensity, target.old_film_intensity),
            shock_intensity: lerp(self.shock_intensity, target.shock_intensity),
            godray_intensity: lerp(self.godray_intensity, target.godray_intensity),
            godray_angle: lerp(self.godray_angle, target.godray_angle),
            godray_gain: lerp(self.godray_gain, target.godray_gain),
            godray_lacunarity: lerp(self.godray_lacunarity, target.godray_lacunarity),
            godray_speed: lerp(self.godray_speed, target.godray_speed),
            godray_parallel: if progress >= 1.0 {
                target.godray_parallel
            } else {
                self.godray_parallel
            },
            godray_center_x: lerp(self.godray_center_x, target.godray_center_x),
            godray_center_y: lerp(self.godray_center_y, target.godray_center_y),
            lut_preset: if target.lut_preset.is_some() || progress >= 1.0 {
                target.lut_preset.clone()
            } else {
                self.lut_preset.clone()
            },
            lut_intensity: lerp(self.lut_intensity, target.lut_intensity),
            bloom_intensity: lerp(self.bloom_intensity, target.bloom_intensity),
            chromatic_aberration: lerp(self.chromatic_aberration, target.chromatic_aberration),
            pixelate_size: lerp(self.pixelate_size, target.pixelate_size),
            glitch_intensity: lerp(self.glitch_intensity, target.glitch_intensity),
            crt_intensity: lerp(self.crt_intensity, target.crt_intensity),
            sharpen_strength: lerp(self.sharpen_strength, target.sharpen_strength),
            radial_blur_strength: lerp(self.radial_blur_strength, target.radial_blur_strength),
            radial_blur_center_x: lerp(self.radial_blur_center_x, target.radial_blur_center_x),
            radial_blur_center_y: lerp(self.radial_blur_center_y, target.radial_blur_center_y),
            motion_blur_strength: lerp(self.motion_blur_strength, target.motion_blur_strength),
            motion_blur_angle: lerp(self.motion_blur_angle, target.motion_blur_angle),
            zoom_blur_strength: lerp(self.zoom_blur_strength, target.zoom_blur_strength),
            zoom_blur_center_x: lerp(self.zoom_blur_center_x, target.zoom_blur_center_x),
            zoom_blur_center_y: lerp(self.zoom_blur_center_y, target.zoom_blur_center_y),
            light_leak_intensity: lerp(self.light_leak_intensity, target.light_leak_intensity),
            light_leak_angle: lerp(self.light_leak_angle, target.light_leak_angle),
            lens_flare_intensity: lerp(self.lens_flare_intensity, target.lens_flare_intensity),
            lens_flare_center_x: lerp(self.lens_flare_center_x, target.lens_flare_center_x),
            lens_flare_center_y: lerp(self.lens_flare_center_y, target.lens_flare_center_y),
            film_grain_intensity: lerp(self.film_grain_intensity, target.film_grain_intensity),
            film_grain_size: lerp(self.film_grain_size, target.film_grain_size),
            heat_haze_intensity: lerp(self.heat_haze_intensity, target.heat_haze_intensity),
            heat_haze_speed: lerp(self.heat_haze_speed, target.heat_haze_speed),
            heat_haze_scale: lerp(self.heat_haze_scale, target.heat_haze_scale),
            water_ripple_intensity: lerp(
                self.water_ripple_intensity,
                target.water_ripple_intensity,
            ),
            water_ripple_frequency: lerp(
                self.water_ripple_frequency,
                target.water_ripple_frequency,
            ),
            water_ripple_speed: lerp(self.water_ripple_speed, target.water_ripple_speed),
            water_ripple_center_x: lerp(self.water_ripple_center_x, target.water_ripple_center_x),
            water_ripple_center_y: lerp(self.water_ripple_center_y, target.water_ripple_center_y),
            fog_intensity: lerp(self.fog_intensity, target.fog_intensity),
            fog_speed: lerp(self.fog_speed, target.fog_speed),
            fog_scale: lerp(self.fog_scale, target.fog_scale),
            vhs_intensity: lerp(self.vhs_intensity, target.vhs_intensity),
            vhs_jitter: lerp(self.vhs_jitter, target.vhs_jitter),
            vhs_noise: lerp(self.vhs_noise, target.vhs_noise),
            halftone_intensity: lerp(self.halftone_intensity, target.halftone_intensity),
            halftone_scale: lerp(self.halftone_scale, target.halftone_scale),
            halftone_angle: lerp(self.halftone_angle, target.halftone_angle),
            dither_intensity: lerp(self.dither_intensity, target.dither_intensity),
            dither_levels: lerp(self.dither_levels, target.dither_levels),
            outline_intensity: lerp(self.outline_intensity, target.outline_intensity),
            outline_thickness: lerp(self.outline_thickness, target.outline_thickness),
            eyelid_openness: lerp(self.eyelid_openness, target.eyelid_openness),
            eyelid_width: lerp(self.eyelid_width, target.eyelid_width),
            eyelid_curvature: lerp(self.eyelid_curvature, target.eyelid_curvature),
            eyelid_softness: lerp(self.eyelid_softness, target.eyelid_softness),
            eyelid_center_x: lerp(self.eyelid_center_x, target.eyelid_center_x),
            eyelid_center_y: lerp(self.eyelid_center_y, target.eyelid_center_y),
        }
    }
}

/// Sparse camera/filter update. Missing fields preserve the current value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PostProcessPatch {
    pub focal_distance: Option<Option<f32>>,
    pub blur_strength: Option<f32>,
    pub distortion_strength: Option<f32>,
    pub vignette_intensity: Option<f32>,
    pub vignette_size: Option<f32>,
    pub blur_amount: Option<f32>,
    pub color_tone: Option<ColorToneMode>,
    pub color_tone_intensity: Option<f32>,
    pub color_exposure: Option<f32>,
    pub color_brightness: Option<f32>,
    pub color_contrast: Option<f32>,
    pub color_saturation: Option<f32>,
    pub color_temperature: Option<f32>,
    pub old_film_intensity: Option<f32>,
    pub shock_intensity: Option<f32>,
    pub godray_intensity: Option<f32>,
    pub godray_angle: Option<f32>,
    pub godray_gain: Option<f32>,
    pub godray_lacunarity: Option<f32>,
    pub godray_speed: Option<f32>,
    pub godray_parallel: Option<bool>,
    pub godray_center_x: Option<f32>,
    pub godray_center_y: Option<f32>,
    pub lut_preset: Option<Option<String>>,
    pub lut_intensity: Option<f32>,
    pub bloom_intensity: Option<f32>,
    pub chromatic_aberration: Option<f32>,
    pub pixelate_size: Option<f32>,
    pub glitch_intensity: Option<f32>,
    pub crt_intensity: Option<f32>,
    pub sharpen_strength: Option<f32>,
    pub radial_blur_strength: Option<f32>,
    pub radial_blur_center_x: Option<f32>,
    pub radial_blur_center_y: Option<f32>,
    pub motion_blur_strength: Option<f32>,
    pub motion_blur_angle: Option<f32>,
    pub zoom_blur_strength: Option<f32>,
    pub zoom_blur_center_x: Option<f32>,
    pub zoom_blur_center_y: Option<f32>,
    pub light_leak_intensity: Option<f32>,
    pub light_leak_angle: Option<f32>,
    pub lens_flare_intensity: Option<f32>,
    pub lens_flare_center_x: Option<f32>,
    pub lens_flare_center_y: Option<f32>,
    pub film_grain_intensity: Option<f32>,
    pub film_grain_size: Option<f32>,
    pub heat_haze_intensity: Option<f32>,
    pub heat_haze_speed: Option<f32>,
    pub heat_haze_scale: Option<f32>,
    pub water_ripple_intensity: Option<f32>,
    pub water_ripple_frequency: Option<f32>,
    pub water_ripple_speed: Option<f32>,
    pub water_ripple_center_x: Option<f32>,
    pub water_ripple_center_y: Option<f32>,
    pub fog_intensity: Option<f32>,
    pub fog_speed: Option<f32>,
    pub fog_scale: Option<f32>,
    pub vhs_intensity: Option<f32>,
    pub vhs_jitter: Option<f32>,
    pub vhs_noise: Option<f32>,
    pub halftone_intensity: Option<f32>,
    pub halftone_scale: Option<f32>,
    pub halftone_angle: Option<f32>,
    pub dither_intensity: Option<f32>,
    pub dither_levels: Option<f32>,
    pub outline_intensity: Option<f32>,
    pub outline_thickness: Option<f32>,
    pub eyelid_openness: Option<f32>,
    pub eyelid_width: Option<f32>,
    pub eyelid_curvature: Option<f32>,
    pub eyelid_softness: Option<f32>,
    pub eyelid_center_x: Option<f32>,
    pub eyelid_center_y: Option<f32>,
}

impl PostProcessPatch {
    /// Restore exactly the fields addressed by this patch from a stable base.
    /// A looping timeline uses this before every sample so an event from the
    /// previous loop cannot leak into the next one.
    pub fn restore_affected_from(&self, effect: &mut PostProcessEffect, base: &PostProcessEffect) {
        let restored = base.clone();
        macro_rules! restore {
            ($field:ident) => {
                if self.$field.is_some() {
                    effect.$field = restored.$field;
                }
            };
        }
        restore!(focal_distance);
        restore!(blur_strength);
        restore!(distortion_strength);
        restore!(vignette_intensity);
        restore!(vignette_size);
        restore!(blur_amount);
        restore!(color_tone);
        restore!(color_tone_intensity);
        restore!(color_exposure);
        restore!(color_brightness);
        restore!(color_contrast);
        restore!(color_saturation);
        restore!(color_temperature);
        restore!(old_film_intensity);
        restore!(shock_intensity);
        restore!(godray_intensity);
        restore!(godray_angle);
        restore!(godray_gain);
        restore!(godray_lacunarity);
        restore!(godray_speed);
        restore!(godray_parallel);
        restore!(godray_center_x);
        restore!(godray_center_y);
        restore!(lut_preset);
        restore!(lut_intensity);
        restore!(bloom_intensity);
        restore!(chromatic_aberration);
        restore!(pixelate_size);
        restore!(glitch_intensity);
        restore!(crt_intensity);
        restore!(sharpen_strength);
        restore!(radial_blur_strength);
        restore!(radial_blur_center_x);
        restore!(radial_blur_center_y);
        restore!(motion_blur_strength);
        restore!(motion_blur_angle);
        restore!(zoom_blur_strength);
        restore!(zoom_blur_center_x);
        restore!(zoom_blur_center_y);
        restore!(light_leak_intensity);
        restore!(light_leak_angle);
        restore!(lens_flare_intensity);
        restore!(lens_flare_center_x);
        restore!(lens_flare_center_y);
        restore!(film_grain_intensity);
        restore!(film_grain_size);
        restore!(heat_haze_intensity);
        restore!(heat_haze_speed);
        restore!(heat_haze_scale);
        restore!(water_ripple_intensity);
        restore!(water_ripple_frequency);
        restore!(water_ripple_speed);
        restore!(water_ripple_center_x);
        restore!(water_ripple_center_y);
        restore!(fog_intensity);
        restore!(fog_speed);
        restore!(fog_scale);
        restore!(vhs_intensity);
        restore!(vhs_jitter);
        restore!(vhs_noise);
        restore!(halftone_intensity);
        restore!(halftone_scale);
        restore!(halftone_angle);
        restore!(dither_intensity);
        restore!(dither_levels);
        restore!(outline_intensity);
        restore!(outline_thickness);
        restore!(eyelid_openness);
        restore!(eyelid_width);
        restore!(eyelid_curvature);
        restore!(eyelid_softness);
        restore!(eyelid_center_x);
        restore!(eyelid_center_y);
    }

    pub fn apply_to(&self, mut effect: PostProcessEffect) -> PostProcessEffect {
        macro_rules! apply {
            ($field:ident) => {
                if let Some(value) = &self.$field {
                    effect.$field = value.clone();
                }
            };
        }
        apply!(focal_distance);
        apply!(blur_strength);
        apply!(distortion_strength);
        apply!(vignette_intensity);
        apply!(vignette_size);
        apply!(blur_amount);
        apply!(color_tone);
        apply!(color_tone_intensity);
        apply!(color_exposure);
        apply!(color_brightness);
        apply!(color_contrast);
        apply!(color_saturation);
        apply!(color_temperature);
        apply!(old_film_intensity);
        apply!(shock_intensity);
        apply!(godray_intensity);
        apply!(godray_angle);
        apply!(godray_gain);
        apply!(godray_lacunarity);
        apply!(godray_speed);
        apply!(godray_parallel);
        apply!(godray_center_x);
        apply!(godray_center_y);
        apply!(lut_preset);
        apply!(lut_intensity);
        apply!(bloom_intensity);
        apply!(chromatic_aberration);
        apply!(pixelate_size);
        apply!(glitch_intensity);
        apply!(crt_intensity);
        apply!(sharpen_strength);
        apply!(radial_blur_strength);
        apply!(radial_blur_center_x);
        apply!(radial_blur_center_y);
        apply!(motion_blur_strength);
        apply!(motion_blur_angle);
        apply!(zoom_blur_strength);
        apply!(zoom_blur_center_x);
        apply!(zoom_blur_center_y);
        apply!(light_leak_intensity);
        apply!(light_leak_angle);
        apply!(lens_flare_intensity);
        apply!(lens_flare_center_x);
        apply!(lens_flare_center_y);
        apply!(film_grain_intensity);
        apply!(film_grain_size);
        apply!(heat_haze_intensity);
        apply!(heat_haze_speed);
        apply!(heat_haze_scale);
        apply!(water_ripple_intensity);
        apply!(water_ripple_frequency);
        apply!(water_ripple_speed);
        apply!(water_ripple_center_x);
        apply!(water_ripple_center_y);
        apply!(fog_intensity);
        apply!(fog_speed);
        apply!(fog_scale);
        apply!(vhs_intensity);
        apply!(vhs_jitter);
        apply!(vhs_noise);
        apply!(halftone_intensity);
        apply!(halftone_scale);
        apply!(halftone_angle);
        apply!(dither_intensity);
        apply!(dither_levels);
        apply!(outline_intensity);
        apply!(outline_thickness);
        apply!(eyelid_openness);
        apply!(eyelid_width);
        apply!(eyelid_curvature);
        apply!(eyelid_softness);
        apply!(eyelid_center_x);
        apply!(eyelid_center_y);
        effect
    }

    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnlockKind {
    Cg,
    Bgm,
}

impl Easing {
    pub fn sample(self, progress: f32) -> f32 {
        let progress = progress.clamp(0.0, 1.0);
        match self {
            Self::Linear => progress,
            Self::EaseIn => progress * progress,
            Self::EaseOut => 1.0 - (1.0 - progress) * (1.0 - progress),
            Self::EaseInOut => progress * progress * (3.0 - 2.0 * progress),
        }
    }
}

impl Default for SpriteTransform {
    fn default() -> Self {
        Self {
            offset_x: 0.0,
            offset_y: 0.0,
            alpha: 1.0,
            scale_x: 1.0,
            scale_y: 1.0,
            rotation: 0.0,
            blur: 0.0,
            width: 0.0,
            height: 0.0,
        }
    }
}

#[cfg(test)]
mod transform_patch_tests {
    use super::*;

    #[test]
    fn sparse_patch_preserves_absent_fields() {
        let base = SpriteTransform {
            offset_x: 12.0,
            offset_y: -8.0,
            alpha: 0.65,
            scale_x: 1.3,
            scale_y: 0.9,
            rotation: 0.2,
            blur: 4.0,
            ..SpriteTransform::default()
        };
        let mut patch = TransformPatch::default();
        patch.set_offset_x(100.0);
        patch.set_blur(0.0);

        assert_eq!(
            patch.apply_to(base),
            SpriteTransform {
                offset_x: 100.0,
                blur: 0.0,
                ..base
            }
        );
    }

    #[test]
    fn empty_patch_is_identity_and_compact() {
        let base = SpriteTransform {
            offset_x: 42.0,
            ..SpriteTransform::default()
        };
        let patch = TransformPatch::default();

        assert!(patch.is_empty());
        assert_eq!(patch.apply_to(base), base);
        // Nine scalar transform channels plus one bit mask; still much smaller
        // than nine `Option<f32>` values and allocation-free to apply.
        assert_eq!(std::mem::size_of::<TransformPatch>(), 40);
    }
}
