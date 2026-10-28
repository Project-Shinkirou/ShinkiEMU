use super::*;
use roukaku_core::Hash256;
use roukaku_text::PackagedFont;

/// Face substituted when the configured family is not installed. GDI's font
/// mapper also substitutes a face of the requested charset instead of
/// failing; this one ships with the core, so every platform resolves it.
const FALLBACK_FAMILY: &str = "Noto Sans JP";
const FALLBACK_FONT: &[u8] =
    include_bytes!("../../../../../../Assets/Fonts/NotoSansJP-Variable.ttf");

/// Installed system faces plus the bundled fallback. Android keeps its
/// system faces in `/system/fonts`, which fontdb does not scan itself.
pub(super) fn system_database() -> fontdb::Database {
    let mut database = fontdb::Database::new();
    database.load_system_fonts();
    database.load_fonts_dir("/system/fonts");
    database.load_font_data(FALLBACK_FONT.to_vec());
    database
}

fn matching<'a>(
    database: &'a fontdb::Database,
    name: &str,
    weight: u16,
) -> Option<&'a fontdb::FaceInfo> {
    database
        .faces()
        .filter(|face| {
            face.families
                .iter()
                .any(|(family, _)| family.eq_ignore_ascii_case(name))
        })
        .min_by_key(|face| {
            (
                face.style != fontdb::Style::Normal,
                face.weight.0.abs_diff(weight),
            )
        })
}

fn face<'a>(
    database: &'a fontdb::Database,
    name: &str,
    weight: u16,
) -> Result<&'a fontdb::FaceInfo, CoreError> {
    // Empty names refer to the native process configuration. Do not substitute
    // a platform-dependent generic family until that configuration is bound.
    if name.is_empty() {
        return Err(error("SHINKI_EMU_CMVS_DEFAULT_FONT_UNBOUND"));
    }
    if let Some(face) = matching(database, name, weight) {
        return Ok(face);
    }
    let fallback = matching(database, FALLBACK_FAMILY, weight)
        .ok_or_else(|| error("SHINKI_EMU_CMVS_FONT_MISSING"))?;
    tracing::warn!(
        event = "cmvs.text.font_substituted",
        requested = name,
        substitute = FALLBACK_FAMILY
    );
    Ok(fallback)
}

pub(super) fn has_distinct_bold_face(
    database: &fontdb::Database,
    name: &str,
) -> Result<bool, CoreError> {
    Ok(face(database, name, 400)?.id != face(database, name, 600)?.id)
}

pub(super) fn load(
    database: &fontdb::Database,
    name: &str,
    weight: u16,
) -> Result<PackagedFont, CoreError> {
    let face = face(database, name, weight)?;
    let (bytes, index) = database
        .with_face_data(face.id, |bytes, index| (bytes.to_vec(), index))
        .ok_or_else(|| error("SHINKI_EMU_CMVS_FONT_READ"))?;
    let hash = Hash256::from_sha256(&bytes);
    Ok(PackagedFont {
        asset_id: format!("asset:/cmvs/font/{hash}/{index}"),
        family: face.families[0].0.clone(),
        face_index: index,
        hash,
        license_id: "system-installed-local-use".into(),
        subset: None,
        coverage: roukaku_text::font_unicode_coverage(&bytes, index)
            .map_err(|_| error("SHINKI_EMU_CMVS_FONT_COVERAGE"))?,
        targets: vec!["shinki-emu-cmvs".into()],
        profiles: vec!["cmvs.native".into()],
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_family_substitutes_the_bundled_japanese_face() {
        let mut database = fontdb::Database::new();
        database.load_font_data(FALLBACK_FONT.to_vec());
        let font = load(&database, "ＭＳ ゴシック", 400).unwrap();
        assert_eq!(font.family, FALLBACK_FAMILY);
        assert!(!has_distinct_bold_face(&database, "ＭＳ ゴシック").unwrap());
        assert!(load(&fontdb::Database::new(), "ＭＳ ゴシック", 400).is_err());
        assert!(load(&database, "", 400).is_err());
    }
}
