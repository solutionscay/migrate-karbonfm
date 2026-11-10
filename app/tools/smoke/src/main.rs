//! `karbon-smoke --app <karbonfm binary> --out <folder>`: the WebDriver smoke test of the desktop window
//! (03 Test strategy, R-03: "WebDriver smoke tests on Windows and Linux through tauri-driver; macOS manual checklist").
//!
//! It starts tauri-driver, which starts the app on a new data folder (KARBONFM_DATA_DIR, a temp folder, so the app
//! creates the big demo there and never opens the user's database). Then, in the app's own window:
//! - the dashboard: its title, the greeting, the DEMO badge and two figures of the demo seed;
//! - invoice INV-1048: its title, customer, total and balance due.
//!
//! Each page gets a screenshot in `--out` (`dashboard.png`, `invoice-INV-1048.png`), also when a check fails, with
//! `summary.txt`, the app's `karbonfm.log` and `tauri-driver.log`. The exit code is 0 only when every check passed.
//!
//! tauri-driver runs on Windows and Linux only (no macOS WebDriver for WKWebView): on macOS the manual checklist covers
//! the window. Linux needs WebKitWebDriver (`webkit2gtk-driver`) and a display (Xvfb in CI); Windows needs an
//! msedgedriver that matches the WebView2 runtime (`--native-driver`).
//!
//! The figures are the big demo's (karbon-synth seed 20261101). The demo's dates move to the local date at creation,
//! but the amounts do not, so these stay the same on every day the test runs.

mod webdriver;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use webdriver::{Driver, Session};

/// One page to open and check.
struct Page {
    path: &'static str,
    title: &'static str,
    expect: &'static [&'static str],
    screenshot: &'static str,
}

const PAGES: [Page; 2] = [
    Page {
        path: "/",
        title: "Dashboard · KarbonFM",
        expect: &["Good day, Mara", "DEMO", "Open pipeline", "$705,400.00", "Balance due", "$393,521.01"],
        screenshot: "dashboard.png",
    },
    Page {
        path: "/sales/INV-1048",
        title: "INV-1048 · KarbonFM",
        expect: &["INV-1048", "Halden & Pryce Architects", "Net 15", "$3,557.15", "Balance due", "$2,057.15"],
        screenshot: "invoice-INV-1048.png",
    },
];

const USAGE: &str = "usage: karbon-smoke --app <karbonfm binary> --out <folder> [--driver tauri-driver] \
                     [--native-driver <path>] [--port 4444] [--data-dir <new folder>] [--keep-data]";

#[derive(Debug)]
struct Args {
    app: PathBuf,
    out: PathBuf,
    driver: PathBuf,
    native_driver: Option<PathBuf>,
    port: u16,
    data_dir: Option<PathBuf>,
    keep_data: bool,
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Args, String> {
    let (mut app, mut out, mut native_driver, mut data_dir) = (None, None, None, None);
    let (mut driver, mut port, mut keep_data) = (PathBuf::from("tauri-driver"), 4444u16, false);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{arg} needs a value\n{USAGE}"));
        match arg.as_str() {
            "--app" => app = Some(PathBuf::from(value()?)),
            "--out" => out = Some(PathBuf::from(value()?)),
            "--driver" => driver = PathBuf::from(value()?),
            "--native-driver" => native_driver = Some(PathBuf::from(value()?)),
            "--port" => port = value()?.parse().map_err(|e| format!("--port: {e}"))?,
            "--data-dir" => data_dir = Some(PathBuf::from(value()?)),
            "--keep-data" => keep_data = true,
            "-h" | "--help" => return Err(USAGE.to_owned()),
            other => return Err(format!("unknown argument {other}\n{USAGE}")),
        }
    }
    let app = app.ok_or_else(|| format!("--app is required\n{USAGE}"))?;
    let out = out.ok_or_else(|| format!("--out is required\n{USAGE}"))?;
    Ok(Args { app, out, driver, native_driver, port, data_dir, keep_data })
}

/// The expected strings that `text` does not contain.
fn missing<'a>(text: &str, expect: &[&'a str]) -> Vec<&'a str> {
    expect.iter().copied().filter(|e| !text.contains(e)).collect()
}

/// `\\?\D:\a\app.exe` -> `D:\a\app.exe`: canonicalize gives Windows' verbatim form, which the app path then
/// carries into msedgedriver and WebView2. A drive path without the prefix is the same file.
fn plain(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path,
    }
}

/// `http://127.0.0.1:1234/session/abc` -> `http://127.0.0.1:1234`.
fn origin(url: &str) -> Option<&str> {
    let after_scheme = url.find("://")? + 3;
    let end = url[after_scheme..].find('/').map_or(url.len(), |i| after_scheme + i);
    Some(&url[..end])
}

/// Kills tauri-driver (and with it the app) however the test ends.
struct DriverProcess(Child);

impl Drop for DriverProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn main() {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    match run(&args) {
        Ok(true) => println!("smoke: all checks passed"),
        Ok(false) => {
            eprintln!("smoke: FAILED (see {})", args.out.join("summary.txt").display());
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("smoke: could not run: {e}");
            std::process::exit(1);
        }
    }
}

/// Ok(true) when every check passed; Err when the app could not be started at all.
fn run(args: &Args) -> Result<bool, String> {
    std::fs::create_dir_all(&args.out).map_err(|e| format!("{}: {e}", args.out.display()))?;
    let out = plain(args.out.canonicalize().map_err(|e| format!("{}: {e}", args.out.display()))?);
    let app = plain(args.app.canonicalize().map_err(|e| format!("--app {}: {e}", args.app.display()))?);

    // A new, empty data folder: the app creates the big demo in it (03 "Demo seed data"). One that already holds a
    // database is refused, so the test can never open (or change) a real one.
    let data_dir = match &args.data_dir {
        Some(d) => d.clone(),
        None => {
            let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
            std::env::temp_dir().join(format!("karbonfm-smoke-{}-{nanos}", std::process::id()))
        }
    };
    if data_dir.join("karbonfm.db").exists() {
        return Err(format!("{} already has a karbonfm.db: give an empty folder", data_dir.display()));
    }
    std::fs::create_dir_all(&data_dir).map_err(|e| format!("{}: {e}", data_dir.display()))?;
    println!("smoke: app {}\nsmoke: data {}\nsmoke: out {}", app.display(), data_dir.display(), out.display());

    let mut report = Vec::new();
    let result = drive(args, &app, &data_dir, &out, &mut report);

    // The app's log, for a failed run.
    let log = data_dir.join("logs").join("karbonfm.log");
    if log.exists() {
        let _ = std::fs::copy(&log, out.join("karbonfm.log"));
    }
    if !args.keep_data && args.data_dir.is_none() {
        let _ = std::fs::remove_dir_all(&data_dir);
    }
    if let Err(e) = &result {
        report.push(format!("ERROR {e}"));
    }
    let summary = report.join("\n") + "\n";
    print!("{summary}");
    std::fs::write(out.join("summary.txt"), &summary).map_err(|e| format!("summary.txt: {e}"))?;
    result
}

fn drive(args: &Args, app: &Path, data_dir: &Path, out: &Path, report: &mut Vec<String>) -> Result<bool, String> {
    let log = std::fs::File::create(out.join("tauri-driver.log")).map_err(|e| format!("tauri-driver.log: {e}"))?;
    let log2 = log.try_clone().map_err(|e| e.to_string())?;
    let mut cmd = Command::new(&args.driver);
    cmd.arg("--port").arg(args.port.to_string()).arg("--native-port").arg((args.port + 1).to_string());
    if let Some(native) = &args.native_driver {
        cmd.arg("--native-driver").arg(native);
    }
    // The app inherits these through the platform driver. TAURI_AUTOMATION lets WebKitGTK accept the automation
    // session (Linux); it changes nothing on Windows.
    cmd.env("KARBONFM_DATA_DIR", data_dir).env("TAURI_AUTOMATION", "true");
    cmd.stdin(Stdio::null()).stdout(log).stderr(log2);
    let _process = DriverProcess(cmd.spawn().map_err(|e| format!("could not start {}: {e}", args.driver.display()))?);

    let driver = Driver::new(args.port);
    driver.wait_ready(Duration::from_secs(30))?;
    let started = Instant::now();
    let session = driver.new_session(&app.to_string_lossy())?;
    report.push(format!("ok   session started in {:.1}s", started.elapsed().as_secs_f32()));

    let mut passed = true;
    for (i, page) in PAGES.iter().enumerate() {
        passed &= check_page(&session, page, i == 0, out, report);
    }
    if let Err(e) = session.close() {
        report.push(format!("warn closing the session: {e}"));
    }
    Ok(passed)
}

/// Opens one page (the first one is where the window lands), checks it and takes its screenshot.
fn check_page(session: &Session, page: &Page, first: bool, out: &Path, report: &mut Vec<String>) -> bool {
    let mut passed = true;
    let mut fail = |report: &mut Vec<String>, what: String| {
        passed = false;
        report.push(format!("FAIL {} {what}", page.path));
    };
    // The window opens the one-time /session URL, which sets the cookie and sends it to the dashboard (AP-13). The
    // first start builds the big demo before the window opens: give it time.
    let opened = if first {
        Ok(())
    } else {
        session
            .url()
            .and_then(|u| origin(&u).map(str::to_owned).ok_or_else(|| format!("no origin in {u}")))
            .and_then(|o| session.navigate(&format!("{o}{}", page.path)))
    };
    let timeout = Duration::from_secs(if first { 120 } else { 30 });
    match opened.and_then(|()| session.wait_title(page.title, timeout)) {
        Ok(()) => report.push(format!("ok   {} title {:?}", page.path, page.title)),
        Err(e) => fail(report, e),
    }
    match session.url() {
        Ok(u) if u.starts_with("http://127.0.0.1:") && u.ends_with(page.path) => {
            report.push(format!("ok   {} url {u}", page.path))
        }
        Ok(u) => fail(report, format!("url {u} is not the local server's {}", page.path)),
        Err(e) => fail(report, e),
    }
    match session.visible_text() {
        Ok(text) => {
            let gone = missing(&text, page.expect);
            if gone.is_empty() {
                report.push(format!("ok   {} shows {}", page.path, page.expect.join(" | ")));
            } else {
                fail(report, format!("does not show {gone:?}"));
                let _ = std::fs::write(out.join(page.screenshot.replace(".png", ".txt")), &text);
            }
        }
        Err(e) => fail(report, e),
    }
    match session
        .screenshot()
        .and_then(|png| std::fs::write(out.join(page.screenshot), &png).map_err(|e| e.to_string()).map(|()| png.len()))
    {
        Ok(bytes) => report.push(format!("ok   {} screenshot {} ({bytes} bytes)", page.path, page.screenshot)),
        Err(e) => fail(report, format!("screenshot: {e}")),
    }
    passed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Result<Args, String> {
        parse_args(list.iter().map(|s| s.to_string()))
    }

    #[test]
    fn arguments() {
        let a = args(&["--app", "karbonfm", "--out", "shots"]).unwrap();
        assert_eq!((a.driver.to_str(), a.port, a.keep_data), (Some("tauri-driver"), 4444, false));
        let a = args(&["--app", "a", "--out", "o", "--native-driver", "msedgedriver.exe", "--port", "5555"]).unwrap();
        assert_eq!((a.native_driver.unwrap().to_str(), a.port), (Some("msedgedriver.exe"), 5555));
        assert!(args(&["--out", "o"]).unwrap_err().contains("--app is required"));
        assert!(args(&["--app"]).unwrap_err().contains("--app needs a value"));
        assert!(args(&["--app", "a", "--out", "o", "--bogus"]).unwrap_err().contains("unknown argument"));
    }

    #[test]
    fn verbatim_drive_paths_lose_their_prefix() {
        assert_eq!(plain(PathBuf::from(r"\\?\D:\a\karbonfm.exe")), PathBuf::from(r"D:\a\karbonfm.exe"));
        assert_eq!(plain(PathBuf::from(r"\\?\UNC\server\share\a.exe")), PathBuf::from(r"\\?\UNC\server\share\a.exe"));
        assert_eq!(
            plain(PathBuf::from("/src/app/target/release/karbonfm")),
            PathBuf::from("/src/app/target/release/karbonfm")
        );
    }

    #[test]
    fn origin_of_the_local_server() {
        assert_eq!(origin("http://127.0.0.1:51234/session/abc"), Some("http://127.0.0.1:51234"));
        assert_eq!(origin("http://127.0.0.1:51234/"), Some("http://127.0.0.1:51234"));
        assert_eq!(origin("http://127.0.0.1:51234"), Some("http://127.0.0.1:51234"));
        assert_eq!(origin("about:blank"), None);
    }

    #[test]
    fn missing_figures_are_named() {
        let text = "Dashboard\nGood day, Mara\nOpen pipeline\n$705,400.00";
        assert_eq!(missing(text, &["Good day, Mara", "$705,400.00"]), Vec::<&str>::new());
        assert_eq!(missing(text, &["Good day, Mara", "$393,521.01"]), vec!["$393,521.01"]);
    }

    /// The pages and figures stay in step with the screenshots CI uploads.
    #[test]
    fn pages_are_the_dashboard_and_inv_1048() {
        assert_eq!(PAGES.map(|p| p.screenshot), ["dashboard.png", "invoice-INV-1048.png"]);
        assert!(PAGES.iter().all(|p| p.title.ends_with(" · KarbonFM") && !p.expect.is_empty()));
    }
}
