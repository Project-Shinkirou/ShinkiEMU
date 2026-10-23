#[cfg(not(target_os = "android"))]
use shinki_emu_translation::PlatformSecretStore;
use shinki_emu_translation::{SecretResolver, TranslationError};

#[cfg(not(target_os = "android"))]
#[derive(Clone)]
pub struct ManagerSecretStore {
    inner: PlatformSecretStore,
}

#[cfg(not(target_os = "android"))]
impl ManagerSecretStore {
    pub fn open() -> Result<Self, TranslationError> {
        PlatformSecretStore::new("dev.shinkiengine.ShinkiEMU").map(|inner| Self { inner })
    }

    pub fn store(&self, reference: &str, secret: &str) -> Result<(), TranslationError> {
        self.inner.store(reference, secret)
    }

    pub fn delete(&self, reference: &str) -> Result<(), TranslationError> {
        self.inner.delete(reference)
    }
}

#[cfg(not(target_os = "android"))]
impl SecretResolver for ManagerSecretStore {
    fn resolve(&self, reference: &str) -> Result<String, TranslationError> {
        self.inner.resolve(reference)
    }
}

/// Secrets live in app-private SharedPreferences, encrypted with a per-reference
/// AndroidKeyStore AES-GCM key through `ShinkiPlatformBridge`.
#[cfg(target_os = "android")]
#[derive(Clone)]
pub struct ManagerSecretStore;

#[cfg(target_os = "android")]
impl ManagerSecretStore {
    pub fn open() -> Result<Self, TranslationError> {
        Ok(Self)
    }

    pub fn store(&self, reference: &str, secret: &str) -> Result<(), TranslationError> {
        crate::platform::android::store_secret(reference, secret).map_err(secret_unavailable)
    }

    pub fn delete(&self, reference: &str) -> Result<(), TranslationError> {
        crate::platform::android::delete_secret(reference).map_err(secret_unavailable)
    }
}

#[cfg(target_os = "android")]
impl SecretResolver for ManagerSecretStore {
    fn resolve(&self, reference: &str) -> Result<String, TranslationError> {
        crate::platform::android::resolve_secret(reference).map_err(secret_unavailable)
    }
}

/// The translation error type carries no detail; keep the bridge diagnostic in
/// the log, as the desktop keyring backend maps every failure the same way.
#[cfg(target_os = "android")]
fn secret_unavailable(code: String) -> TranslationError {
    tracing::warn!(event = "shinki.emu.android.secret_store_failed", diagnostic_code = %code);
    TranslationError::SecretUnavailable
}
