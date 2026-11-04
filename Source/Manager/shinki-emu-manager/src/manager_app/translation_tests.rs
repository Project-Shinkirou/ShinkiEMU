use super::*;
use std::cell::RefCell;

/// In-memory secret store standing in for the platform credential store.
#[derive(Default)]
struct MemorySecrets(RefCell<BTreeMap<String, String>>);

impl ProviderSecrets for MemorySecrets {
    fn store(&self, reference: &str, secret: &str) -> Result<(), String> {
        self.0.borrow_mut().insert(reference.into(), secret.into());
        Ok(())
    }

    fn resolve(&self, reference: &str) -> Result<String, String> {
        self.0
            .borrow()
            .get(reference)
            .cloned()
            .ok_or_else(|| "SHINKI_EMU_TRANSLATION_SECRET_UNAVAILABLE".into())
    }

    fn delete(&self, reference: &str) -> Result<(), String> {
        self.0.borrow_mut().remove(reference);
        Ok(())
    }
}

fn controller(root: &Path) -> ShinkiEmuManagerController {
    ShinkiEmuManagerController::open_library(root.into(), FrameMailbox::new(), Vec::new()).unwrap()
}

fn form(base_url: &str, model: &str) -> TranslationFormViewModel {
    TranslationFormViewModel {
        provider: "openai".into(),
        base_url: base_url.into(),
        openai_api: "chat_completions".into(),
        model: model.into(),
        language: "zh-CN".into(),
        timeout_s: 15,
        custom_prompt: String::new(),
        context_segments: 8,
        context_chars: 6000,
    }
}

#[test]
fn legacy_profile_becomes_the_enabled_first_card_and_keeps_its_key() {
    let root = tempfile::tempdir().unwrap();
    let legacy = TranslationProfile {
        model: "gpt-4.1-mini".into(),
        secret_reference: "translation.b".into(),
        ..TranslationProfile::default()
    };
    {
        let mut library = Library::open(root.path().join("library.sqlite3")).unwrap();
        library.set_translation_profile(&legacy).unwrap();
    }
    let app = controller(root.path());
    let book = app.provider_book().unwrap();
    assert_eq!(book.providers.len(), 1);
    let card = &book.providers[0];
    assert_eq!(book.active.as_deref(), Some(card.id.as_str()));
    assert_eq!((card.name.as_str(), card.preset.as_str()), ("OpenAI", "openai"));
    assert_eq!(card.profile, legacy, "profile and secret reference are unchanged");
    assert_eq!(app.profile().unwrap(), Some(legacy.clone()));
    let views = app.translation_provider_views().unwrap();
    assert!(views[0].active);
    assert_eq!(views[0].form.model, "gpt-4.1-mini");
    drop(app);

    // Migration runs once; later opens keep the cards as they are.
    let app = controller(root.path());
    assert_eq!(app.provider_book().unwrap(), book);
}

#[test]
fn legacy_profile_on_a_custom_endpoint_is_named_after_its_host() {
    let root = tempfile::tempdir().unwrap();
    {
        let mut library = Library::open(root.path().join("library.sqlite3")).unwrap();
        library
            .set_translation_profile(&TranslationProfile {
                base_url: "https://llm.example.net/v1".into(),
                model: "m".into(),
                ..TranslationProfile::default()
            })
            .unwrap();
    }
    let book = controller(root.path()).provider_book().unwrap();
    assert_eq!(book.providers[0].name, "llm.example.net");
    assert_eq!(book.providers[0].preset, "custom");
}

#[test]
fn no_legacy_profile_means_no_cards() {
    let root = tempfile::tempdir().unwrap();
    let app = controller(root.path());
    assert_eq!(app.provider_book().unwrap(), ProviderBook::default());
    assert_eq!(app.profile().unwrap(), None);
}

#[test]
fn cards_save_switch_duplicate_and_delete_with_their_keys() {
    let root = tempfile::tempdir().unwrap();
    let mut app = controller(root.path());
    let secrets = MemorySecrets::default();
    let openai = form("https://api.openai.com/v1", "gpt-4.1-mini");
    let deepseek = form("https://api.deepseek.com/v1", "deepseek-chat");

    assert_eq!(
        app.save_translation_provider_with(&secrets, "", "OpenAI", "openai", &openai, "")
            .unwrap_err(),
        "SHINKI_EMU_TRANSLATION_SECRET_REQUIRED",
        "a new card needs a key"
    );
    let first = app
        .save_translation_provider_with(&secrets, "", "OpenAI", "openai", &openai, "key-a")
        .unwrap();
    let second = app
        .save_translation_provider_with(&secrets, "", "DeepSeek", "deepseek", &deepseek, "key-b")
        .unwrap();
    assert_ne!(first, second);
    let book = app.provider_book().unwrap();
    assert_eq!(book.active.as_deref(), Some(first.as_str()), "the first card is enabled");
    let reference = |app: &ShinkiEmuManagerController, id: &str| {
        app.provider_book()
            .unwrap()
            .get(id)
            .unwrap()
            .profile
            .secret_reference
            .clone()
    };
    assert_eq!(secrets.resolve(&reference(&app, &first)).unwrap(), "key-a");
    assert_eq!(secrets.resolve(&reference(&app, &second)).unwrap(), "key-b");

    // Switching changes the profile sessions use and the mirrored row.
    app.activate_translation_provider(&second).unwrap();
    assert_eq!(app.profile().unwrap().unwrap().model, "deepseek-chat");
    assert_eq!(
        app.library.translation_profile().unwrap().unwrap().model,
        "deepseek-chat"
    );
    let views = app.translation_provider_views().unwrap();
    assert_eq!(
        views.iter().map(|view| view.active).collect::<Vec<_>>(),
        [false, true]
    );

    // Editing without a key keeps it; a new key replaces and deletes the old one.
    let renamed = form("https://api.deepseek.com/v1", "deepseek-reasoner");
    let before = reference(&app, &second);
    app.save_translation_provider_with(&secrets, &second, "DeepSeek R", "deepseek", &renamed, "")
        .unwrap();
    assert_eq!(reference(&app, &second), before);
    assert_eq!(app.profile().unwrap().unwrap().model, "deepseek-reasoner");
    app.save_translation_provider_with(&secrets, &second, "DeepSeek R", "deepseek", &renamed, "key-c")
        .unwrap();
    let after = reference(&app, &second);
    assert_ne!(after, before);
    assert_eq!(secrets.resolve(&after).unwrap(), "key-c");
    assert!(secrets.resolve(&before).is_err());
    assert_eq!(
        app.library.translation_profile().unwrap().unwrap().secret_reference,
        after
    );

    // A copy carries its own copy of the key and starts disabled.
    let copy = app.duplicate_translation_provider_with(&secrets, &first).unwrap();
    let book = app.provider_book().unwrap();
    let copied = book.get(&copy).unwrap();
    assert_eq!(copied.name, tr!("{name} 副本", name = "OpenAI"));
    assert_ne!(copied.profile.secret_reference, reference(&app, &first));
    assert_eq!(secrets.resolve(&copied.profile.secret_reference).unwrap(), "key-a");
    assert_eq!(book.active.as_deref(), Some(second.as_str()));

    // The enabled card stays; others go with their keys.
    assert_eq!(
        app.delete_translation_provider_with(&secrets, &second).unwrap_err(),
        "SHINKI_EMU_TRANSLATION_PROVIDER_ACTIVE"
    );
    let first_reference = reference(&app, &first);
    app.delete_translation_provider_with(&secrets, &first).unwrap();
    assert!(app.provider_book().unwrap().get(&first).is_none());
    assert!(secrets.resolve(&first_reference).is_err());
    assert_eq!(
        app.activate_translation_provider(&first).unwrap_err(),
        "SHINKI_EMU_TRANSLATION_PROVIDER_UNKNOWN"
    );
}

#[test]
fn model_list_failures_name_their_cause() {
    let unauthorized = connection_failure(&TranslationError::HttpStatus(401));
    let forbidden = connection_failure(&TranslationError::HttpStatus(403));
    let missing = connection_failure(&TranslationError::HttpStatus(404));
    let method = connection_failure(&TranslationError::HttpStatus(405));
    let parse = connection_failure(&TranslationError::Protocol("model list response is invalid"));
    let timeout = connection_failure(&TranslationError::Timeout);
    assert_eq!(
        unauthorized,
        tr!("API Key 无效或没有权限（HTTP {code}），请检查密钥", code = 401)
    );
    assert_eq!(
        forbidden,
        tr!("API Key 无效或没有权限（HTTP {code}），请检查密钥", code = 403)
    );
    assert_eq!(
        missing,
        tr!("找不到模型列表接口（HTTP {code}），请检查 Base URL 和接口类型", code = 404)
    );
    assert_eq!(
        method,
        tr!("找不到模型列表接口（HTTP {code}），请检查 Base URL 和接口类型", code = 405)
    );
    assert_eq!(parse, tr!("返回内容不是模型列表，请检查 Base URL 和接口类型"));
    assert_eq!(timeout, tr!("连接超时，请检查网络或调大超时时间"));
    let all = [&unauthorized, &missing, &parse, &timeout];
    for (index, left) in all.iter().enumerate() {
        for right in &all[index + 1..] {
            assert_ne!(left, right);
        }
    }
}
