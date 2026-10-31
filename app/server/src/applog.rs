//! The local log file (OP-ADMIN-05 `log_write`, BR-ADMIN-03, D-30). One JSON object per line in
//! `<app log folder>/karbonfm.log`, rotated at 5 MB to `karbonfm.1.log` .. `karbonfm.9.log` (10 files at most).
//! Secrets are masked before a line is written (AP-03): payload fields whose name says secret, and secret-looking
//! text in the message. There is no log table in SQLite (04: the karbon_log Log table has no target table, D-30).
//!
//! The desktop shell calls [`init`] with its log folder at start; before that, and in the dev server and tests,
//! lines go to stderr.

use regex::Regex;
use serde_json::{json, Map, Value};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Rotation (D-30): a file is rotated before it would pass `MAX_BYTES`; `MAX_FILES` files are kept.
pub const MAX_BYTES: u64 = 5 * 1024 * 1024;
pub const MAX_FILES: usize = 10;
pub const FILE_NAME: &str = "karbonfm.log";

/// What replaces a masked value.
pub const MASK: &str = "***";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Error,
    Warn,
    Info,
}

impl Level {
    fn as_str(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warn => "warn",
            Level::Info => "info",
        }
    }
}

/// One log line: the operation (command or route), an error code (05 CommandError codes, D-25), a message and the
/// form payload of the operation.
#[derive(Debug, Clone)]
pub struct Entry<'a> {
    pub level: Level,
    pub operation: &'a str,
    pub code: Option<&'a str>,
    pub message: &'a str,
    pub payload: Option<&'a Value>,
}

impl<'a> Entry<'a> {
    pub fn error(operation: &'a str, code: &'a str, message: &'a str) -> Self {
        Self { level: Level::Error, operation, code: Some(code), message, payload: None }
    }

    pub fn info(operation: &'a str, message: &'a str) -> Self {
        Self { level: Level::Info, operation, code: None, message, payload: None }
    }

    pub fn with_payload(mut self, payload: &'a Value) -> Self {
        self.payload = Some(payload);
        self
    }
}

/// A log file with rotation. The app has one, behind [`init`]; tests make their own.
pub struct Logger {
    dir: PathBuf,
    max_bytes: u64,
    file: Mutex<Option<File>>,
}

impl Logger {
    pub fn new(dir: impl Into<PathBuf>) -> std::io::Result<Self> {
        Self::with_max_bytes(dir, MAX_BYTES)
    }

    pub fn with_max_bytes(dir: impl Into<PathBuf>, max_bytes: u64) -> std::io::Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        Ok(Self { dir, max_bytes, file: Mutex::new(None) })
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join(FILE_NAME)
    }

    /// Writes one masked line. A failure to write the log never fails the operation that logs.
    pub fn write(&self, entry: &Entry) {
        let line = format_line(entry);
        if let Err(e) = self.append(&line) {
            eprintln!("log file {}: {e}; {line}", self.path().display());
        }
    }

    fn append(&self, line: &str) -> std::io::Result<()> {
        let mut file = self.file.lock().unwrap_or_else(|e| e.into_inner());
        let len = fs::metadata(self.path()).map(|m| m.len()).unwrap_or(0);
        if len > 0 && len + line.len() as u64 + 1 > self.max_bytes {
            *file = None;
            rotate(&self.dir)?;
        }
        if file.is_none() {
            *file = Some(OpenOptions::new().create(true).append(true).open(self.path())?);
        }
        let f = file.as_mut().expect("opened above");
        writeln!(f, "{line}")?;
        f.flush()
    }
}

/// karbonfm.8.log -> karbonfm.9.log, ..., karbonfm.log -> karbonfm.1.log; the oldest is dropped.
fn rotate(dir: &Path) -> std::io::Result<()> {
    let name = |n: usize| if n == 0 { dir.join(FILE_NAME) } else { dir.join(format!("karbonfm.{n}.log")) };
    let oldest = name(MAX_FILES - 1);
    if oldest.exists() {
        fs::remove_file(&oldest)?;
    }
    for n in (0..MAX_FILES - 1).rev() {
        if name(n).exists() {
            fs::rename(name(n), name(n + 1))?;
        }
    }
    Ok(())
}

static LOGGER: OnceLock<Logger> = OnceLock::new();

/// Sends the app's log lines to `<dir>/karbonfm.log` from now on. Called once by the desktop shell.
pub fn init(dir: impl Into<PathBuf>) -> std::io::Result<&'static Path> {
    let logger = Logger::new(dir)?;
    let logger = LOGGER.get_or_init(|| logger);
    Ok(logger.dir.as_path())
}

/// OP-ADMIN-05 `log_write`: writes one masked line to the app log (stderr before [`init`]).
pub fn log_write(entry: &Entry) {
    match LOGGER.get() {
        Some(logger) => logger.write(entry),
        None => eprintln!("{}", format_line(entry)),
    }
}

/// The JSON line of an entry, masked. Time is local civil time (D-05).
pub fn format_line(entry: &Entry) -> String {
    let mut line = Map::new();
    line.insert("ts".into(), json!(crate::audit::local_now()));
    line.insert("level".into(), json!(entry.level.as_str()));
    line.insert("op".into(), json!(mask_text(entry.operation)));
    if let Some(code) = entry.code {
        line.insert("code".into(), json!(code));
    }
    line.insert("message".into(), json!(mask_text(entry.message)));
    if let Some(payload) = entry.payload {
        line.insert("payload".into(), mask_value(payload));
    }
    Value::Object(line).to_string()
}

/// Field names whose value is a secret (AP-03): passwords, keys, tokens, cookies, credentials. Compared without case,
/// `_` or `-`, so `smtp_password`, `apiKey`, `X-Api-Key` and `session_token` all match.
const SECRET_NAMES: &[&str] =
    &["password", "passwd", "pwd", "secret", "token", "apikey", "authorization", "cookie", "credential", "privatekey"];

pub fn is_secret_name(name: &str) -> bool {
    let n: String = name.chars().filter(|c| *c != '_' && *c != '-').flat_map(char::to_lowercase).collect();
    SECRET_NAMES.iter().any(|s| n.contains(s))
}

/// Masks every value under a secret field name, at any depth, and secret-looking text in every string.
pub fn mask_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| {
                    let v = if is_secret_name(k) && !v.is_null() { json!(MASK) } else { mask_value(v) };
                    (k.clone(), v)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(mask_value).collect()),
        Value::String(s) => Value::String(mask_text(s)),
        other => other.clone(),
    }
}

/// Masks secrets inside free text: `Bearer <token>`, `Basic <credentials>`, `name=value` and `name: value` pairs whose
/// name is a secret name, and the password of a URL (`smtp://user:pass@host`).
pub fn mask_text(text: &str) -> String {
    static PATTERNS: OnceLock<[Regex; 3]> = OnceLock::new();
    let [auth, pair, url] = PATTERNS.get_or_init(|| {
        [
            Regex::new(r"(?i)\b(bearer|basic)\s+[A-Za-z0-9._~+/=-]+").unwrap(),
            Regex::new(
                r#"(?i)\b([A-Za-z0-9_-]*(?:password|passwd|pwd|secret|token|api[_-]?key|authorization|cookie|credential|private[_-]?key)[A-Za-z0-9_-]*)("?\s*[=:]\s*"?)([^\s"&,;]+)"#,
            )
            .unwrap(),
            Regex::new(r"([A-Za-z][A-Za-z0-9+.-]*://[^\s:/@]+:)[^\s@/]+@").unwrap(),
        ]
    });
    let text = auth.replace_all(text, format!("$1 {MASK}"));
    let text = pair.replace_all(&text, format!("${{1}}${{2}}{MASK}"));
    let text = url.replace_all(&text, format!("${{1}}{MASK}@"));
    text.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_names() {
        for name in
            ["password", "smtp_password", "apiKey", "X-Api-Key", "session_token", "Authorization", "client_secret"]
        {
            assert!(is_secret_name(name), "{name}");
        }
        for name in ["name", "email", "account_name", "smtp_server", "smtp_user_name", "tax_rate_bp"] {
            assert!(!is_secret_name(name), "{name}");
        }
    }

    #[test]
    fn masks_payload_fields_at_any_depth() {
        let payload = json!({
            "smtp_server": "smtp.example.com",
            "smtp_password": "hunter2",
            "nested": { "api_key": "AKIA123", "list": [{ "token": "t0k" }, "Bearer abc.def"] },
            "empty_secret": null,
            "amount": 1200
        });
        let masked = mask_value(&payload);
        assert_eq!(masked["smtp_server"], "smtp.example.com");
        assert_eq!(masked["smtp_password"], MASK);
        assert_eq!(masked["nested"]["api_key"], MASK);
        assert_eq!(masked["nested"]["list"][0]["token"], MASK);
        assert_eq!(masked["nested"]["list"][1], "Bearer ***");
        assert_eq!(masked["empty_secret"], Value::Null);
        assert_eq!(masked["amount"], 1200);
        let text = masked.to_string();
        for secret in ["hunter2", "AKIA123", "t0k", "abc.def"] {
            assert!(!text.contains(secret), "{secret} in {text}");
        }
    }

    #[test]
    fn masks_secrets_in_text() {
        let cases = [
            ("SMTP login failed: password=hunter2 for mara", "password=***"),
            ("header Authorization: Bearer eyJhbGciOi.x-y_z", "Authorization: ***"),
            ("retry with Bearer eyJhbGciOi.x-y_z", "Bearer ***"),
            ("POST failed {\"api_key\": \"k-123\"}", "\"api_key\": \"***"),
            ("connect smtp://mara:hunter2@smtp.example.com:587", "smtp://mara:***@smtp.example.com"),
            ("Cookie: karbon_session=abc123", "Cookie: ***"),
            ("session_token=abc123; path=/", "session_token=***"),
        ];
        for (input, expected) in cases {
            let out = mask_text(input);
            assert!(out.contains(expected), "{input} -> {out}");
            for secret in ["hunter2", "k-123", "abc123", "eyJhbGciOi"] {
                assert!(!out.contains(secret), "{out}");
            }
        }
        assert_eq!(mask_text("database: no such table: party"), "database: no such table: party");
    }

    #[test]
    fn line_is_json_with_local_time_and_masked_payload() {
        let payload = json!({ "name": "Ellison Studio", "password": "hunter2" });
        let entry = Entry::error("POST /settings", "Validation", "token=abc").with_payload(&payload);
        let line: Value = serde_json::from_str(&format_line(&entry)).unwrap();
        assert!(crate::audit::is_timestamp(line["ts"].as_str().unwrap()));
        assert_eq!(line["level"], "error");
        assert_eq!(line["op"], "POST /settings");
        assert_eq!(line["code"], "Validation");
        assert_eq!(line["message"], "token=***");
        assert_eq!(line["payload"]["name"], "Ellison Studio");
        assert_eq!(line["payload"]["password"], MASK);
    }

    #[test]
    fn writes_and_rotates_ten_files() {
        let dir = tempfile::tempdir().unwrap();
        let logger = Logger::with_max_bytes(dir.path(), 400).unwrap();
        for i in 0..200 {
            logger.write(&Entry::info("test", &format!("line {i} password=hunter2")));
        }
        let mut files: Vec<String> =
            fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        files.sort();
        assert_eq!(files.len(), MAX_FILES, "{files:?}");
        assert!(files.contains(&"karbonfm.log".into()) && files.contains(&"karbonfm.9.log".into()));
        for f in &files {
            let text = fs::read_to_string(dir.path().join(f)).unwrap();
            assert!(fs::metadata(dir.path().join(f)).unwrap().len() <= 400);
            assert!(!text.contains("hunter2"));
        }
        let text = fs::read_to_string(logger.path()).unwrap();
        let last: Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
        assert_eq!(last["message"], "line 199 password=***");
    }
}
