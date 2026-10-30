//! Source-backed declared timing. Player waits and unmodelled commands split segments.
use super::projection::{BlockCard, BlockKind, EiyashouProjection, SourceField};
use std::ops::Range;

#[derive(Clone, Debug)]
pub(crate) struct DurationField {
    pub range: Range<usize>,
    pub seconds: f64,
    pub milliseconds: bool,
}
impl DurationField {
    pub fn replacement(&self, seconds: f64) -> String {
        let ms = (seconds.max(0.) * 100.).round() * 10.;
        if self.milliseconds {
            format!("{ms:.0}ms")
        } else {
            format!("{}s", ms / 1000.)
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) struct TimelineRow {
    pub block: BlockCard,
    pub start: f64,
    pub duration: Option<DurationField>,
}
#[derive(Clone, Debug)]
pub(crate) struct Segment {
    pub scene: String,
    pub rows: Vec<TimelineRow>,
    pub end: f64,
}

fn literal(range: Range<usize>, text: &str) -> Option<DurationField> {
    let trimmed = text.trim();
    let (number, milliseconds) = if let Some(s) = trimmed.strip_suffix("ms") {
        (s, true)
    } else {
        (trimmed.strip_suffix('s')?, false)
    };
    let number: f64 = number.trim().parse().ok()?;
    if !number.is_finite() || number < 0. {
        return None;
    }
    let start = range.start + text.len() - text.trim_start().len();
    Some(DurationField {
        range: start..start + trimmed.len(),
        seconds: if milliseconds { number / 1000. } else { number },
        milliseconds,
    })
}
fn duration(command: &str, fields: &[SourceField]) -> Option<DurationField> {
    let key = match command {
        "wait" => "0",
        "camera.move"
        | "camera.reset"
        | "camera.effect"
        | "move"
        | "sprite.update"
        | "screen.curtain.show"
        | "screen.curtain.hide" => "duration",
        "background" | "sprite" | "hide" => "transition",
        _ => return None,
    };
    let field = fields
        .iter()
        .find(|f| f.key == key && f.insertion.is_none() && !f.quoted)?;
    if key != "transition" {
        return literal(field.range.clone(), &field.value);
    }
    // Transition constructors have exactly one duration; leave expressions and custom rules alone.
    let open = field.value.find('(')?;
    if !matches!(
        field.value[..open].trim(),
        "fade" | "crossfade" | "dissolve" | "wipe_left" | "wipe_right" | "wipe_up" | "wipe_down"
    ) {
        return None;
    }
    let close = field.value.rfind(')')?;
    literal(
        field.range.start + open + 1..field.range.start + close,
        &field.value[open + 1..close],
    )
}

pub(crate) fn segments(source: &str, projection: &EiyashouProjection) -> Vec<Segment> {
    let mut result = Vec::new();
    for scene in &projection.scenes {
        let mut segment = Segment {
            scene: scene.name.clone(),
            rows: Vec::new(),
            end: 0.,
        };
        let mut cursor: f64 = 0.;
        for block in &scene.blocks {
            if block.disabled {
                continue;
            }
            let command = source
                .get(block.statement_range.clone())
                .unwrap_or("")
                .split('(')
                .next()
                .unwrap_or("")
                .trim();
            let fields = projection
                .source_fields_for_block(source, block)
                .unwrap_or_default();
            let timing = (block.kind == BlockKind::Command && !block.read_only)
                .then(|| duration(command, &fields))
                .flatten();
            let blocking = fields
                .iter()
                .find(|f| f.key == "blocking" && f.insertion.is_none());
            let unknown_blocking =
                blocking.is_some_and(|f| !matches!(f.value.as_str(), "true" | "false"));
            let boundary = timing.is_none() || block.depth > 0 || unknown_blocking;
            if boundary && !segment.rows.is_empty() {
                result.push(segment);
                segment = Segment {
                    scene: scene.name.clone(),
                    rows: Vec::new(),
                    end: 0.,
                };
                cursor = 0.;
            }
            let start = cursor;
            if let Some(field) = &timing {
                segment.end = segment.end.max(start + field.seconds);
                if !blocking.is_some_and(|f| f.value == "false") {
                    cursor += field.seconds;
                }
            }
            segment.rows.push(TimelineRow {
                block: block.clone(),
                start,
                duration: timing,
            });
            if boundary {
                result.push(segment);
                segment = Segment {
                    scene: scene.name.clone(),
                    rows: Vec::new(),
                    end: 0.,
                };
                cursor = 0.;
            }
        }
        if !segment.rows.is_empty() {
            result.push(segment);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timing_preserves_player_boundaries_and_parallel_actions() {
        let source = "scene a { camera.move(scene, x: 1, duration: 2s, blocking: false), wait(500ms), camera.move(scene, x: 2, duration: 1s), \"Next\", wait(1s) }";
        let projection = EiyashouProjection::parse(source);
        let groups = segments(source, &projection);
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].end, 2.);
        assert_eq!(
            groups[0].rows.iter().map(|r| r.start).collect::<Vec<_>>(),
            [0., 0., 0.5]
        );
        assert!(groups[1].rows[0].duration.is_none());
        assert_eq!(groups[2].rows[0].start, 0.);
        let field = groups[0].rows[1].duration.as_ref().unwrap();
        let mut edited = source.to_owned();
        edited.replace_range(field.range.clone(), &field.replacement(0.75));
        assert!(edited.contains("wait(750ms)"));
        assert!(EiyashouProjection::parse(&edited).read_only.is_empty());
        assert!(literal(0..3, "-1s").is_none());
        assert!(literal(0..6, "time*s").is_none());
    }
}
