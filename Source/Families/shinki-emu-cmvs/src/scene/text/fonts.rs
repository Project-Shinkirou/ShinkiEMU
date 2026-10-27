use super::*;
use roukaku_core::Hash256;
use roukaku_text::PackagedFont;

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
        .ok_or_else(|| error("SHINKI_EMU_CMVS_FONT_MISSING"))
}

#[cfg(windows)]
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
