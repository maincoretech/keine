//! Grouped author positions and layouts; engine coordinates remain unchanged.
use super::*;

impl<'a> Parser<'a> {
    pub(in crate::adapter::script::native) fn grouped_args(
        &self,
        argument: &Argument,
        report: &mut ParseReport,
    ) -> Option<(String, Vec<Argument>)> {
        let tokens = &argument.token_indices;
        if tokens.len() < 3
            || self.tokens[tokens[0]].kind != NativeTokenKind::Identifier
            || self.text(tokens[1]) != "("
            || self.text(*tokens.last()?) != ")"
        {
            report
                .diagnostics
                .push(self.error("expected a grouped value such as `right(x: 20)`"));
            return None;
        }
        let mut args = Vec::new();
        let mut start = 2;
        let mut depth = 0usize;
        for cursor in 2..tokens.len() - 1 {
            match self.text(tokens[cursor]) {
                "(" | "[" => depth += 1,
                ")" | "]" => {
                    let Some(next) = depth.checked_sub(1) else {
                        report
                            .diagnostics
                            .push(self.error("unbalanced grouped value"));
                        return None;
                    };
                    depth = next;
                }
                "," if depth == 0 => {
                    args.push(self.grouped_field(&tokens[start..cursor], report)?);
                    start = cursor + 1;
                }
                _ => {}
            }
        }
        if depth != 0 {
            report
                .diagnostics
                .push(self.error("unbalanced grouped value"));
            return None;
        }
        if start < tokens.len() - 1 {
            args.push(self.grouped_field(&tokens[start..tokens.len() - 1], report)?);
        } else if !args.is_empty() {
            report
                .diagnostics
                .push(self.error("trailing commas are not allowed"));
            return None;
        }
        Some((self.text(tokens[0]).to_owned(), args))
    }

    fn grouped_field(&self, tokens: &[usize], report: &mut ParseReport) -> Option<Argument> {
        if tokens.len() < 3
            || self.tokens[tokens[0]].kind != NativeTokenKind::Identifier
            || self.text(tokens[1]) != ":"
        {
            report
                .diagnostics
                .push(self.error("grouped values require named fields"));
            return None;
        }
        Some(Argument {
            name: Some(self.text(tokens[0]).to_owned()),
            token_indices: tokens[2..].to_vec(),
        })
    }

    pub(in crate::adapter::script::native) fn checked_group(
        &self,
        name: &str,
        args: &[Argument],
        fields: &[&str],
        report: &mut ParseReport,
    ) -> Option<()> {
        let before = report.diagnostics.len();
        self.validate_signature(name, args, 0, fields, report);
        let mut seen = HashSet::new();
        for field in args.iter().filter_map(|argument| argument.name.as_deref()) {
            if !seen.insert(field) {
                report
                    .diagnostics
                    .push(self.error(format!("duplicate grouped field `{field}`")));
            }
        }
        (before == report.diagnostics.len()).then_some(())
    }

    pub(in crate::adapter::script::native) fn author_position(
        &self,
        argument: Option<&Argument>,
        report: &mut ParseReport,
    ) -> Option<Position> {
        let (name, args) = match argument {
            None => ("center".to_owned(), Vec::new()),
            Some(argument) => match self.argument_identifier(argument) {
                Some(name) => (name, Vec::new()),
                None => self.grouped_args(argument, report)?,
            },
        };
        self.checked_group(&name, &args, &["x", "y"], report)?;
        let offset = self.checked_number(&args, "x", report)?.unwrap_or(0.0);
        let y = self.checked_number(&args, "y", report)?.unwrap_or(0.0);
        let x = match name.as_str() {
            "left" => keine_core::Anchor::Left(offset),
            "center" => keine_core::Anchor::Center(offset),
            "right" => keine_core::Anchor::Right(offset),
            _ => {
                report.diagnostics.push(self.error("position must be `left`, `center`, or `right`, optionally with `(x: ..., y: ...)`"));
                return None;
            }
        };
        Some(Position { x, y })
    }

    fn layout_pair(
        &self,
        argument: &Argument,
        constructor: &str,
        fields: [&str; 2],
        defaults: Option<[f32; 2]>,
        report: &mut ParseReport,
    ) -> Option<[f32; 2]> {
        let (name, args) = self.grouped_args(argument, report)?;
        if name != constructor {
            report
                .diagnostics
                .push(self.error(format!("expected `{constructor}(...)`")));
            return None;
        }
        self.checked_group(&name, &args, &fields, report)?;
        let mut pair = [0.0; 2];
        for (index, field) in fields.iter().enumerate() {
            pair[index] = match self.checked_number(&args, field, report)? {
                Some(value) => value,
                None if defaults.is_some() => defaults?[index],
                None => {
                    report
                        .diagnostics
                        .push(self.error(format!("{constructor} requires `{field}`")));
                    return None;
                }
            };
        }
        Some(pair)
    }

    pub(in crate::adapter::script::native) fn v11_sprite_layout(
        &self,
        args: &[Argument],
        report: &mut ParseReport,
    ) -> Option<SpriteLayout> {
        let Some(argument) = self.named_arg(args, "layout") else {
            return Some(SpriteLayout::Natural);
        };
        if self.argument_identifier(argument).as_deref() == Some("natural") {
            return Some(SpriteLayout::Natural);
        }
        let (name, fields) = self.grouped_args(argument, report)?;
        let args = fields.as_slice();
        match name.as_str() {
            "natural" => {
                self.checked_group(&name, args, &[], report)?;
                Some(SpriteLayout::Natural)
            }
            "viewport" => {
                self.checked_group(&name, args, &["height"], report)?;
                let height = self
                    .checked_number(args, "height", report)?
                    .filter(|height| *height > 0.0);
                match height {
                    Some(height) => Some(SpriteLayout::ViewportHeight(height)),
                    None => {
                        report
                            .diagnostics
                            .push(self.error("viewport requires positive `height`"));
                        None
                    }
                }
            }
            "scene" => {
                self.checked_group(
                    &name,
                    args,
                    &["fit", "x", "y", "anchor", "width", "height"],
                    report,
                )?;
                let fit = match self.named_identifier(args, "fit").as_deref() {
                    None if self.named_arg(args, "fit").is_none() => keine_core::SceneFit::ByHeight,
                    Some("by_height") => keine_core::SceneFit::ByHeight,
                    Some("by_width") => keine_core::SceneFit::ByWidth,
                    Some("cover") => keine_core::SceneFit::Cover,
                    Some("contain") => keine_core::SceneFit::Contain,
                    Some("stretch") => keine_core::SceneFit::Stretch,
                    Some("center") => keine_core::SceneFit::Center,
                    _ => {
                        report
                            .diagnostics
                            .push(self.error("unknown scene layout fit"));
                        return None;
                    }
                };
                let width = self.checked_number(args, "width", report)?;
                let height = self.checked_number(args, "height", report)?;
                if width.is_some() != height.is_some() {
                    report
                        .diagnostics
                        .push(self.error("scene layout size requires both width and height"));
                    return None;
                }
                let anchor = match self.named_arg(args, "anchor") {
                    Some(arg) => {
                        self.layout_pair(arg, "point", ["x", "y"], Some([0.5; 2]), report)?
                    }
                    None => [0.5; 2],
                };
                Some(SpriteLayout::Scene(keine_core::SceneLayerLayout {
                    fit,
                    position: [
                        self.checked_number(args, "x", report)?.unwrap_or(0.0),
                        self.checked_number(args, "y", report)?.unwrap_or(0.0),
                    ],
                    anchor,
                    size: width.zip(height).map(|(w, h)| [w, h]),
                }))
            }
            "composite" => {
                self.checked_group(&name, args, &["canvas", "rect", "height"], report)?;
                let Some(canvas) = self.named_arg(args, "canvas") else {
                    report.diagnostics.push(
                        self.error("composite requires `canvas: size(width: ..., height: ...)`"),
                    );
                    return None;
                };
                let canvas = self.layout_pair(canvas, "size", ["width", "height"], None, report)?;
                let rect = if let Some(arg) = self.named_arg(args, "rect") {
                    let (name, rect) = self.grouped_args(arg, report)?;
                    if name != "rect" {
                        report.diagnostics.push(
                            self.error("expected `rect(x: ..., y: ..., width: ..., height: ...)`"),
                        );
                        return None;
                    }
                    self.checked_group(&name, &rect, &["x", "y", "width", "height"], report)?;
                    let mut values = [0.0; 4];
                    for (index, field) in ["x", "y", "width", "height"].iter().enumerate() {
                        let Some(value) = self.checked_number(&rect, field, report)? else {
                            report
                                .diagnostics
                                .push(self.error(format!("rect requires `{field}`")));
                            return None;
                        };
                        values[index] = value;
                    }
                    Some(values)
                } else {
                    None
                };
                Some(SpriteLayout::Composite {
                    canvas,
                    rect,
                    height_ratio: self.checked_number(args, "height", report)?,
                })
            }
            _ => {
                report.diagnostics.push(
                    self.error(
                        "layout must be natural, viewport(...), scene(...), or composite(...)",
                    ),
                );
                None
            }
        }
    }
}
