//! Secrets of AP-03: SMTP passwords and integration API keys live in the OS keychain and are read only in Rust.
//!
//! - [`Secret`] holds a value. It has no `Display`, `Serialize` or `ToString`, so askama cannot render it and serde
//!   cannot write it to SQLite or a response; its `Debug` prints `Secret(****)`, so a log line cannot show it.
//!   Only [`Secret::expose`] gives the text, for the SMTP or HTTP client.
//! - [`SecretStore`] is the store: [`Keychain`] in the app (macOS Keychain, Windows Credential Manager, Linux Secret
//!   Service), [`MemoryStore`] in tests and the dev server.
//! - Pages get a [`SecretField`]: the name and a `has_secret` flag, never the value. The input is write-only
//!   (`templates/partials/secret_field.html`): an empty value means "keep the saved one".
//!
//! Keychain calls block (and may show an OS prompt): call them from `tokio::task::spawn_blocking` in a handler.

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

/// Every secret the app keeps (D-09, 05 EXT-01..03). The account name is what the keychain shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SecretKey {
    /// The system email account of the settings (BR-ADMIN-05).
    SystemSmtpPassword,
    /// The user's own SMTP password (BR-INTEG-01).
    UserSmtpPassword,
    GooglePlacesApiKey,
    SmartyStreetsApiKey,
    FullContactApiKey,
}

impl SecretKey {
    pub const ALL: [SecretKey; 5] = [
        SecretKey::SystemSmtpPassword,
        SecretKey::UserSmtpPassword,
        SecretKey::GooglePlacesApiKey,
        SecretKey::SmartyStreetsApiKey,
        SecretKey::FullContactApiKey,
    ];

    pub fn account(self) -> &'static str {
        match self {
            SecretKey::SystemSmtpPassword => "system_smtp_password",
            SecretKey::UserSmtpPassword => "user_smtp_password",
            SecretKey::GooglePlacesApiKey => "google_places",
            SecretKey::SmartyStreetsApiKey => "smartystreets",
            SecretKey::FullContactApiKey => "fullcontact",
        }
    }
}

/// A secret value. Deliberately not `Display`, `Serialize` or `PartialEq`.
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The text, for the one client call that needs it. Do not log, render or store the result.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(****)")
    }
}

/// A store error. Its text names the key and the failure, never the value.
#[derive(Debug)]
pub struct SecretError(String);

impl fmt::Display for SecretError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "keychain: {}", self.0)
    }
}

impl std::error::Error for SecretError {}

pub trait SecretStore: Send + Sync {
    fn get(&self, key: SecretKey) -> Result<Option<Secret>, SecretError>;
    fn set(&self, key: SecretKey, value: &Secret) -> Result<(), SecretError>;
    /// Removes the secret; removing a missing one is not an error.
    fn delete(&self, key: SecretKey) -> Result<(), SecretError>;

    /// The flag the settings page shows instead of the value.
    fn has_secret(&self, key: SecretKey) -> Result<bool, SecretError> {
        Ok(self.get(key)?.is_some())
    }

    /// Applies a write-only form field: an empty field keeps the saved secret, a value replaces it.
    fn update_from_form(&self, key: SecretKey, field: &str) -> Result<(), SecretError> {
        if field.is_empty() {
            Ok(())
        } else {
            self.set(key, &Secret::new(field))
        }
    }

    /// The view of one secret for a template.
    fn field(&self, key: SecretKey, label: &'static str) -> Result<SecretField, SecretError> {
        Ok(SecretField { name: key.account(), label, has_secret: self.has_secret(key)? })
    }
}

/// The OS keychain, one entry per [`SecretKey`] under the app's identifier.
pub struct Keychain {
    service: String,
}

impl Keychain {
    /// The service name is the Tauri bundle identifier (`desktop/tauri.conf.json`).
    pub const SERVICE: &'static str = "com.solutionscay.karbonfm";

    pub fn new() -> Self {
        Self::with_service(Self::SERVICE)
    }

    /// Another service name, for the keychain smoke test (it must not touch the app's entries).
    pub fn with_service(service: impl Into<String>) -> Self {
        Self { service: service.into() }
    }

    fn entry(&self, key: SecretKey) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(&self.service, key.account()).map_err(|e| error(key, e))
    }
}

impl Default for Keychain {
    fn default() -> Self {
        Self::new()
    }
}

/// Maps a keyring error to a message without the secret (BadEncoding carries the stored bytes).
fn error(key: SecretKey, e: keyring::Error) -> SecretError {
    let why = match e {
        keyring::Error::BadEncoding(_) => "the stored value is not UTF-8".to_owned(),
        other => other.to_string(),
    };
    SecretError(format!("{}: {why}", key.account()))
}

impl SecretStore for Keychain {
    fn get(&self, key: SecretKey) -> Result<Option<Secret>, SecretError> {
        match self.entry(key)?.get_password() {
            Ok(v) => Ok(Some(Secret(v))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(error(key, e)),
        }
    }

    fn set(&self, key: SecretKey, value: &Secret) -> Result<(), SecretError> {
        self.entry(key)?.set_password(value.expose()).map_err(|e| error(key, e))
    }

    fn delete(&self, key: SecretKey) -> Result<(), SecretError> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(error(key, e)),
        }
    }
}

/// The test double: secrets in memory, gone when the process ends. Also used by the dev server.
#[derive(Default)]
pub struct MemoryStore(Mutex<HashMap<SecretKey, String>>);

impl SecretStore for MemoryStore {
    fn get(&self, key: SecretKey) -> Result<Option<Secret>, SecretError> {
        Ok(self.0.lock().unwrap().get(&key).cloned().map(Secret))
    }

    fn set(&self, key: SecretKey, value: &Secret) -> Result<(), SecretError> {
        self.0.lock().unwrap().insert(key, value.expose().to_owned());
        Ok(())
    }

    fn delete(&self, key: SecretKey) -> Result<(), SecretError> {
        self.0.lock().unwrap().remove(&key);
        Ok(())
    }
}

/// What a template gets for a secret: the field name, its label and whether a value is saved.
#[derive(Clone, Debug, askama::Template)]
#[template(path = "partials/secret_field.html")]
pub struct SecretField {
    pub name: &'static str,
    pub label: &'static str,
    pub has_secret: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_shows_the_value() {
        let s = Secret::new("hunter2");
        assert_eq!(format!("{s:?}"), "Secret(****)");
        assert_eq!(format!("{:?}", Some(s)), "Some(Secret(****))");
    }

    #[test]
    fn memory_store_round_trip_and_flags() {
        let store = MemoryStore::default();
        let key = SecretKey::UserSmtpPassword;
        assert!(!store.has_secret(key).unwrap());
        store.update_from_form(key, "").unwrap();
        assert!(!store.has_secret(key).unwrap(), "an empty field saves nothing");
        store.update_from_form(key, "hunter2").unwrap();
        assert!(store.has_secret(key).unwrap());
        assert_eq!(store.get(key).unwrap().unwrap().expose(), "hunter2");
        store.update_from_form(key, "").unwrap();
        assert_eq!(store.get(key).unwrap().unwrap().expose(), "hunter2", "an empty field keeps the secret");
        store.delete(key).unwrap();
        store.delete(key).unwrap();
        assert!(!store.has_secret(key).unwrap());
    }

    #[test]
    fn account_names_are_distinct() {
        let mut names: Vec<_> = SecretKey::ALL.iter().map(|k| k.account()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), SecretKey::ALL.len());
    }

    /// Writes, reads and deletes one entry in the real OS keychain under a test service name.
    /// Run by hand: `cargo test -p karbon-server keychain_round_trip -- --ignored`.
    #[test]
    #[ignore = "touches the OS keychain"]
    fn keychain_round_trip() {
        let kc = Keychain::with_service("com.solutionscay.karbonfm.test");
        let key = SecretKey::FullContactApiKey;
        kc.delete(key).unwrap();
        assert!(!kc.has_secret(key).unwrap());
        kc.set(key, &Secret::new("test-value-123")).unwrap();
        assert_eq!(kc.get(key).unwrap().unwrap().expose(), "test-value-123");
        kc.delete(key).unwrap();
        assert!(!kc.has_secret(key).unwrap());
    }
}
