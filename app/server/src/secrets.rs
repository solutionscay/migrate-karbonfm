//! Secrets of AP-03: SMTP passwords and integration API keys live in the OS keychain and are read only in Rust.
//!
//! - [`Secret`] holds a value. It has no `Display`, `Serialize` or `ToString`, so askama cannot render it and serde
//!   cannot write it to SQLite or a response; its `Debug` prints `Secret(****)`, so a log line cannot show it.
//!   Only [`Secret::expose`] gives the text, for the SMTP or HTTP client.
//! - [`SecretStore`] is a store: [`Keychain`] in the app (macOS Keychain, Windows Credential Manager, Linux Secret
//!   Service), [`MemoryStore`] in tests and the dev server, [`NoKeychain`] when the computer has no keychain, and
//!   [`FileStore`] (`secrets.toml`, mode 0600) when the user chose it because there is no keychain (D-09, issue #7).
//! - [`Secrets`] is what the routes use: the file while `secrets.toml` exists (the user's choice), else the keychain.
//!   Without either, every call answers [`SecretError::Unavailable`]: the page disables the fields. There is no silent
//!   fallback.
//! - Pages get a [`SecretField`]: the name and a `has_secret` flag, never the value. The input is write-only
//!   (`templates/partials/secret_field.html`): an empty value means "keep the saved one".
//!
//! Keychain and file calls block (and the keychain may show an OS prompt): call them from
//! `tokio::task::spawn_blocking` in a handler.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Every secret the app keeps (D-09, 05 EXT-01..03). The account name is what the keychain shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SecretKey {
    /// The system email account of the settings (BR-ADMIN-05).
    SystemSmtpPassword,
    /// The user's own SMTP password (BR-INTEG-01).
    UserSmtpPassword,
    GooglePlacesApiKey,
    /// SmartyStreets signs a call with an auth ID and an auth token (117 s42-43, System #54, #55); this is the token.
    SmartyStreetsApiKey,
    /// The SmartyStreets auth ID, kept with its token (a pair of credentials, D-09).
    SmartyStreetsAuthId,
    FullContactApiKey,
}

impl SecretKey {
    pub const ALL: [SecretKey; 6] = [
        SecretKey::SystemSmtpPassword,
        SecretKey::UserSmtpPassword,
        SecretKey::GooglePlacesApiKey,
        SecretKey::SmartyStreetsApiKey,
        SecretKey::SmartyStreetsAuthId,
        SecretKey::FullContactApiKey,
    ];

    pub fn account(self) -> &'static str {
        match self {
            SecretKey::SystemSmtpPassword => "system_smtp_password",
            SecretKey::UserSmtpPassword => "user_smtp_password",
            SecretKey::GooglePlacesApiKey => "google_places",
            SecretKey::SmartyStreetsApiKey => "smartystreets",
            SecretKey::SmartyStreetsAuthId => "smartystreets_auth_id",
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

/// A store error. Its text names the key and the failure, never the value. The key comes last, in parentheses, so
/// the log mask (`applog::mask_text`, which hides the word after `<secret name>:`) keeps the reason readable.
#[derive(Debug)]
pub enum SecretError {
    /// No store can keep a secret: the computer has no keychain and the user has not chosen the file. The page shows
    /// this as disabled fields or a field error (422), never as a 500.
    Unavailable(String),
    /// The store refused or failed (a locked keychain, a file that cannot be read or written).
    Failed(String),
}

impl fmt::Display for SecretError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SecretError::Unavailable(why) => write!(f, "no secret store: {why}"),
            SecretError::Failed(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for SecretError {}

pub trait SecretStore: Send + Sync {
    fn get(&self, key: SecretKey) -> Result<Option<Secret>, SecretError>;
    fn set(&self, key: SecretKey, value: &Secret) -> Result<(), SecretError>;
    /// Removes the secret; removing a missing one is not an error.
    fn delete(&self, key: SecretKey) -> Result<(), SecretError>;

    /// False when this store cannot keep secrets at all ([`NoKeychain`]).
    fn available(&self) -> bool {
        true
    }

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

    /// The view of one secret for a template. With no store available, the field is disabled (not an error).
    fn field(&self, key: SecretKey, label: &'static str) -> Result<SecretField, SecretError> {
        let (has_secret, available) = match self.has_secret(key) {
            Ok(has) => (has, true),
            Err(SecretError::Unavailable(_)) => (false, false),
            Err(e) => return Err(e),
        };
        Ok(SecretField { name: key.account(), label, has_secret, available, error: None })
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The OS keychain
// ---------------------------------------------------------------------------------------------------------------

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

    /// Whether the platform keychain can be used, with the real cause when it cannot. keyring initialises its store
    /// once per process (on Linux: the session D-Bus, then a Secret Service session) and later calls only say
    /// `NoDefaultStore`, so this is the one place that sees why. It reads no entry.
    pub fn probe() -> Result<(), String> {
        keyring::Entry::store_status().as_ref().map(|_| ()).map_err(|e| e.to_string())
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

/// The store the desktop app uses: the keychain when [`Keychain::probe`] passes, else [`NoKeychain`]. The cause is
/// logged once, here. A keychain started while the app runs is seen at the next start (keyring keeps its probe).
pub fn system_store() -> Arc<dyn SecretStore> {
    match Keychain::probe() {
        Ok(()) => Arc::new(Keychain::new()),
        Err(why) => {
            let message = format!("no system keychain, so secrets need the file option (D-09): {why}");
            crate::applog::log_write(&crate::applog::Entry {
                level: crate::applog::Level::Warn,
                operation: "start",
                code: Some("Keychain"),
                message: &message,
                payload: None,
            });
            Arc::new(NoKeychain::new(why))
        }
    }
}

/// Maps a keyring error to a message without the secret (BadEncoding carries the stored bytes).
fn error(key: SecretKey, e: keyring::Error) -> SecretError {
    let account = key.account();
    match e {
        keyring::Error::NoDefaultStore => SecretError::Unavailable(format!("keychain: {e} (entry {account})")),
        keyring::Error::BadEncoding(_) => {
            SecretError::Failed(format!("keychain: the stored value is not UTF-8 (entry {account})"))
        }
        other => SecretError::Failed(format!("keychain: {other} (entry {account})")),
    }
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

/// The store of a computer without a keychain (Linux without a Secret Service): every call answers
/// [`SecretError::Unavailable`] with the cause from [`Keychain::probe`]. Also the test double for that case.
pub struct NoKeychain {
    why: String,
}

impl NoKeychain {
    pub fn new(why: impl Into<String>) -> Self {
        Self { why: why.into() }
    }

    fn refuse<T>(&self, key: SecretKey) -> Result<T, SecretError> {
        Err(SecretError::Unavailable(format!("no system keychain: {} (entry {})", self.why, key.account())))
    }
}

impl SecretStore for NoKeychain {
    fn get(&self, key: SecretKey) -> Result<Option<Secret>, SecretError> {
        self.refuse(key)
    }

    fn set(&self, key: SecretKey, _value: &Secret) -> Result<(), SecretError> {
        self.refuse(key)
    }

    fn delete(&self, key: SecretKey) -> Result<(), SecretError> {
        self.refuse(key)
    }

    fn available(&self) -> bool {
        false
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

// ---------------------------------------------------------------------------------------------------------------
// The file the user chose because there is no keychain (D-09, issue #7)
// ---------------------------------------------------------------------------------------------------------------

/// The file name in the app data folder. Backups copy only karbonfm.db and files/, so it is never in one, and a
/// restore leaves it alone.
pub const SECRETS_FILE: &str = "secrets.toml";

/// `<data folder>/secrets.toml`.
pub fn secrets_path(data_dir: &Path) -> PathBuf {
    data_dir.join(SECRETS_FILE)
}

const FILE_HEADER: &str = "# KarbonFM passwords and API keys, kept in this file because this computer has no system\n\
# keychain (you chose this in Settings). Only your user account may read or write it (mode 0600). It is not\n\
# encrypted and it is never put in a backup. Do not share it.\n";

/// One writer at a time for the read-modify-write of the file (the server is one process).
static FILE_LOCK: Mutex<()> = Mutex::new(());

/// Secrets in `secrets.toml`: one `account = "value"` line each.
///
/// - Unix: the file is created with mode 0600 (owner read/write only), and every open checks the mode and sets 0600
///   again if it is wider (a copy or an editor may have changed it). Writes go to a 0600 `.part` file that is then
///   renamed over the file, so a crash never leaves half a file.
/// - Windows: there are no mode bits. The file is in the per-user app data folder (`%APPDATA%\com.solutionscay.
///   karbonfm`), which only the user, SYSTEM and Administrators can open, and it inherits that folder's ACL. In
///   practice Windows always has Credential Manager, so the file option is not offered there.
pub struct FileStore {
    path: PathBuf,
}

impl FileStore {
    /// The user's opt-in: creates the file (no secrets yet) with mode 0600. An existing file is kept and checked.
    pub fn create(path: &Path) -> Result<Self, SecretError> {
        let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        if path.exists() {
            return Self::open(path);
        }
        let store = Self { path: path.to_owned() };
        store.write(&BTreeMap::new())?;
        Ok(store)
    }

    /// An existing file, made private (0600) if it is not.
    pub fn open(path: &Path) -> Result<Self, SecretError> {
        if !path.is_file() {
            return Err(SecretError::Failed(format!("secrets file {} is missing", path.display())));
        }
        make_private(path)?;
        Ok(Self { path: path.to_owned() })
    }

    fn fail(&self, what: &str, e: impl fmt::Display) -> SecretError {
        SecretError::Failed(format!("secrets file {}: {what}: {e}", self.path.display()))
    }

    fn read(&self) -> Result<BTreeMap<String, String>, SecretError> {
        make_private(&self.path)?;
        let text = fs::read_to_string(&self.path).map_err(|e| self.fail("could not read it", e))?;
        // Not the parser's message: it quotes the line, which may hold a secret.
        toml::from_str(&text)
            .map_err(|_| SecretError::Failed(format!("secrets file {} is not valid TOML", self.path.display())))
    }

    fn write(&self, secrets: &BTreeMap<String, String>) -> Result<(), SecretError> {
        let body = toml::to_string(secrets).map_err(|e| self.fail("could not encode it", e))?;
        let part = self.path.with_extension("toml.part");
        let _ = fs::remove_file(&part);
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&part).map_err(|e| self.fail("could not create it", e))?;
        // Again after create: the mode given to open is reduced by the umask.
        #[cfg(unix)]
        set_mode_600(&part).map_err(|e| self.fail("could not make it private", e))?;
        file.write_all(FILE_HEADER.as_bytes())
            .and_then(|()| file.write_all(body.as_bytes()))
            .and_then(|()| file.sync_all())
            .map_err(|e| self.fail("could not write it", e))?;
        drop(file);
        fs::rename(&part, &self.path).map_err(|e| self.fail("could not replace it", e))
    }

    /// The secrets in the file, for moving them to the keychain.
    pub fn all(&self) -> Result<Vec<(SecretKey, Secret)>, SecretError> {
        let map = self.read()?;
        Ok(SecretKey::ALL.iter().filter_map(|k| map.get(k.account()).map(|v| (*k, Secret::new(v.clone())))).collect())
    }
}

#[cfg(unix)]
fn set_mode_600(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

/// Unix: sets mode 0600 when the file has any other mode, and logs that it did. Windows: nothing (see [`FileStore`]).
fn make_private(path: &Path) -> Result<(), SecretError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let fail = |what: &str, e: std::io::Error| {
            SecretError::Failed(format!("secrets file {}: {what}: {e}", path.display()))
        };
        let mode = fs::metadata(path).map_err(|e| fail("could not read its permissions", e))?.permissions().mode();
        if mode & 0o777 != 0o600 {
            set_mode_600(path).map_err(|e| fail("could not make it private", e))?;
            let message = format!("secrets file {} had mode {:03o}; set to 600", path.display(), mode & 0o777);
            crate::applog::log_write(&crate::applog::Entry {
                level: crate::applog::Level::Warn,
                operation: "secrets",
                code: Some("Permissions"),
                message: &message,
                payload: None,
            });
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

impl SecretStore for FileStore {
    fn get(&self, key: SecretKey) -> Result<Option<Secret>, SecretError> {
        Ok(self.read()?.remove(key.account()).map(Secret))
    }

    fn set(&self, key: SecretKey, value: &Secret) -> Result<(), SecretError> {
        let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut map = self.read()?;
        map.insert(key.account().to_owned(), value.expose().to_owned());
        self.write(&map)
    }

    fn delete(&self, key: SecretKey) -> Result<(), SecretError> {
        let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut map = self.read()?;
        if map.remove(key.account()).is_some() {
            self.write(&map)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Which store the routes use
// ---------------------------------------------------------------------------------------------------------------

/// Where the secrets are kept now (the settings page says so).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreKind {
    Keychain,
    /// `secrets.toml`, chosen by the user because there was no keychain.
    File,
    /// No keychain and no file: secrets cannot be saved.
    Unavailable,
}

/// The routes' view of the secrets: the file while it exists (the user's choice is the file itself, so a backup or a
/// restore never changes it), else the keychain store the app started with.
#[derive(Clone)]
pub struct Secrets {
    keychain: Arc<dyn SecretStore>,
    /// `<data folder>/secrets.toml`; `None` without a data folder (then the file option is not offered).
    file: Option<PathBuf>,
}

impl Secrets {
    pub fn new(keychain: Arc<dyn SecretStore>, file: Option<PathBuf>) -> Self {
        Self { keychain, file }
    }

    pub fn kind(&self) -> StoreKind {
        if self.file.as_deref().is_some_and(Path::is_file) {
            StoreKind::File
        } else if self.keychain.available() {
            StoreKind::Keychain
        } else {
            StoreKind::Unavailable
        }
    }

    pub fn keychain_available(&self) -> bool {
        self.keychain.available()
    }

    /// The opt-in is offered only without a keychain, and only where there is a data folder for the file.
    pub fn file_offered(&self) -> bool {
        self.file.is_some() && !self.keychain.available()
    }

    pub fn file_path(&self) -> Option<&Path> {
        self.file.as_deref()
    }

    /// The opt-in: creates `secrets.toml` (0600). The caller checks [`Secrets::file_offered`] first.
    pub fn use_file(&self) -> Result<PathBuf, SecretError> {
        let path = self.file.clone().ok_or_else(|| SecretError::Unavailable("no data folder for the file".into()))?;
        FileStore::create(&path)?;
        Ok(path)
    }

    /// Back to the keychain: copies every secret of the file into the keychain, reads each one back, then removes the
    /// file. On any failure the file stays, and with it the file store. Returns how many secrets moved.
    pub fn use_keychain(&self) -> Result<usize, SecretError> {
        let path = self.file.as_deref().ok_or_else(|| SecretError::Unavailable("no data folder".into()))?;
        let file = FileStore::open(path)?;
        let all = file.all()?;
        for (key, value) in &all {
            self.keychain.set(*key, value)?;
        }
        for (key, value) in &all {
            let back = self.keychain.get(*key)?;
            if back.as_ref().map(Secret::expose) != Some(value.expose()) {
                return Err(SecretError::Failed(format!(
                    "keychain: the secret did not read back the same (entry {})",
                    key.account()
                )));
            }
        }
        fs::remove_file(path)
            .map_err(|e| SecretError::Failed(format!("secrets file {}: could not remove it: {e}", path.display())))?;
        Ok(all.len())
    }

    fn with<T>(&self, f: impl FnOnce(&dyn SecretStore) -> Result<T, SecretError>) -> Result<T, SecretError> {
        match (self.kind(), self.file.as_deref()) {
            (StoreKind::File, Some(path)) => f(&FileStore::open(path)?),
            _ => f(self.keychain.as_ref()),
        }
    }
}

impl SecretStore for Secrets {
    fn get(&self, key: SecretKey) -> Result<Option<Secret>, SecretError> {
        self.with(|s| s.get(key))
    }

    fn set(&self, key: SecretKey, value: &Secret) -> Result<(), SecretError> {
        self.with(|s| s.set(key, value))
    }

    fn delete(&self, key: SecretKey) -> Result<(), SecretError> {
        self.with(|s| s.delete(key))
    }

    fn available(&self) -> bool {
        self.kind() != StoreKind::Unavailable
    }
}

/// What a template gets for a secret: the field name, its label and whether a value is saved. `available` is false
/// when no store can keep it (the input is disabled); `error` is a field error of the last save.
#[derive(Clone, Debug, askama::Template)]
#[template(path = "partials/secret_field.html")]
pub struct SecretField {
    pub name: &'static str,
    pub label: &'static str,
    pub has_secret: bool,
    pub available: bool,
    pub error: Option<String>,
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

    #[test]
    fn no_keychain_is_unavailable_and_its_field_is_disabled() {
        let store = NoKeychain::new("no session bus");
        assert!(!store.available());
        assert!(matches!(store.get(SecretKey::SystemSmtpPassword), Err(SecretError::Unavailable(_))));
        assert!(matches!(store.update_from_form(SecretKey::SystemSmtpPassword, "x"), Err(SecretError::Unavailable(_))));
        store.update_from_form(SecretKey::SystemSmtpPassword, "").unwrap();
        let field = store.field(SecretKey::SystemSmtpPassword, "SMTP password").unwrap();
        assert!(!field.available && !field.has_secret);
    }

    /// The keychain error text keeps its reason through the log mask (it used to read `system_smtp_password: ***`).
    #[test]
    fn keychain_errors_survive_the_log_mask() {
        let unavailable = error(SecretKey::SystemSmtpPassword, keyring::Error::NoDefaultStore).to_string();
        let masked = crate::applog::mask_text(&unavailable);
        assert_eq!(masked, unavailable);
        assert!(masked.contains("No default store") && masked.contains("system_smtp_password"), "{masked}");
        let failed = error(SecretKey::UserSmtpPassword, keyring::Error::NoEntry).to_string();
        assert_eq!(crate::applog::mask_text(&failed), failed);
        let none = NoKeychain::new("org.freedesktop.secrets was not provided").get(SecretKey::SystemSmtpPassword);
        let text = none.err().unwrap().to_string();
        assert_eq!(crate::applog::mask_text(&text), text);
    }

    #[test]
    fn file_store_round_trip_keeps_other_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = secrets_path(dir.path());
        let store = FileStore::create(&path).unwrap();
        assert!(!store.has_secret(SecretKey::SystemSmtpPassword).unwrap());
        store.set(SecretKey::SystemSmtpPassword, &Secret::new("p\"a=ss\\word\nline")).unwrap();
        store.set(SecretKey::FullContactApiKey, &Secret::new("fc-1")).unwrap();
        let again = FileStore::open(&path).unwrap();
        assert_eq!(again.get(SecretKey::SystemSmtpPassword).unwrap().unwrap().expose(), "p\"a=ss\\word\nline");
        again.delete(SecretKey::SystemSmtpPassword).unwrap();
        again.delete(SecretKey::SystemSmtpPassword).unwrap();
        assert!(!again.has_secret(SecretKey::SystemSmtpPassword).unwrap());
        assert_eq!(again.get(SecretKey::FullContactApiKey).unwrap().unwrap().expose(), "fc-1");
        assert!(fs::read_to_string(&path).unwrap().starts_with("# KarbonFM passwords and API keys"));
        assert!(!path.with_extension("toml.part").exists());
    }

    #[test]
    fn a_broken_file_never_quotes_its_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = secrets_path(dir.path());
        fs::write(&path, "system_smtp_password = \"hunter2").unwrap();
        let err = FileStore::open(&path).unwrap().get(SecretKey::SystemSmtpPassword).err().unwrap().to_string();
        assert!(err.contains("not valid TOML") && !err.contains("hunter2"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn file_store_is_0600_and_open_fixes_a_wider_mode() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let dir = tempfile::tempdir().unwrap();
        let path = secrets_path(dir.path());
        let store = FileStore::create(&path).unwrap();
        assert_eq!(mode(&path), 0o600);
        store.set(SecretKey::GooglePlacesApiKey, &Secret::new("gp")).unwrap();
        assert_eq!(mode(&path), 0o600, "a write keeps 0600");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        FileStore::open(&path).unwrap();
        assert_eq!(mode(&path), 0o600, "open sets 0600 again");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();
        assert!(store.has_secret(SecretKey::GooglePlacesApiKey).unwrap());
        assert_eq!(mode(&path), 0o600, "every read checks it");
    }

    #[test]
    fn secrets_prefer_the_file_and_move_back_to_the_keychain() {
        let dir = tempfile::tempdir().unwrap();
        let path = secrets_path(dir.path());
        let none = Secrets::new(Arc::new(NoKeychain::new("test")), Some(path.clone()));
        assert_eq!(none.kind(), StoreKind::Unavailable);
        assert!(none.file_offered());
        assert!(matches!(none.set(SecretKey::SystemSmtpPassword, &Secret::new("x")), Err(SecretError::Unavailable(_))));
        none.use_file().unwrap();
        assert_eq!(none.kind(), StoreKind::File);
        none.set(SecretKey::SystemSmtpPassword, &Secret::new("pw-1")).unwrap();
        assert!(matches!(none.use_keychain(), Err(SecretError::Unavailable(_))), "no keychain to move to");
        assert!(path.exists(), "a failed move keeps the file");

        // The next start, with a keychain running: the file still wins until the user moves the secrets.
        let keychain = Arc::new(MemoryStore::default());
        let later = Secrets::new(keychain.clone(), Some(path.clone()));
        assert_eq!(later.kind(), StoreKind::File);
        assert!(!later.file_offered());
        assert_eq!(later.use_keychain().unwrap(), 1);
        assert!(!path.exists());
        assert_eq!(later.kind(), StoreKind::Keychain);
        assert_eq!(keychain.get(SecretKey::SystemSmtpPassword).unwrap().unwrap().expose(), "pw-1");
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
