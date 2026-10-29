use std::{
    collections::{BTreeMap, HashMap},
    env, fs,
    path::{Path, PathBuf},
};

use rspolib::prelude::*;

/// Bundled translation languages. zh-CN is the source language (msgid = zh text).
const LANGUAGES: [&str; 2] = ["en", "ja"];
const DOMAIN: &str = "shinki-emu-manager-ui-slint";

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let translations = manifest.join("translations");
    let catalog = load_catalog(&translations);
    check_slint_sources(&manifest.join("ui"), &catalog);
    write_rust_catalog(&catalog);

    let material = manifest.join("material-1.1.0/material.slint");
    let config = slint_build::CompilerConfiguration::new()
        .with_style("material".into())
        .with_library_paths(HashMap::from([("material".to_string(), material)]))
        .with_bundled_translations(translations)
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    slint_build::compile_with_config("ui/main.slint", config).expect("SHINKI_EMU_SLINT_COMPILE");
}

/// msgid -> one translation per entry of `LANGUAGES`.
type Catalog = BTreeMap<String, Vec<String>>;

/// Every msgid must be translated in every language, with the same placeholders.
fn load_catalog(root: &Path) -> Catalog {
    let mut per_language = Vec::new();
    for language in LANGUAGES {
        let path = root.join(language).join("LC_MESSAGES").join(format!("{DOMAIN}.po"));
        println!("cargo:rerun-if-changed={}", path.display());
        let file = rspolib::pofile(path.as_path())
            .unwrap_or_else(|e| panic!("SHINKI_EMU_I18N_PO_PARSE {}: {e}", path.display()));
        let mut entries = BTreeMap::new();
        for entry in file.entries {
            assert!(
                entry.msgctxt.is_none() && entry.msgid_plural.is_none(),
                "SHINKI_EMU_I18N_UNSUPPORTED_ENTRY {language}: {:?}",
                entry.msgid
            );
            if entry.obsolete {
                continue;
            }
            let translated = entry.translated();
            let msgstr = entry.msgstr.clone().unwrap_or_default();
            assert!(
                translated,
                "SHINKI_EMU_I18N_UNTRANSLATED {language}: {:?}",
                entry.msgid
            );
            assert_eq!(
                placeholders(&entry.msgid),
                placeholders(&msgstr),
                "SHINKI_EMU_I18N_PLACEHOLDER_MISMATCH {language}: {:?}",
                entry.msgid
            );
            assert!(
                entries.insert(entry.msgid.clone(), msgstr).is_none(),
                "SHINKI_EMU_I18N_DUPLICATE {language}: {:?}",
                entry.msgid
            );
        }
        per_language.push(entries);
    }
    let mut catalog = Catalog::new();
    for msgid in per_language.iter().flat_map(BTreeMap::keys) {
        if catalog.contains_key(msgid) {
            continue;
        }
        let row = per_language
            .iter()
            .zip(LANGUAGES)
            .map(|(entries, language)| {
                entries
                    .get(msgid)
                    .cloned()
                    .unwrap_or_else(|| panic!("SHINKI_EMU_I18N_MISSING {language}: {msgid:?}"))
            })
            .collect();
        catalog.insert(msgid.clone(), row);
    }
    catalog
}

/// `{}`, `{0}` and `{name}` tokens, sorted, so reordering is allowed.
fn placeholders(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        let Some(len) = rest[start..].find('}') else { break };
        found.push(rest[start..start + len + 1].to_owned());
        rest = &rest[start + len + 1..];
    }
    found.sort();
    found
}

/// Every `@tr("…")` in the Slint sources needs a catalog entry; Slint itself
/// would silently fall back to the zh source text.
fn check_slint_sources(ui: &Path, catalog: &Catalog) {
    println!("cargo:rerun-if-changed={}", ui.display());
    let mut missing = Vec::new();
    let mut stack = vec![ui.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).expect("SHINKI_EMU_I18N_UI_DIR") {
            let path = entry.expect("SHINKI_EMU_I18N_UI_DIR").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "slint") {
                let source = fs::read_to_string(&path).expect("SHINKI_EMU_I18N_UI_READ");
                for msgid in tr_literals(&source) {
                    if !catalog.contains_key(&msgid) {
                        missing.push(format!("{}: {msgid:?}", path.display()));
                    }
                }
            }
        }
    }
    missing.sort();
    assert!(
        missing.is_empty(),
        "SHINKI_EMU_I18N_MISSING (add to every translations/<lang>/LC_MESSAGES/{DOMAIN}.po):\n{}",
        missing.join("\n")
    );
}

/// First string literal of each `@tr(` call, unescaped like the Slint lexer.
fn tr_literals(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = source;
    while let Some(at) = rest.find("@tr(") {
        rest = rest[at + 4..].trim_start();
        let Some(body) = rest.strip_prefix('"') else { continue };
        let mut text = String::new();
        let mut chars = body.char_indices();
        while let Some((i, c)) = chars.next() {
            match c {
                '"' => {
                    rest = &body[i + 1..];
                    break;
                }
                '\\' => match chars.next().map(|(_, c)| c) {
                    Some('n') => text.push('\n'),
                    Some('t') => text.push('\t'),
                    Some(other) => text.push(other),
                    None => {}
                },
                c => text.push(c),
            }
        }
        out.push(text);
    }
    out
}

/// Sorted msgid table plus one translation column per language, consumed by `src/i18n.rs`.
fn write_rust_catalog(catalog: &Catalog) {
    let mut code = String::from("pub(crate) static MSGIDS: &[&str] = &[\n");
    for msgid in catalog.keys() {
        code.push_str(&format!("    {msgid:?},\n"));
    }
    code.push_str("];\npub(crate) static TRANSLATIONS: &[&[&str]] = &[\n");
    for column in 0..LANGUAGES.len() {
        code.push_str("    &[\n");
        for row in catalog.values() {
            code.push_str(&format!("        {:?},\n", row[column]));
        }
        code.push_str("    ],\n");
    }
    code.push_str("];\n");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("SHINKI_EMU_I18N_OUT_DIR"));
    fs::write(out.join("i18n_catalog.rs"), code).expect("SHINKI_EMU_I18N_CATALOG_WRITE");
}
