//! Bundled CJK UI fonts. The Manager shows Simplified Chinese and Japanese
//! text whatever the system has installed, so both Noto Sans faces ship in
//! the binary and own the Han/kana fallback chain. Han code points are shared
//! between the two languages but drawn differently, so the face listed first
//! for `Hani` follows the UI language: JP for ja, SC otherwise.
//! OpenHarmony also uses these faces for the generic UI fonts before the first
//! window is constructed. Slint's software renderer assumes a font is
//! available even when Fontique cannot discover fonts through Fontconfig.

use std::{cell::OnceCell, sync::Arc};

use slint::fontique_011::{
    fontique::{Blob, FallbackKey, FamilyId, GenericFamily, Script},
    shared_collection,
};

use crate::i18n::Locale;

static NOTO_SANS_SC: &[u8] = include_bytes!("../../../../Assets/Fonts/NotoSansSC-Variable.ttf");
static NOTO_SANS_JP: &[u8] = include_bytes!("../../../../Assets/Fonts/NotoSansJP-Variable.ttf");

struct Families {
    sc: Vec<FamilyId>,
    jp: Vec<FamilyId>,
}

thread_local! {
    // Fontique family IDs belong to the Slint context on this UI thread.
    static FAMILIES: OnceCell<Families> = const { OnceCell::new() };
}

/// Orders the CJK fallback chain for `locale`, registering the bundled faces
/// on first use. Needs an installed Slint platform.
pub(crate) fn apply(locale: Locale) {
    apply_families(locale, cfg!(target_env = "ohos"));
}

fn apply_families(locale: Locale, use_bundled_generics: bool) {
    let mut collection = shared_collection();
    FAMILIES.with(|families| {
        let families = families.get_or_init(|| {
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
        let (first, second) = match locale {
            Locale::Ja => (&families.jp, &families.sc),
            Locale::ZhCn | Locale::En => (&families.sc, &families.jp),
        };
        let ordered = || first.iter().chain(second).copied();
        if use_bundled_generics {
            for generic in [
                GenericFamily::SansSerif,
                GenericFamily::SystemUi,
                GenericFamily::UiSansSerif,
            ] {
                collection.set_generic_families(generic, ordered());
            }
        }
        collection.set_fallbacks(
            FallbackKey::new(Script::from_bytes(*b"Hani"), None),
            ordered(),
        );
        for kana in [*b"Hira", *b"Kana"] {
            collection.set_fallbacks(
                FallbackKey::new(Script::from_bytes(kana), None),
                families.jp.iter().copied(),
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use i_slint_core::{graphics::FontRequest, window::WindowInner};
    use slint::{
        fontique_011::fontique::{Collection, CollectionOptions},
        platform::{
            Platform, WindowAdapter,
            software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
        },
    };
    use std::rc::Rc;

    struct SoftwarePlatform(Rc<MinimalSoftwareWindow>);

    impl Platform for SoftwarePlatform {
        fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
            Ok(self.0.clone())
        }
    }

    #[test]
    fn software_manager_starts_without_system_fonts() {
        let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        slint::platform::set_platform(Box::new(SoftwarePlatform(window.clone()))).unwrap();
        let context = WindowInner::from_pub(window.window()).context();
        context.font_context().borrow_mut().collection = Collection::new(CollectionOptions {
            shared: true,
            system_fonts: false,
        });
        let request = FontRequest {
            family: Some("Roboto".into()),
            ..Default::default()
        };
        let match_font = || {
            let mut fonts = context.font_context().borrow_mut();
            let parley = &mut **fonts;
            request.query_fontique(&mut parley.collection, &mut parley.source_cache)
        };

        assert!(match_font().is_none());
        apply_families(Locale::ZhCn, true);
        let sc = match_font().expect("bundled default font").family.0;
        let _manager = crate::ManagerWindow::new().unwrap();
        const WIDTH: usize = 360;
        const HEIGHT: usize = 780;
        window.set_size(slint::PhysicalSize::new(WIDTH as u32, HEIGHT as u32));
        let mut pixels = vec![slint::Rgb8Pixel::default(); WIDTH * HEIGHT];
        assert!(window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, WIDTH);
        }));
        assert!(pixels.iter().any(|pixel| *pixel != pixels[0]));

        apply_families(Locale::Ja, true);
        let jp = match_font().expect("bundled Japanese UI font").family.0;
        assert_ne!(sc, jp);
        assert_eq!(
            shared_collection()
                .fallback_families(FallbackKey::new(Script::from_bytes(*b"Hani"), None))
                .next(),
            Some(jp)
        );
    }
}
