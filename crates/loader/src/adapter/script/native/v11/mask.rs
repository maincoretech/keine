use super::*;
use keine_core::{
    StageMask, StageMaskFillMode, StageMaskFit, StageMaskImageChannel, StageMaskMode,
    StageMaskPlane, StageMaskScope, StageMaskShape, StageMaskTextureBlend, StageMaskVisibility,
};

pub(super) const MASK_COMMANDS: &[&str] = &["stage.mask.show", "stage.mask.hide"];
pub(super) const MASK_FIELDS: &[&str] = &[
    "duration",
    "blocking",
    "mode",
    "plane",
    "scope",
    "targets",
    "shape",
    "image",
    "image_channel",
    "image_fit",
    "rotation",
    "radius",
    "visibility",
    "feather",
    "opacity",
    "fill_mode",
    "gradient_direction",
    "texture",
    "texture_fit",
    "texture_blend",
    "texture_scale",
    "texture_opacity",
    "blur",
    "vignette_amount",
    "vignette_size",
    "noise_amount",
    "noise_size",
    "hue",
    "saturation",
    "brightness",
    "center_x",
    "center_y",
    "size_x",
    "size_y",
    "color",
    "gradient_start",
    "gradient_end",
];

impl<'a> Parser<'a> {
    pub(super) fn parse_v11_mask_command(
        &self,
        name: &str,
        args: &[Argument],
        report: &mut ParseReport,
    ) -> Option<Action> {
        let before = report.diagnostics.len();
        self.validate_signature(
            name,
            args,
            1,
            if name == "stage.mask.show" {
                MASK_FIELDS
            } else {
                &["duration", "blocking"]
            },
            report,
        );
        if report.diagnostics.len() != before {
            return None;
        }
        let id = self.v11_identifier(args.first(), "mask ID", report)?;
        let duration = self.named_duration_checked(args, "duration", report)?;
        let blocking = self.v11_optional_bool(args, "blocking", true, report)?;
        if name == "stage.mask.hide" {
            return Some(Action::StageMask {
                id,
                mask: None,
                duration,
                blocking,
            });
        }
        let mut mask = StageMask::default();
        if let Some(arg) = self.named_arg(args, "mode") {
            mask.mode = match self.argument_identifier(arg).as_deref() {
                Some("overlay") => StageMaskMode::Overlay,
                Some("clip") => StageMaskMode::Clip,
                _ => {
                    report.diagnostics.push(self.error("unknown mask mode"));
                    return None;
                }
            };
        }
        if let Some(arg) = self.named_arg(args, "plane") {
            mask.plane = match self.argument_identifier(arg).as_deref() {
                Some("behind_scene") => StageMaskPlane::BehindScene,
                Some("bottom") => StageMaskPlane::Bottom,
                Some("top") => StageMaskPlane::Top,
                Some("topmost") => StageMaskPlane::Topmost,
                _ => {
                    report.diagnostics.push(self.error("unknown mask plane"));
                    return None;
                }
            };
        }
        if let Some(arg) = self.named_arg(args, "scope") {
            mask.scope = match self.argument_identifier(arg).as_deref() {
                Some("scene") => StageMaskScope::Scene,
                Some("characters") => StageMaskScope::Characters,
                Some("all") => StageMaskScope::All,
                Some("selected") => StageMaskScope::Selected,
                _ => {
                    report.diagnostics.push(self.error("unknown mask scope"));
                    return None;
                }
            };
        }
        if let Some(arg) = self.named_arg(args, "shape") {
            mask.shape = match self.argument_identifier(arg).as_deref() {
                Some("rectangle") => StageMaskShape::Rectangle,
                Some("rounded_rectangle") => StageMaskShape::RoundedRectangle,
                Some("ellipse") => StageMaskShape::Ellipse,
                Some("image") => StageMaskShape::Image,
                _ => {
                    report.diagnostics.push(self.error("unknown mask shape"));
                    return None;
                }
            };
        }
        if let Some(arg) = self.named_arg(args, "image_channel") {
            mask.image_channel = match self.argument_identifier(arg).as_deref() {
                Some("alpha") => StageMaskImageChannel::Alpha,
                Some("luminance") => StageMaskImageChannel::Luminance,
                _ => {
                    report
                        .diagnostics
                        .push(self.error("unknown mask image_channel"));
                    return None;
                }
            };
        }
        if let Some(arg) = self.named_arg(args, "image_fit") {
            mask.image_fit = match self.argument_identifier(arg).as_deref() {
                Some("stretch") => StageMaskFit::Stretch,
                Some("cover") => StageMaskFit::Cover,
                Some("contain") => StageMaskFit::Contain,
                _ => {
                    report
                        .diagnostics
                        .push(self.error("unknown mask image_fit"));
                    return None;
                }
            };
        }
        if let Some(arg) = self.named_arg(args, "visibility") {
            mask.visibility = match self.argument_identifier(arg).as_deref() {
                Some("inside") => StageMaskVisibility::Inside,
                Some("outside") => StageMaskVisibility::Outside,
                _ => {
                    report
                        .diagnostics
                        .push(self.error("unknown mask visibility"));
                    return None;
                }
            };
        }
        if let Some(arg) = self.named_arg(args, "fill_mode") {
            mask.fill_mode = match self.argument_identifier(arg).as_deref() {
                Some("solid") => StageMaskFillMode::Solid,
                Some("gradient") => StageMaskFillMode::Gradient,
                Some("texture") => StageMaskFillMode::Texture,
                _ => {
                    report
                        .diagnostics
                        .push(self.error("unknown mask fill_mode"));
                    return None;
                }
            };
        }
        if let Some(arg) = self.named_arg(args, "texture_fit") {
            mask.texture_fit = match self.argument_identifier(arg).as_deref() {
                Some("stretch") => StageMaskFit::Stretch,
                Some("cover") => StageMaskFit::Cover,
                Some("contain") => StageMaskFit::Contain,
                _ => {
                    report
                        .diagnostics
                        .push(self.error("unknown mask texture_fit"));
                    return None;
                }
            };
        }
        if let Some(arg) = self.named_arg(args, "texture_blend") {
            mask.texture_blend = match self.argument_identifier(arg).as_deref() {
                Some("normal") => StageMaskTextureBlend::Normal,
                Some("multiply") => StageMaskTextureBlend::Multiply,
                Some("screen") => StageMaskTextureBlend::Screen,
                Some("add") => StageMaskTextureBlend::Add,
                _ => {
                    report
                        .diagnostics
                        .push(self.error("unknown mask texture_blend"));
                    return None;
                }
            };
        }
        if let Some(arg) = self.named_arg(args, "image") {
            mask.image = if self.argument_identifier(arg).as_deref() == Some("none") {
                None
            } else {
                Some(self.v11_identifier(Some(arg), "mask image asset", report)?)
            };
        }
        if let Some(arg) = self.named_arg(args, "texture") {
            mask.texture = if self.argument_identifier(arg).as_deref() == Some("none") {
                None
            } else {
                Some(self.v11_identifier(Some(arg), "mask texture asset", report)?)
            };
        }
        if let Some(arg) = self.named_arg(args, "targets") {
            mask.targets = self.portrait_character_ids(arg, report)?;
        }
        mask.rotation = self.v11_optional_number(args, "rotation", mask.rotation, report)?;
        mask.radius = self.v11_optional_number(args, "radius", mask.radius, report)?;
        mask.feather = self.v11_optional_number(args, "feather", mask.feather, report)?;
        mask.opacity = self.v11_optional_number(args, "opacity", mask.opacity, report)?;
        mask.gradient_direction =
            self.v11_optional_number(args, "gradient_direction", mask.gradient_direction, report)?;
        mask.texture_scale =
            self.v11_optional_number(args, "texture_scale", mask.texture_scale, report)?;
        mask.texture_opacity =
            self.v11_optional_number(args, "texture_opacity", mask.texture_opacity, report)?;
        mask.blur = self.v11_optional_number(args, "blur", mask.blur, report)?;
        mask.vignette_amount =
            self.v11_optional_number(args, "vignette_amount", mask.vignette_amount, report)?;
        mask.vignette_size =
            self.v11_optional_number(args, "vignette_size", mask.vignette_size, report)?;
        mask.noise_amount =
            self.v11_optional_number(args, "noise_amount", mask.noise_amount, report)?;
        mask.noise_size = self.v11_optional_number(args, "noise_size", mask.noise_size, report)?;
        mask.hue = self.v11_optional_number(args, "hue", mask.hue, report)?;
        mask.saturation = self.v11_optional_number(args, "saturation", mask.saturation, report)?;
        mask.brightness = self.v11_optional_number(args, "brightness", mask.brightness, report)?;
        mask.center[0] = self.v11_optional_number(args, "center_x", mask.center[0], report)?;
        mask.center[1] = self.v11_optional_number(args, "center_y", mask.center[1], report)?;
        mask.size[0] = self.v11_optional_number(args, "size_x", mask.size[0], report)?;
        mask.size[1] = self.v11_optional_number(args, "size_y", mask.size[1], report)?;
        mask.color = self.v11_optional_color(args, "color", mask.color, report)?;
        mask.gradient_start =
            self.v11_optional_color(args, "gradient_start", mask.gradient_start, report)?;
        mask.gradient_end =
            self.v11_optional_color(args, "gradient_end", mask.gradient_end, report)?;
        Some(Action::StageMask {
            id,
            mask: Some(Box::new(mask)),
            duration,
            blocking,
        })
    }
}
