//! Bundled CJK UI fonts. The Manager shows Simplified Chinese and Japanese
//! text whatever the system has installed, so both Noto Sans faces ship in
//! the binary and own the Han/kana fallback chain. Han code points are shared
//! between the two languages but drawn differently, so the face listed first
//! for `Hani` follows the UI language: JP for ja, SC otherwise.

use std::sync::{Arc, LazyLock};

use slint::fontique_011::{
    fontique::{Blob, FallbackKey, FamilyId, Script},
    shared_collection,
};

use crate::i18n::Locale;

/// Also rasterizes the resale notice, so the face is embedded once.
pub static NOTO_SANS_SC: &[u8] = include_bytes!("../../../../Assets/Fonts/NotoSansSC-Variable.ttf");
static NOTO_SANS_JP: &[u8] = include_bytes!("../../../../Assets/Fonts/NotoSansJP-Variable.ttf");

struct Families {
    sc: Vec<FamilyId>,
    jp: Vec<FamilyId>,
}

/// Registered on first access, which [`apply`] makes only once the Slint
/// platform exists.
static FAMILIES: LazyLock<Families> = LazyLock::new(|| {
    let mut collection = shared_collection();
    let mut register = |data: &'static [u8]| {
        collection
            .register_fonts(Blob::new(Arc::new(data)), None)
            .into_iter()
            .map(|(family, _)| family)
            .collect()
    };
    Families {
        sc: register(NOTO_SANS_SC),
        jp: register(NOTO_SANS_JP),
    }
});

/// Orders the CJK fallback chain for `locale`, registering the bundled faces
/// on first use. Needs the Slint platform, i.e. a component already exists.
pub(crate) fn apply(locale: Locale) {
    let families = &*FAMILIES;
    let mut collection = shared_collection();
    let (first, second) = match locale {
        Locale::Ja => (&families.jp, &families.sc),
        Locale::ZhCn | Locale::En => (&families.sc, &families.jp),
    };
    collection.set_fallbacks(
        FallbackKey::new(Script::from_bytes(*b"Hani"), None),
        first.iter().chain(second).copied(),
    );
    for kana in [*b"Hira", *b"Kana"] {
        collection.set_fallbacks(
            FallbackKey::new(Script::from_bytes(kana), None),
            families.jp.iter().copied(),
        );
    }
}
