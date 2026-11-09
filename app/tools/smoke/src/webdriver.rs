//! The few W3C WebDriver calls the smoke test needs, sent to tauri-driver on 127.0.0.1 (plain HTTP and JSON).
//! tauri-driver passes them to the platform's driver: WebKitWebDriver on Linux, msedgedriver on Windows.

use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// A screenshot of a 1440 x 900 window is a few MB of base64; leave room for a HiDPI one.
const MAX_BODY: u64 = 64 * 1024 * 1024;

pub struct Driver {
    agent: ureq::Agent,
    base: String,
}

pub struct Session<'a> {
    driver: &'a Driver,
    id: String,
}

impl Driver {
    pub fn new(port: u16) -> Self {
        let agent = ureq::Agent::config_builder()
            // WebDriver errors come back as 4xx/5xx with a JSON body that says what went wrong: read it.
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(180)))
            .build()
            .into();
        Driver { agent, base: format!("http://127.0.0.1:{port}") }
    }

    /// One call; the response's `value`, or the driver's error and message.
    fn call(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
        let url = format!("{}{path}", self.base);
        let response = match method {
            "GET" => self.agent.get(&url).call(),
            "DELETE" => self.agent.delete(&url).call(),
            _ => self.agent.post(&url).send_json(body.unwrap_or_else(|| json!({}))),
        }
        .map_err(|e| format!("{method} {path}: {e}"))?;
        let status = response.status();
        let reply: Value = response
            .into_body()
            .with_config()
            .limit(MAX_BODY)
            .read_json()
            .map_err(|e| format!("{method} {path}: HTTP {status}, unreadable reply: {e}"))?;
        let value = reply.get("value").cloned().unwrap_or(Value::Null);
        match value.get("error").and_then(Value::as_str) {
            Some(error) => {
                let message = value.get("message").and_then(Value::as_str).unwrap_or("");
                Err(format!("{method} {path}: HTTP {status}: {error}: {message}"))
            }
            None if !status.is_success() => Err(format!("{method} {path}: HTTP {status}: {reply}")),
            None => Ok(value),
        }
    }

    /// Waits until the driver answers `/status` with ready.
    pub fn wait_ready(&self, timeout: Duration) -> Result<(), String> {
        let start = Instant::now();
        let mut last = String::new();
        while start.elapsed() < timeout {
            match self.call("GET", "/status", None) {
                Ok(v) if v.get("ready").and_then(Value::as_bool) != Some(false) => return Ok(()),
                Ok(v) => last = v.to_string(),
                Err(e) => last = e,
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        Err(format!("tauri-driver was not ready after {timeout:?}: {last}"))
    }

    /// Starts the app under the driver (tauri-driver's `tauri:options.application`).
    pub fn new_session(&self, application: &str) -> Result<Session<'_>, String> {
        let caps = json!({
            "capabilities": {
                "alwaysMatch": { "browserName": "wry", "tauri:options": { "application": application } }
            }
        });
        let value = self.call("POST", "/session", Some(caps))?;
        let id = value.get("sessionId").and_then(Value::as_str).ok_or_else(|| format!("no sessionId in {value}"))?;
        Ok(Session { driver: self, id: id.to_owned() })
    }
}

impl Session<'_> {
    fn call(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
        self.driver.call(method, &format!("/session/{}{path}", self.id), body)
    }

    fn string(&self, path: &str) -> Result<String, String> {
        let v = self.call("GET", path, None)?;
        v.as_str().map(str::to_owned).ok_or_else(|| format!("GET {path}: not a string: {v}"))
    }

    pub fn title(&self) -> Result<String, String> {
        self.string("/title")
    }

    pub fn url(&self) -> Result<String, String> {
        self.string("/url")
    }

    pub fn navigate(&self, url: &str) -> Result<(), String> {
        self.call("POST", "/url", Some(json!({ "url": url }))).map(drop)
    }

    /// The page's text as the user reads it (`innerText`: no script, style or hidden element).
    pub fn visible_text(&self) -> Result<String, String> {
        let v = self.call(
            "POST",
            "/execute/sync",
            Some(json!({ "script": "return document.body.innerText;", "args": [] })),
        )?;
        v.as_str().map(str::to_owned).ok_or_else(|| format!("innerText: not a string: {v}"))
    }

    /// A PNG of the window's page.
    pub fn screenshot(&self) -> Result<Vec<u8>, String> {
        use base64::Engine as _;
        let b64 = self.string("/screenshot")?;
        base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|e| format!("screenshot: {e}"))
    }

    /// Polls the page title until it is `want`; the last title seen when it never is.
    pub fn wait_title(&self, want: &str, timeout: Duration) -> Result<(), String> {
        let start = Instant::now();
        let mut last = String::new();
        while start.elapsed() < timeout {
            match self.title() {
                Ok(t) if t == want => return Ok(()),
                Ok(t) => last = format!("title {t:?}"),
                Err(e) => last = e,
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        Err(format!("the title was not {want:?} after {timeout:?} (last: {last})"))
    }

    /// Ends the session, which closes the app.
    pub fn close(self) -> Result<(), String> {
        self.call("DELETE", "", None).map(drop)
    }
}
