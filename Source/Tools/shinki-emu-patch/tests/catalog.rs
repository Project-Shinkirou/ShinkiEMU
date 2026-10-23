use serde_json::{json, Value};
use shinki_emu_patch::{list_recipes, PatchErrorKind};
use std::fs;

fn listing(records: Value) -> Result<Vec<shinki_emu_patch::RecipeListing>, PatchErrorKind> {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("catalog.json"),
        serde_json::to_vec(&records).unwrap(),
    )
    .unwrap();
    list_recipes(root.path()).map_err(|error| error.kind())
}

fn recipe() -> Value {
    json!({
        "id": "public-recipe",
        "title": "Public recipe",
        "script": "public.luau",
        "vn_ids": ["v17", "v18"],
        "emulators": ["shinki.emu.musica"],
    })
}

#[test]
fn catalog_rejects_missing_or_malformed_targets() {
    for (field, value) in [
        ("vn_ids", json!([])),
        ("vn_ids", json!(["17"])),
        ("vn_ids", json!(["v"])),
        ("vn_ids", json!(["v1234567890123"])),
        ("vn_ids", json!(["v17", "v17"])),
        ("vn_ids", json!("v17")),
        ("emulators", json!([])),
        ("emulators", json!([" "])),
        ("emulators", json!(["rfvp", "rfvp"])),
        ("emulators", json!(["reminori"])),
        ("emulators", json!(["unknown-core"])),
    ] {
        let mut record = recipe();
        record[field] = value.clone();
        assert_eq!(
            listing(json!([record])).unwrap_err(),
            PatchErrorKind::InvalidRequest,
            "{field} = {value}"
        );
    }
    for field in ["vn_ids", "emulators"] {
        let mut record = recipe();
        record.as_object_mut().unwrap().remove(field);
        assert_eq!(
            listing(json!([record])).unwrap_err(),
            PatchErrorKind::InvalidRequest
        );
    }
}
