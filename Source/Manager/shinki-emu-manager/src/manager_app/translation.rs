//! Translation service providers. The user keeps several provider cards and
//! enables one of them; the enabled card's profile is what a game session
//! uses. Cards live in the `translation.providers` app state; each card's API
//! key lives in the platform secret store under its own reference.
//!
//! A Manager that only knew the single `translation_profile` row migrates it
//! into the first card, keeping its secret reference, and still mirrors the
//! enabled card into that row.

use super::*;
use serde::{Deserialize, Serialize};
use shinki_emu_manager_ui_slint::tr;
use shinki_emu_manager_ui_slint::{TranslationFormViewModel, TranslationProviderViewModel};
use shinki_emu_translation::TranslationError;
use std::sync::Arc;

pub(super) const TRANSLATION_PROVIDERS_KEY: &str = "translation.providers";
const TEST_SECRET_REFERENCE: &str = "translation.test";
/// Model name used to validate a profile whose model is still empty while
/// its model list is fetched; the list request does not send it.
const MODEL_LIST_PLACEHOLDER: &str = "model-list";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProviderBook {
    pub(super) active: Option<String>,
    pub(super) providers: Vec<StoredProvider>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredProvider {
    pub(super) id: String,
    pub(super) name: String,
    /// Preset the card was created from, `custom` otherwise.
    pub(super) preset: String,
    pub(super) profile: TranslationProfile,
}

impl ProviderBook {
    fn get(&self, id: &str) -> Option<&StoredProvider> {
        self.providers.iter().find(|provider| provider.id == id)
    }

    fn active(&self) -> Option<&StoredProvider> {
        self.active.as_deref().and_then(|id| self.get(id))
    }

    fn next_id(&self) -> String {
        let next = self
            .providers
            .iter()
            .filter_map(|provider| provider.id.strip_prefix("provider-")?.parse::<u32>().ok())
            .max()
            .unwrap_or(0)
            + 1;
        format!("provider-{next}")
    }
}

/// API keys of provider cards; the platform store in the Manager.
pub(super) trait ProviderSecrets {
    fn store(&self, reference: &str, secret: &str) -> Result<(), String>;
    fn resolve(&self, reference: &str) -> Result<String, String>;
    fn delete(&self, reference: &str) -> Result<(), String>;
}

impl ProviderSecrets for ManagerSecretStore {
    fn store(&self, reference: &str, secret: &str) -> Result<(), String> {
        ManagerSecretStore::store(self, reference, secret).map_err(|error| error.to_string())
    }

    fn resolve(&self, reference: &str) -> Result<String, String> {
        SecretResolver::resolve(self, reference).map_err(|error| error.to_string())
    }

    fn delete(&self, reference: &str) -> Result<(), String> {
        ManagerSecretStore::delete(self, reference).map_err(|error| error.to_string())
    }
}

fn platform_secrets() -> Result<ManagerSecretStore, String> {
    ManagerSecretStore::open().map_err(|error| error.to_string())
}

/// A fresh secret reference for card `id`, distinct from `previous` so a new
/// key is stored before the old one is deleted.
fn next_secret_reference(id: &str, previous: Option<&str>) -> String {
    let side = if previous.is_some_and(|reference| reference.ends_with(".a")) {
        "b"
    } else {
        "a"
    };
    format!("translation.{id}.{side}")
}

/// Card name and preset for a profile saved before cards existed.
fn migrated_identity(profile: &TranslationProfile) -> (String, String) {
    let official = profile.base_url == profile.provider.default_base_url();
    let (preset, name) = match profile.provider {
        TranslationProvider::OpenAi => ("openai", "OpenAI"),
        TranslationProvider::Anthropic => ("anthropic", "Anthropic"),
        TranslationProvider::Google => ("google", "Google Gemini"),
    };
    if official {
        return (name.into(), preset.into());
    }
    let host = reqwest::Url::parse(&profile.base_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| name.to_owned());
    (host, "custom".into())
}

/// Turn a pre-card profile into the first, enabled card. Runs once: a book
/// that already exists is never rebuilt from the old row.
pub(super) fn migrate_provider_book(library: &mut Library) -> Result<(), String> {
    if library
        .app_state::<ProviderBook>(TRANSLATION_PROVIDERS_KEY)
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Ok(());
    }
    let Some(profile) = library
        .translation_profile()
        .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };
    let (name, preset) = migrated_identity(&profile);
    let id = ProviderBook::default().next_id();
    let book = ProviderBook {
        active: Some(id.clone()),
        providers: vec![StoredProvider {
            id,
            name,
            preset,
            profile,
        }],
    };
    library
        .set_app_state(TRANSLATION_PROVIDERS_KEY, &book)
        .map_err(|error| error.to_string())?;
    tracing::info!(event = "shinki.emu.manager.translation_providers_migrated");
    Ok(())
}

/// Localized reason for a failed model list request.
pub(super) fn connection_failure(error: &TranslationError) -> String {
    match error {
        TranslationError::HttpStatus(code @ (401 | 403)) => tr!(
            "API Key 无效或没有权限（HTTP {code}），请检查密钥",
            code = code
        ),
        TranslationError::HttpStatus(code @ (404 | 405)) => tr!(
            "找不到模型列表接口（HTTP {code}），请检查 Base URL 和接口类型",
            code = code
        ),
        TranslationError::HttpStatus(code) => {
            tr!("服务返回 HTTP {code}，请稍后再试", code = code)
        }
        TranslationError::Protocol(_) => {
            tr!("返回内容不是模型列表，请检查 Base URL 和接口类型").into()
        }
        TranslationError::Timeout => tr!("连接超时，请检查网络或调大超时时间").into(),
        TranslationError::Transport => tr!("无法连接到服务，请检查网络和 Base URL").into(),
        TranslationError::SecretUnavailable => tr!("读取 API Key 失败，请重新填写").into(),
        other => other.to_string(),
    }
}

struct FormSecret(String);

impl SecretResolver for FormSecret {
    fn resolve(&self, _reference: &str) -> Result<String, TranslationError> {
        Ok(self.0.clone())
    }
}

impl ShinkiEmuManagerController {
    pub(super) fn provider_book(&self) -> Result<ProviderBook, String> {
        Ok(self
            .library
            .app_state::<ProviderBook>(TRANSLATION_PROVIDERS_KEY)
            .map_err(|error| error.to_string())?
            .unwrap_or_default())
    }

    /// Persist the cards, then mirror the enabled one into the profile row
    /// older Manager builds read.
    fn write_provider_book(&mut self, book: &ProviderBook) -> Result<(), String> {
        self.library
            .set_app_state(TRANSLATION_PROVIDERS_KEY, book)
            .map_err(|error| error.to_string())?;
        match book.active() {
            Some(active) => self.library.set_translation_profile(&active.profile),
            None => self.library.clear_translation_profile(),
        }
        .map_err(|error| error.to_string())
    }

    /// The enabled card's profile.
    pub(super) fn profile(&self) -> Result<Option<TranslationProfile>, String> {
        Ok(self
            .provider_book()?
            .active()
            .map(|provider| provider.profile.clone()))
    }

    pub(super) fn translation_provider_views(
        &self,
    ) -> Result<Vec<TranslationProviderViewModel>, String> {
        let book = self.provider_book()?;
        Ok(book
            .providers
            .iter()
            .map(|provider| TranslationProviderViewModel {
                id: provider.id.clone(),
                name: provider.name.clone(),
                preset: provider.preset.clone(),
                active: book.active.as_deref() == Some(provider.id.as_str()),
                form: profile_form(Some(&provider.profile)),
            })
            .collect())
    }

    fn require_no_session(&self) -> Result<(), String> {
        if self.session_running() {
            return Err("SHINKI_EMU_TRANSLATION_CLOSE_GAME_BEFORE_CONFIGURATION".into());
        }
        Ok(())
    }

    /// Fetch the model list of the form's endpoint. An empty `secret` uses
    /// the key stored for card `provider_id`.
    pub(super) fn test_translation_connection(
        &mut self,
        provider_id: &str,
        form: &TranslationFormViewModel,
        secret: &str,
    ) -> Result<ManagerViewModel, String> {
        self.require_network()?;
        self.connection_test.take();
        let mut form = form.clone();
        if form.model.trim().is_empty() {
            form.model = MODEL_LIST_PLACEHOLDER.into();
        }
        let mut profile = profile_from_form(&form)?;
        let secrets: Arc<dyn SecretResolver> = if secret.is_empty() {
            profile.secret_reference = self
                .provider_book()?
                .get(provider_id)
                .ok_or("SHINKI_EMU_TRANSLATION_SECRET_REQUIRED")?
                .profile
                .secret_reference
                .clone();
            Arc::new(platform_secrets()?)
        } else {
            profile.secret_reference = TEST_SECRET_REFERENCE.into();
            Arc::new(FormSecret(secret.to_owned()))
        };
        let wake = self
            .host_wake
            .clone()
            .ok_or("SHINKI_EMU_HOST_WAKE_REQUIRED")?;
        self.connection_test = Some(connection_test::ConnectionTest::start(
            profile, secrets, wake,
        )?);
        self.translation_test_for = provider_id.into();
        self.translation_models.clear();
        self.translation_status = tr!("正在获取模型列表…").into();
        self.model()
    }

    pub(super) fn poll_connection_test(&mut self) -> bool {
        let result = self.connection_test.as_ref().and_then(|test| test.poll());
        let Some(result) = result else {
            return false;
        };
        self.connection_test.take();
        match result {
            Ok(result) => {
                self.translation_status = tr!(
                    "连接成功（{latency} ms，{models} 个可用模型）",
                    latency = result.latency_ms,
                    models = result.models.len()
                );
                self.translation_models = result.models;
            }
            Err(error) => {
                let reason = match &error {
                    connection_test::TestFailure::Provider(error) => connection_failure(error),
                    connection_test::TestFailure::Stopped => {
                        tr!("测试中断，请重试").into()
                    }
                };
                self.translation_status = tr!("连接失败：{error}", error = reason);
            }
        }
        true
    }

    pub(super) fn save_translation_provider(
        &mut self,
        provider_id: &str,
        name: &str,
        preset: &str,
        form: &TranslationFormViewModel,
        secret: &str,
    ) -> Result<ManagerViewModel, String> {
        self.save_translation_provider_with(
            &platform_secrets()?,
            provider_id,
            name,
            preset,
            form,
            secret,
        )?;
        self.show_snackbar(tr!("服务商已保存"), None);
        self.model()
    }

    /// Create (empty `provider_id`) or update a card. A new key is stored
    /// under a fresh reference before the cards are written; the replaced key
    /// is deleted afterwards. The first card is enabled automatically.
    pub(super) fn save_translation_provider_with(
        &mut self,
        secrets: &dyn ProviderSecrets,
        provider_id: &str,
        name: &str,
        preset: &str,
        form: &TranslationFormViewModel,
        secret: &str,
    ) -> Result<String, String> {
        self.require_no_session()?;
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 64 {
            return Err("SHINKI_EMU_TRANSLATION_PROVIDER_NAME".into());
        }
        self.connection_test.take();
        let mut book = self.provider_book()?;
        let mut profile = profile_from_form(form)?;
        let existing = book.providers.iter().position(|p| p.id == provider_id);
        let id = match existing {
            Some(index) => book.providers[index].id.clone(),
            None if provider_id.is_empty() => book.next_id(),
            None => return Err("SHINKI_EMU_TRANSLATION_PROVIDER_UNKNOWN".into()),
        };
        let previous = existing.map(|index| book.providers[index].profile.clone());
        profile.profile_id = previous
            .as_ref()
            .map_or_else(|| format!("translation.{id}"), |old| old.profile_id.clone());
        let replaced_secret = if secret.is_empty() {
            let old = previous
                .as_ref()
                .ok_or("SHINKI_EMU_TRANSLATION_SECRET_REQUIRED")?;
            secrets
                .resolve(&old.secret_reference)
                .map_err(|_| "SHINKI_EMU_TRANSLATION_SECRET_REQUIRED".to_owned())?;
            profile.secret_reference = old.secret_reference.clone();
            None
        } else {
            profile.secret_reference = next_secret_reference(
                &id,
                previous.as_ref().map(|old| old.secret_reference.as_str()),
            );
            profile.validate().map_err(|error| error.to_string())?;
            secrets.store(&profile.secret_reference, secret)?;
            previous.map(|old| old.secret_reference)
        };
        let stored = StoredProvider {
            id: id.clone(),
            name: name.into(),
            preset: preset.into(),
            profile,
        };
        match existing {
            Some(index) => book.providers[index] = stored.clone(),
            None => book.providers.push(stored.clone()),
        }
        if book.active().is_none() {
            book.active = Some(id.clone());
        }
        if let Err(error) = self.write_provider_book(&book) {
            if !secret.is_empty() {
                secrets
                    .delete(&stored.profile.secret_reference)
                    .map_err(|cleanup| format!("{error}; {cleanup}"))?;
            }
            return Err(error);
        }
        if let Some(old) = replaced_secret {
            secrets
                .delete(&old)
                .map_err(|_| "SHINKI_EMU_TRANSLATION_SAVED_OLD_SECRET_CLEANUP_FAILED")?;
        }
        self.translation_saved = self.translation_saved.wrapping_add(1);
        tracing::info!(event = "shinki.emu.manager.translation_provider_saved", provider = %id);
        Ok(id)
    }

    /// Make card `provider_id` the one game sessions use.
    pub(super) fn activate_translation_provider(
        &mut self,
        provider_id: &str,
    ) -> Result<ManagerViewModel, String> {
        self.require_no_session()?;
        let mut book = self.provider_book()?;
        let name = book
            .get(provider_id)
            .ok_or("SHINKI_EMU_TRANSLATION_PROVIDER_UNKNOWN")?
            .name
            .clone();
        book.active = Some(provider_id.into());
        self.write_provider_book(&book)?;
        tracing::info!(event = "shinki.emu.manager.translation_provider_activated", provider = %provider_id);
        self.show_snackbar(tr!("已切换到 {name}", name = name), None);
        self.model()
    }

    pub(super) fn duplicate_translation_provider(
        &mut self,
        provider_id: &str,
    ) -> Result<ManagerViewModel, String> {
        self.duplicate_translation_provider_with(&platform_secrets()?, provider_id)?;
        self.model()
    }

    /// Copy a card and its key; the copy starts disabled.
    pub(super) fn duplicate_translation_provider_with(
        &mut self,
        secrets: &dyn ProviderSecrets,
        provider_id: &str,
    ) -> Result<String, String> {
        self.require_no_session()?;
        let mut book = self.provider_book()?;
        let source = book
            .get(provider_id)
            .ok_or("SHINKI_EMU_TRANSLATION_PROVIDER_UNKNOWN")?
            .clone();
        let id = book.next_id();
        let key = secrets
            .resolve(&source.profile.secret_reference)
            .map_err(|_| "SHINKI_EMU_TRANSLATION_SECRET_REQUIRED".to_owned())?;
        let mut profile = source.profile;
        profile.profile_id = format!("translation.{id}");
        profile.secret_reference = next_secret_reference(&id, None);
        secrets.store(&profile.secret_reference, &key)?;
        let name: String = tr!("{name} 副本", name = source.name).chars().take(64).collect();
        book.providers.push(StoredProvider {
            id: id.clone(),
            name,
            preset: source.preset,
            profile,
        });
        if let Err(error) = self.write_provider_book(&book) {
            let reference = format!("translation.{id}.a");
            secrets
                .delete(&reference)
                .map_err(|cleanup| format!("{error}; {cleanup}"))?;
            return Err(error);
        }
        Ok(id)
    }

    pub(super) fn delete_translation_provider(
        &mut self,
        provider_id: &str,
    ) -> Result<ManagerViewModel, String> {
        self.delete_translation_provider_with(&platform_secrets()?, provider_id)?;
        self.show_snackbar(tr!("服务商已删除"), None);
        self.model()
    }

    /// Remove a disabled card and its key. The enabled card cannot be
    /// deleted; enable another one first.
    pub(super) fn delete_translation_provider_with(
        &mut self,
        secrets: &dyn ProviderSecrets,
        provider_id: &str,
    ) -> Result<(), String> {
        self.require_no_session()?;
        let mut book = self.provider_book()?;
        if book.active.as_deref() == Some(provider_id) {
            return Err("SHINKI_EMU_TRANSLATION_PROVIDER_ACTIVE".into());
        }
        let index = book
            .providers
            .iter()
            .position(|provider| provider.id == provider_id)
            .ok_or("SHINKI_EMU_TRANSLATION_PROVIDER_UNKNOWN")?;
        let removed = book.providers.remove(index);
        self.write_provider_book(&book)?;
        if secrets.resolve(&removed.profile.secret_reference).is_ok() {
            secrets.delete(&removed.profile.secret_reference)?;
        }
        Ok(())
    }

    pub(super) fn set_translation_enabled(
        &mut self,
        enabled: bool,
    ) -> Result<ManagerViewModel, String> {
        if enabled {
            self.require_network()?;
            self.profile()?
                .ok_or("SHINKI_EMU_TRANSLATION_PROFILE_REQUIRED")?;
        }
        self.library
            .set_app_state(TRANSLATION_ENABLED_KEY, &enabled)
            .map_err(|error| error.to_string())?;
        self.translation_enabled = enabled;
        self.model()
    }

    /// The live translation profile for a new session, or `None` when the
    /// network permission, the translation switch or an enabled card is missing.
    pub(super) fn launch_translation_profile(&self) -> Result<Option<TranslationProfile>, String> {
        if !self.network_allowed || !self.translation_enabled {
            return Ok(None);
        }
        self.profile()
    }
}

pub(super) fn profile_from_form(
    form: &TranslationFormViewModel,
) -> Result<TranslationProfile, String> {
    let provider = match form.provider.as_str() {
        "openai" => TranslationProvider::OpenAi,
        "anthropic" => TranslationProvider::Anthropic,
        "google" => TranslationProvider::Google,
        _ => return Err("SHINKI_EMU_TRANSLATION_PROVIDER_INVALID".into()),
    };
    let openai_api = match form.openai_api.as_str() {
        "responses" => OpenAiApi::Responses,
        "chat_completions" => OpenAiApi::ChatCompletions,
        _ => return Err("SHINKI_EMU_TRANSLATION_PROTOCOL_INVALID".into()),
    };
    let profile = TranslationProfile {
        profile_id: "translation.default".into(),
        provider,
        base_url: form.base_url.trim().trim_end_matches('/').into(),
        openai_api,
        model: form.model.trim().into(),
        target_language: form.language.trim().into(),
        timeout_ms: u64::try_from(form.timeout_s)
            .ok()
            .and_then(|seconds| seconds.checked_mul(1000))
            .ok_or("SHINKI_EMU_TRANSLATION_TIMEOUT_INVALID")?,
        secret_reference: "translation.default".into(),
        custom_prompt: form.custom_prompt.clone(),
        context_segments: u32::try_from(form.context_segments)
            .map_err(|_| "SHINKI_EMU_TRANSLATION_CONTEXT_INVALID")?,
        context_chars: u32::try_from(form.context_chars)
            .map_err(|_| "SHINKI_EMU_TRANSLATION_CONTEXT_INVALID")?,
    };
    profile.validate().map_err(|error| error.to_string())?;
    Ok(profile)
}

pub(super) fn profile_form(profile: Option<&TranslationProfile>) -> TranslationFormViewModel {
    let defaults = TranslationProfile::default();
    let profile = profile.unwrap_or(&defaults);
    TranslationFormViewModel {
        provider: profile.provider.as_str().into(),
        base_url: profile.base_url.clone(),
        openai_api: match profile.openai_api {
            OpenAiApi::Responses => "responses".into(),
            OpenAiApi::ChatCompletions => "chat_completions".into(),
        },
        model: profile.model.clone(),
        language: profile.target_language.clone(),
        timeout_s: i32::try_from(profile.timeout_ms / 1000).unwrap_or(i32::MAX),
        custom_prompt: profile.custom_prompt.clone(),
        context_segments: i32::try_from(profile.context_segments).unwrap_or(i32::MAX),
        context_chars: i32::try_from(profile.context_chars).unwrap_or(i32::MAX),
    }
}

#[cfg(test)]
#[path = "translation_tests.rs"]
mod tests;
