//! Batch voice assignment from explicit dialogue IDs; never replaces an existing voice.
use super::projection::EiyashouProjection;
use super::{AssetEntry, AssetKind};

pub(crate) struct VoiceMatch {
    pub line: usize,
    pub column: usize,
    pub id: String,
    pub voice: Option<String>,
    pub status: &'static str,
}

pub(crate) fn match_voices(source: &str, assets: &[AssetEntry]) -> (String, Vec<VoiceMatch>) {
    let projection = EiyashouProjection::parse(source);
    let mut edited = source.to_owned();
    let mut rows = Vec::new();
    // Later source ranges first, so earlier ranges remain valid.
    for block in projection.scenes.iter().flat_map(|s| &s.blocks).rev() {
        if block.disabled || block.read_only {
            continue;
        }
        let Some(mut metadata) = projection.text_block_metadata_for_block(source, block) else {
            continue;
        };
        if metadata.voice.is_some() {
            continue;
        }
        let id = metadata.stable_id.clone().unwrap_or_default();
        let candidates: Vec<_> = assets
            .iter()
            .filter(|asset| {
                asset.kind == AssetKind::Voice
                    && asset.exists
                    && !id.is_empty()
                    && (asset.id == id
                        || asset.path.file_stem().and_then(|s| s.to_str()) == Some(&id))
            })
            .collect();
        let voice = match candidates.as_slice() {
            [asset] => Some(asset.id.clone()),
            _ => None,
        };
        let mut status = if id.is_empty() {
            "No dialogue ID"
        } else if candidates.len() > 1 {
            "Multiple matching voices"
        } else if voice.is_none() {
            "Missing voice"
        } else {
            "Matched"
        };
        metadata.voice = voice.clone();
        // Each replacement uses the original ranges. Edits below this row don't shift it.
        let voice = if voice.is_some() {
            match projection.replace_text_block_metadata(
                &edited,
                block.source_range.start,
                &metadata,
            ) {
                Ok(next) => {
                    edited = next;
                    voice
                }
                Err(_) => {
                    status = "Cannot assign voice";
                    None
                }
            }
        } else {
            None
        };
        rows.push(VoiceMatch {
            line: block.line,
            column: block.column,
            id,
            voice,
            status,
        });
    }
    rows.reverse();
    (edited, rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_ids_without_overwriting_or_guessing_ambiguous_voices() {
        let asset = |id: &str, file: &str| AssetEntry {
            kind: AssetKind::Voice,
            id: id.into(),
            path: file.into(),
            exists: true,
            tags: vec![],
            reference_count: 0,
        };
        let source =
            "scene start { @one \"A\", @two \"B\", old, @three \"C\", @four \"D\", \"E\" }";
        let (edited, rows) = match_voices(
            source,
            &[
                asset("one", "one.opus"),
                asset("v3", "three.opus"),
                asset("x", "four.opus"),
                asset("y", "four.opus"),
            ],
        );
        let projection = EiyashouProjection::parse(&edited);
        assert!(projection.read_only.is_empty(), "{edited}");
        let voices: Vec<_> = projection.scenes[0]
            .blocks
            .iter()
            .map(|b| {
                projection
                    .text_block_metadata_for_block(&edited, b)
                    .unwrap()
                    .voice
            })
            .collect();
        assert_eq!(
            voices,
            [
                Some("one".into()),
                Some("old".into()),
                Some("v3".into()),
                None,
                None
            ]
        );
        assert_eq!(rows.iter().filter(|r| r.voice.is_some()).count(), 2);
        assert_eq!(rows[2].status, "Multiple matching voices");
        assert_eq!(
            match_voices(
                &edited,
                &[asset("one", "one.opus"), asset("v3", "three.opus")]
            )
            .0,
            edited
        );
    }
}
