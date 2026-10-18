//! Editor-only interface language. Source text, protocol values and IDs are never localized.
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "en")]
    English,
    #[serde(rename = "zh-CN")]
    SimplifiedChinese,
    #[serde(rename = "zh-TW")]
    TraditionalChinese,
}

impl Language {
    pub const ALL: [Self; 3] = [
        Self::SimplifiedChinese,
        Self::TraditionalChinese,
        Self::English,
    ];
    pub const fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::SimplifiedChinese => "zh-CN",
            Self::TraditionalChinese => "zh-TW",
        }
    }
    pub const fn name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::SimplifiedChinese => "简体中文",
            Self::TraditionalChinese => "繁體中文",
        }
    }
}

// One application-wide preference, shared by every window. No IO on the render path.
static LANGUAGE: AtomicU8 = AtomicU8::new(0);
pub fn language() -> Language {
    match LANGUAGE.load(Ordering::Relaxed) {
        1 => Language::SimplifiedChinese,
        2 => Language::TraditionalChinese,
        _ => Language::English,
    }
}
pub(crate) fn set_language(language: Language) {
    LANGUAGE.store(language as u8, Ordering::Relaxed);
    gpui_kit::component::set_locale(language.code());
}
pub fn tr(message: &str) -> &str {
    translate(language(), message)
}
pub(crate) fn text(message: &str) -> gpui_kit::SharedString {
    tr(message).to_owned().into()
}

macro_rules! catalog {
    ($( $en:literal => ($cn:literal, $tw:literal), )*) => {
        pub fn translate(language: Language, message: &str) -> &str {
            if language == Language::English { return message; }
            match message {
                $($en => if language == Language::SimplifiedChinese { $cn } else { $tw },)*
                _ => message,
            }
        }
        const MESSAGES: &[(&str, &str, &str)] = &[$(($en, $cn, $tw)),*];
    };
}

include!("i18n/catalog.rs");

/// Substitute named UI fields once; inserted paths, names and diagnostic details stay verbatim.
pub(crate) fn format(message: &str, fields: &[(&str, String)]) -> String {
    let message = tr(message);
    let mut output = String::with_capacity(message.len());
    let mut rest = message;
    while let Some(start) = rest.find('{') {
        output.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('}').map(|n| start + n) else {
            output.push_str(&rest[start..]);
            return output;
        };
        let key = &rest[start + 1..end];
        if let Some((_, value)) = fields.iter().find(|(name, _)| *name == key) {
            output.push_str(value);
        } else {
            output.push_str(&rest[start..=end]);
        }
        rest = &rest[end + 1..];
    }
    output.push_str(rest);
    output
}

/// Existing input placeholders can be refreshed without touching their editable contents.
pub(crate) fn refresh_input(
    input: &gpui_kit::Entity<gpui_kit::component::input::InputState>,
    window: &mut gpui_kit::Window,
    cx: &mut gpui_kit::App,
) {
    let presentation = input.read(cx).presentation();
    let previous = presentation.placeholder();
    let key = MESSAGES
        .iter()
        .find(|(en, cn, tw)| {
            *en == previous.as_ref() || *cn == previous.as_ref() || *tw == previous.as_ref()
        })
        .map(|row| row.0);
    if let Some(key) = key {
        let next = tr(key);
        if next != previous.as_ref() {
            input.update(cx, |input, cx| input.set_placeholder(next, window, cx));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_is_complete_and_preserves_fields() {
        let mut keys = std::collections::HashSet::new();
        fn fields(s: &str) -> Vec<&str> {
            let mut fields = s
                .split('{')
                .skip(1)
                .filter_map(|part| part.split_once('}').map(|p| p.0))
                .collect::<Vec<_>>();
            fields.sort_unstable();
            fields
        }
        for &(en, cn, tw) in MESSAGES {
            assert!(keys.insert(en), "duplicate: {en}");
            assert!(!cn.is_empty() && !tw.is_empty(), "missing: {en}");
            assert_eq!(fields(en), fields(cn), "Simplified Chinese: {en}");
            assert_eq!(fields(en), fields(tw), "Traditional Chinese: {en}");
            for (language, expected) in [
                (Language::English, en),
                (Language::SimplifiedChinese, cn),
                (Language::TraditionalChinese, tw),
            ] {
                assert_eq!(translate(language, en), expected);
                assert_eq!(
                    translate(language, "characters/我的角色.webp"),
                    "characters/我的角色.webp"
                );
            }
        }
        for kind in crate::authoring::commands::InsertKind::ALL {
            assert!(
                keys.contains(kind.label()),
                "missing Block label: {}",
                kind.label()
            );
        }
    }

    #[test]
    fn substitutions_preserve_names_and_unknown_diagnostics() {
        assert_eq!(
            format(
                "{name}: {detail}",
                &[("name", "{detail}".into()), ("detail", "Saved".into())]
            ),
            "{detail}: Saved"
        );
        assert_eq!(format("Unrecognized {field}", &[]), "Unrecognized {field}");
    }
}
