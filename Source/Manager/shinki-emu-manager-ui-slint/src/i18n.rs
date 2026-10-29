//! Manager UI language. zh-CN is the source language: every msgid is the
//! zh-CN text, and `translations/<lang>/LC_MESSAGES/*.po` supply en and ja.
//! The Slint `@tr()` strings and the Rust [`tr!`](crate::tr) strings share
//! those files; build.rs fails on any untranslated or missing entry.

use std::{
    collections::BTreeMap,
    fmt::Display,
    sync::atomic::{AtomicU8, Ordering},
};

include!(concat!(env!("OUT_DIR"), "/i18n_catalog.rs"));

/// A language the Manager ships.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    ZhCn,
    En,
    Ja,
}

impl Locale {
    /// The persisted `AppearanceSettings::language` value.
    pub fn id(self) -> &'static str {
        match self {
            Self::ZhCn => "zh-CN",
            Self::En => "en",
            Self::Ja => "ja",
        }
    }

    /// The `translations/<lang>` directory, or "" for the zh-CN source text.
    fn bundle(self) -> &'static str {
        match self {
            Self::ZhCn => "",
            Self::En => "en",
            Self::Ja => "ja",
        }
    }

    /// Column in the generated `TRANSLATIONS` table.
    fn column(self) -> Option<usize> {
        match self {
            Self::ZhCn => None,
            Self::En => Some(0),
            Self::Ja => Some(1),
        }
    }

    /// Maps a BCP 47 / POSIX locale tag: zh-* → zh-CN, ja-* → ja, else en.
    pub fn from_tag(tag: &str) -> Self {
        let language = tag
            .split(['-', '_', '.', '@'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        match language.as_str() {
            "zh" => Self::ZhCn,
            "ja" => Self::Ja,
            _ => Self::En,
        }
    }

    /// Resolves the persisted setting ("system", "zh-CN", "en" or "ja")
    /// against the system locale tag, if one is known.
    pub fn resolve(setting: &str, system: Option<&str>) -> Self {
        match setting {
            "zh-CN" => Self::ZhCn,
            "en" => Self::En,
            "ja" => Self::Ja,
            _ => system.map_or(Self::En, Self::from_tag),
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// The language Rust-side strings are rendered in.
pub fn current() -> Locale {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Locale::En,
        2 => Locale::Ja,
        _ => Locale::ZhCn,
    }
}

fn set(locale: Locale) {
    CURRENT.store(
        match locale {
            Locale::ZhCn => 0,
            Locale::En => 1,
            Locale::Ja => 2,
        },
        Ordering::Relaxed,
    );
}

/// Renders Rust-side text in the system language until [`apply`] runs, so a
/// startup failure before the settings load is reported in a readable language.
pub fn select_system() {
    set(Locale::resolve("system", sys_locale::get_locale().as_deref()));
}

/// Applies the persisted language setting to the Rust catalog, the Slint
/// bundle and the CJK font fallback order. Call after the first Slint
/// component exists.
pub fn apply(setting: &str) -> Locale {
    let system = sys_locale::get_locale();
    let locale = Locale::resolve(setting, system.as_deref());
    set(locale);
    if let Err(error) = slint::select_bundled_translation(locale.bundle()) {
        tracing::warn!(?error, language = locale.id(), "SHINKI_EMU_I18N_SELECT_FAILED");
    }
    crate::fonts::apply(locale);
    locale
}

/// Text from a metadata map keyed by language id. The metadata schema requires
/// zh-CN and en; ja is optional and falls back to en, as on the portal.
pub fn pick(texts: &BTreeMap<String, String>) -> &str {
    texts
        .get(current().id())
        .or_else(|| texts.get("en"))
        .or_else(|| texts.get("zh-CN"))
        .map_or("", String::as_str)
}

const fn str_cmp(a: &str, b: &str) -> core::cmp::Ordering {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut i = 0;
    while i < a.len() && i < b.len() {
        if a[i] != b[i] {
            return if a[i] < b[i] {
                core::cmp::Ordering::Less
            } else {
                core::cmp::Ordering::Greater
            };
        }
        i += 1;
    }
    if a.len() == b.len() {
        core::cmp::Ordering::Equal
    } else if a.len() < b.len() {
        core::cmp::Ordering::Less
    } else {
        core::cmp::Ordering::Greater
    }
}

/// Catalog index of `msgid`. Evaluated in a const by [`tr!`](crate::tr),
/// so a msgid missing from the .po files is a compile error.
#[doc(hidden)]
pub const fn index_of(msgid: &str) -> usize {
    let (mut lo, mut hi) = (0, MSGIDS.len());
    while lo < hi {
        let mid = (lo + hi) / 2;
        match str_cmp(MSGIDS[mid], msgid) {
            core::cmp::Ordering::Less => lo = mid + 1,
            core::cmp::Ordering::Greater => hi = mid,
            core::cmp::Ordering::Equal => return mid,
        }
    }
    panic!("SHINKI_EMU_I18N_MISSING: add this msgid to every translations/{{en,ja}}/LC_MESSAGES/*.po")
}

#[doc(hidden)]
pub fn text(index: usize) -> &'static str {
    match current().column() {
        Some(column) => TRANSLATIONS[column][index],
        None => MSGIDS[index],
    }
}

/// Substitutes `{name}` placeholders.
#[doc(hidden)]
pub fn format(index: usize, args: &[(&str, &dyn Display)]) -> String {
    let template = text(index);
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let tail = &rest[start + 1..];
        let Some(end) = tail.find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let name = &tail[..end];
        match args.iter().find(|(key, _)| *key == name) {
            Some((_, value)) => {
                use std::fmt::Write;
                let _ = write!(out, "{value}");
            }
            None => out.push_str(&rest[start..start + end + 2]),
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Translated Manager text. `tr!("…")` yields `&'static str`;
/// `tr!("…{name}…", name = value)` yields a `String`.
#[macro_export]
macro_rules! tr {
    ($msgid:literal) => {{
        const INDEX: usize = $crate::i18n::index_of($msgid);
        $crate::i18n::text(INDEX)
    }};
    ($msgid:literal, $($name:ident = $value:expr),+ $(,)?) => {{
        const INDEX: usize = $crate::i18n::index_of($msgid);
        $crate::i18n::format(
            INDEX,
            &[$((stringify!($name), &$value as &dyn ::std::fmt::Display)),+],
        )
    }};
}

#[cfg(test)]
mod tests {
    use super::Locale;

    #[test]
    fn system_tag_selects_locale_and_persisted_choice_wins() {
        assert_eq!(Locale::resolve("system", Some("zh-Hans-CN")), Locale::ZhCn);
        assert_eq!(Locale::resolve("system", Some("zh_TW.UTF-8")), Locale::ZhCn);
        assert_eq!(Locale::resolve("system", Some("ja-JP")), Locale::Ja);
        assert_eq!(Locale::resolve("system", Some("fr-FR")), Locale::En);
        assert_eq!(Locale::resolve("system", None), Locale::En);
        assert_eq!(Locale::resolve("ja", Some("zh-CN")), Locale::Ja);
        assert_eq!(Locale::resolve("zh-CN", Some("en-US")), Locale::ZhCn);
        assert_eq!(Locale::resolve("en", Some("ja-JP")), Locale::En);
    }
}
