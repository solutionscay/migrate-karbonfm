//! The SMTP contract test of I4 (03 "Done when", EXT-01; OP-INTEG-01, BR-INTEG-01, AP-02, AP-03, AP-08). The real
//! send route and lettre's SMTP transport talk to a capture server in this file, bound to 127.0.0.1 on a free port:
//! no real email, no real credentials. Each case says what the server answers and what the app must then hold.

use axum::body::Body;
use axum::http::{header, HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use karbon_server::secrets::{MemoryStore, Secret, SecretKey, SecretStore};
use karbon_server::{db, router, AppState};
use rusqlite::Connection;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tower::ServiceExt;

/// A fake password for the capture server; never a real credential.
const PASSWORD: &str = "capture-test-password-41";

// ---------------------------------------------------------------------------------------------------------------
// The capture server
// ---------------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Accepts everything.
    Ok,
    /// Answers AUTH with 535.
    AuthFails,
    /// Accepts the envelope, then answers the end of DATA with 554.
    DataFails,
}

#[derive(Default, Debug, Clone)]
struct Captured {
    auth: Vec<String>,
    mail_from: Vec<String>,
    rcpt_to: Vec<String>,
    data: String,
}

struct Capture {
    port: u16,
    connections: Arc<AtomicUsize>,
    captured: Arc<Mutex<Captured>>,
}

impl Capture {
    fn start(mode: Mode) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let connections = Arc::new(AtomicUsize::new(0));
        let captured = Arc::new(Mutex::new(Captured::default()));
        let (count, cap) = (connections.clone(), captured.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                count.fetch_add(1, Ordering::SeqCst);
                let cap = cap.clone();
                std::thread::spawn(move || serve(stream, mode, cap));
            }
        });
        Capture { port, connections, captured }
    }
    fn captured(&self) -> Captured {
        self.captured.lock().unwrap().clone()
    }
}

fn serve(stream: std::net::TcpStream, mode: Mode, cap: Arc<Mutex<Captured>>) {
    let mut out = stream.try_clone().unwrap();
    let mut lines = BufReader::new(stream);
    let mut say = |s: &str| {
        let _ = out.write_all(s.as_bytes());
        let _ = out.flush();
    };
    say("220 capture.local ESMTP ready\r\n");
    let mut line = String::new();
    loop {
        line.clear();
        if lines.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let cmd = line.trim_end().to_owned();
        let upper = cmd.to_ascii_uppercase();
        if upper.starts_with("EHLO") || upper.starts_with("HELO") {
            say("250-capture.local\r\n250-AUTH PLAIN LOGIN\r\n250 8BITMIME\r\n");
        } else if upper.starts_with("AUTH") {
            cap.lock().unwrap().auth.push(cmd.clone());
            if mode == Mode::AuthFails {
                say("535 5.7.8 Authentication credentials invalid\r\n");
            } else {
                say("235 2.7.0 Authentication successful\r\n");
            }
        } else if upper.starts_with("MAIL FROM") {
            cap.lock().unwrap().mail_from.push(cmd.clone());
            say("250 2.1.0 OK\r\n");
        } else if upper.starts_with("RCPT TO") {
            let addr =
                cmd.split_once(':').map(|(_, a)| a.trim().trim_matches(['<', '>']).to_owned()).unwrap_or_default();
            cap.lock().unwrap().rcpt_to.push(addr);
            say("250 2.1.5 OK\r\n");
        } else if upper == "DATA" {
            say("354 End data with <CR><LF>.<CR><LF>\r\n");
            let mut data = String::new();
            loop {
                line.clear();
                if lines.read_line(&mut line).unwrap_or(0) == 0 {
                    return;
                }
                if line == ".\r\n" {
                    break;
                }
                data.push_str(&line);
            }
            cap.lock().unwrap().data = data;
            if mode == Mode::DataFails {
                say("554 5.6.0 Message rejected by policy\r\n");
            } else {
                say("250 2.0.0 Ok: queued as CAPTURE42\r\n");
            }
        } else if upper == "QUIT" {
            say("221 2.0.0 Bye\r\n");
            return;
        } else if upper == "RSET" || upper == "NOOP" {
            say("250 OK\r\n");
        } else {
            say("502 5.5.2 Command not recognized\r\n");
        }
    }
}

/// A port nothing listens on: bound, then released.
fn closed_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

// ---------------------------------------------------------------------------------------------------------------
// The app around it
// ---------------------------------------------------------------------------------------------------------------

/// The app log of this test binary, once (`applog` keeps one logger per process).
fn log_dir() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("karbonfm-smtp-contract-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        karbon_server::applog::init(&dir).unwrap();
        dir
    })
}

fn log_text() -> String {
    std::fs::read_to_string(log_dir().join("karbonfm.log")).unwrap_or_default()
}

struct App {
    router: axum::Router,
    secrets: Arc<MemoryStore>,
    path: PathBuf,
    _tmp: tempfile::TempDir,
}

impl App {
    /// The small demo in a temp data folder, both accounts pointed at the capture server on `port` (plain text,
    /// password authentication), and the fake password in a memory keychain.
    fn new(port: u16) -> Self {
        log_dir();
        let tmp = tempfile::tempdir().unwrap();
        let path = db::database_path(tmp.path());
        let conn = db::open_demo(Some(&path)).unwrap();
        let dir = tmp.path().to_path_buf();
        Self::around(port, tmp, dir, path, conn)
    }

    /// The big demo: the same synthetic export and import the desktop app opens on.
    fn big_demo(port: u16) -> Self {
        log_dir();
        let tmp = tempfile::tempdir().unwrap();
        let (export, data) = (tmp.path().join("export"), tmp.path().join("data"));
        karbon_synth::generate(karbon_synth::Config::default(), &export).unwrap();
        karbon_server::import::import_export(&export, &data, &Default::default()).unwrap();
        let path = db::database_path(&data);
        let conn = db::open(&path, db::Seed::Reference).unwrap().0;
        Self::around(port, tmp, data, path, conn)
    }

    fn around(port: u16, tmp: tempfile::TempDir, dir: PathBuf, path: PathBuf, conn: Connection) -> Self {
        conn.execute(
            "UPDATE system_setting SET system_email_address = 'billing@ellisonstudio.example',
                    system_email_full_name = 'Ellison Studio', system_email_user_name = 'billing',
                    system_email_smtp_server = '127.0.0.1', system_email_smtp_server_port = ?1,
                    system_email_smtp_server_encryption_type = 'None', system_email_smtp_server_authentication = 'Password'",
            [port],
        )
        .unwrap();
        conn.execute(
            "UPDATE user SET smtp_email_address = 'mara@ellisonstudio.example', smtp_full_name = 'Mara Ellison',
                    smtp_user_name = 'mara', smtp_server = NULL, smtp_server_port = NULL",
            [],
        )
        .unwrap();
        let secrets = Arc::new(MemoryStore::default());
        secrets.set(SecretKey::SystemSmtpPassword, &Secret::new(PASSWORD)).unwrap();
        secrets.set(SecretKey::UserSmtpPassword, &Secret::new(PASSWORD)).unwrap();
        let state = AppState::new(conn).with_secrets(secrets.clone()).with_data_dir(&dir);
        App { router: router(state), secrets, path, _tmp: tmp }
    }

    fn read(&self) -> Connection {
        Connection::open(&self.path).unwrap()
    }

    async fn send(&self, method: &str, uri: &str, body: &str) -> (StatusCode, HeaderMap, String) {
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header("HX-Request", "true")
            .body(Body::from(body.to_owned()))
            .unwrap();
        let res = self.router.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let headers = res.headers().clone();
        let body = res.into_body().collect().await.unwrap().to_bytes();
        (status, headers, String::from_utf8_lossy(&body).into_owned())
    }

    async fn page(&self, uri: &str) -> String {
        let res = self.router.clone().oneshot(Request::get(uri).body(Body::empty()).unwrap()).await.unwrap();
        String::from_utf8_lossy(&res.into_body().collect().await.unwrap().to_bytes()).into_owned()
    }

    fn email_of(&self, party: &str) -> (String, String) {
        self.read()
            .query_row(
                "SELECT cm.id, n.address FROM contact_method cm JOIN network_address n ON n.id = cm.id
                 WHERE cm.party_id = ?1 AND n.email_address_ind = 1",
                [party],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
    }

    /// An unsent email to Elena (To), Marcus (Cc) and Joanna (Bcc), through the routes; returns its id.
    async fn draft(&self, account: &str) -> String {
        let (cm, _) = self.email_of("p-hp-eh");
        let body = format!("party_id=p-hp-eh&email_id={cm}&entity_type_id=&entity_id=&subject=Lighting+schedule&message=Hi+Elena%2C%0Athe+warmer+option+is+attached.");
        let (status, h, _) = self.send("POST", "/email-messages", &body).await;
        assert_eq!(status, StatusCode::OK);
        let id = h["HX-Redirect"].to_str().unwrap().trim_start_matches("/email-messages/").to_owned();
        for (role, party) in [(56, "p-hp-mp"), (57, "p-hp-jw")] {
            let (cm, _) = self.email_of(party);
            let (status, _, html) = self
                .send(
                    "PUT",
                    &format!("/activities/{id}/parties"),
                    &format!("role={role}&other_id={party}&q=&email_id={cm}"),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "{html}");
        }
        let (status, _, _) = self
            .send(
                "PATCH",
                &format!("/email-messages/{id}"),
                &format!(
                    "subject=Lighting+schedule&message=Hi+Elena%2C%0Athe+warmer+option+is+attached.&account={account}"
                ),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        id
    }

    fn sent_row(&self, id: &str) -> (i64, Option<String>, Option<String>) {
        self.read()
            .query_row(
                "SELECT COALESCE(sent_ind, 0), smtp_timestamp, smtp_result FROM email_message WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap()
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The cases
// ---------------------------------------------------------------------------------------------------------------

/// 1. Success: the envelope has To, Cc and Bcc; the headers have From "Ellison Studio" <account address>, To, Cc and
/// the subject, no Bcc; the body is plain text. Then sent_ind = 1, smtp_timestamp, smtp_result = the server's reply,
/// and the page is read only (AP-08).
#[tokio::test]
async fn success_sends_plain_text_with_bcc_in_the_envelope_and_marks_it_sent() {
    let server = Capture::start(Mode::Ok);
    let app = App::new(server.port);
    let id = app.draft("system").await;
    let (status, h, _) = app.send("POST", &format!("/email-messages/{id}/send"), "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(h["HX-Redirect"], format!("/email-messages/{id}").as_str());
    assert!(h["HX-Trigger"].to_str().unwrap().contains("Email sent"));

    let c = server.captured();
    let (_, elena) = app.email_of("p-hp-eh");
    let (_, marcus) = app.email_of("p-hp-mp");
    let (_, joanna) = app.email_of("p-hp-jw");
    assert_eq!(c.mail_from.len(), 1);
    assert!(c.mail_from[0].contains("<billing@ellisonstudio.example>"), "{:?}", c.mail_from);
    assert_eq!(c.rcpt_to, [elena.clone(), marcus.clone(), joanna.clone()], "To, Cc and Bcc in the envelope");
    assert!(c.auth.iter().any(|a| a.starts_with("AUTH")), "password authentication: {:?}", c.auth);
    let data = c.data;
    assert!(data.contains("From: \"Ellison Studio\" <billing@ellisonstudio.example>"), "{data}");
    assert!(data.contains(&format!("To: {elena}")), "{data}");
    assert!(data.contains(&format!("Cc: {marcus}")), "{data}");
    assert!(!data.to_ascii_lowercase().contains("bcc:") && !data.contains(&joanna), "no Bcc header: {data}");
    assert!(data.contains("Subject: Lighting schedule"));
    // B12: a Message-ID on the sender's domain, never the machine's host name.
    let mid = data.lines().find(|l| l.to_ascii_lowercase().starts_with("message-id:")).expect("a Message-ID header");
    assert!(mid.trim_end().ends_with("@ellisonstudio.example>"), "{mid}");
    assert!(data.contains("Content-Type: text/plain"));
    assert!(data.contains("the warmer option is attached."));
    assert!(!data.contains("text/html"), "no HTML part (337 s160)");

    let (sent, at, reply) = app.sent_row(&id);
    assert_eq!(sent, 1);
    assert!(karbon_server::audit::is_timestamp(at.as_deref().unwrap()));
    assert_eq!(reply.as_deref(), Some("250 2.0.0 Ok: queued as CAPTURE42"));
    let page = app.page(&format!("/email-messages/{id}")).await;
    assert!(page.contains("Sent</span>") && page.contains("queued as CAPTURE42"));
    assert!(!page.contains(r#"name="subject""#) && !page.contains("/send"), "read only once sent");
    assert!(!page.contains(PASSWORD));
}

/// 2. The server refuses the sign-in (535): nothing is written, the message stays unsent, one masked log line;
/// with the user's account its password leaves the keychain and the panel asks for it (kontacts 775 s18-29).
#[tokio::test]
async fn an_auth_failure_leaves_it_unsent_and_forgets_the_user_password() {
    let server = Capture::start(Mode::AuthFails);
    let app = App::new(server.port);
    let id = app.draft("user").await;
    let (status, _, html) = app.send("POST", &format!("/email-messages/{id}/send"), "").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("refused the sign-in") && html.contains(r#"name="password""#), "{html}");
    assert!(!html.contains(PASSWORD));
    assert_eq!(app.sent_row(&id), (0, None, None));
    assert!(app.secrets.get(SecretKey::UserSmtpPassword).unwrap().is_none(), "the user password is forgotten");
    assert!(app.secrets.get(SecretKey::SystemSmtpPassword).unwrap().is_some(), "the system one stays");
    assert!(server.captured().data.is_empty(), "no message data after the refusal");
    let log = log_text();
    assert!(log.contains("email_send") && log.contains("Integration"), "{log}");
    assert!(!log.contains(PASSWORD), "the log never holds the password");

    // Typing it again in the send dialog: still refused here, so it is not kept.
    let (status, _, _) = app.send("POST", &format!("/email-messages/{id}/send"), "password=typed-again").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(app.secrets.get(SecretKey::UserSmtpPassword).unwrap().is_none());
}

/// 3. 554 at the end of DATA: unsent, nothing written.
#[tokio::test]
async fn a_rejected_message_stays_unsent() {
    let server = Capture::start(Mode::DataFails);
    let app = App::new(server.port);
    let id = app.draft("system").await;
    let (status, _, html) = app.send("POST", &format!("/email-messages/{id}/send"), "").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("could not be sent") && html.contains("not sent"), "{html}");
    assert_eq!(app.sent_row(&id), (0, None, None));
    assert!(!server.captured().data.is_empty(), "the server got the data and rejected it");
}

/// 4. Nothing listens on the port: unsent, nothing written.
#[tokio::test]
async fn a_refused_connection_leaves_it_unsent() {
    let app = App::new(closed_port());
    let id = app.draft("system").await;
    let (status, _, html) = app.send("POST", &format!("/email-messages/{id}/send"), "").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("could not be sent"), "{html}");
    assert_eq!(app.sent_row(&id), (0, None, None));
}

/// 5. A host the outbound policy does not allow (AP-02, T-08-02 style): refused before any connection.
#[test]
fn a_host_outside_the_policy_is_refused_without_connecting() {
    let server = Capture::start(Mode::Ok);
    let tmp = tempfile::tempdir().unwrap();
    let path = db::database_path(tmp.path());
    let conn = db::open_demo(Some(&path)).unwrap();
    conn.execute(
        "UPDATE system_setting SET system_email_address = 'billing@ellisonstudio.example', system_email_user_name = 'b',
                system_email_smtp_server = '127.0.0.1', system_email_smtp_server_port = ?1,
                system_email_smtp_server_encryption_type = 'None', system_email_smtp_server_authentication = 'Password'",
        [server.port],
    )
    .unwrap();
    let actor = karbon_server::admin::current_user(&conn).unwrap().unwrap();
    let db = Mutex::new(conn);
    let secrets = MemoryStore::default();
    secrets.set(SecretKey::SystemSmtpPassword, &Secret::new(PASSWORD)).unwrap();
    let new = karbon_server::email::NewEmail {
        subject: "S".into(),
        message: "M".into(),
        to: vec![("p-hp-eh".into(), "cm-037".into())],
        use_system_settings: true,
        ..Default::default()
    };
    let id = karbon_server::email::email_create(&mut db.lock().unwrap(), &new, &actor).unwrap();
    let deny = karbon_server::net::OutboundPolicy::deny_all();
    let mailer = karbon_server::email::LettreMailer::default();
    let r = karbon_server::email::email_send(&db, &secrets, &mailer, Some(&deny), &id, &actor, None);
    assert!(matches!(r, Err(karbon_server::admin::CommandError::Refused(ref m)) if m.contains("not enabled")), "{r:?}");
    assert_eq!(server.connections.load(Ordering::SeqCst), 0, "nothing connected");
    // With the policy from the settings it goes through.
    let r = karbon_server::email::email_send(&db, &secrets, &mailer, None, &id, &actor, None).unwrap();
    assert!(matches!(r, karbon_server::email::SendOutcome::Sent(_)));
}

/// 6. A missing To, password or server: field errors, no connection (337 s103).
#[tokio::test]
async fn missing_items_are_listed_and_nothing_connects() {
    let server = Capture::start(Mode::Ok);
    let app = App::new(server.port);
    let (status, h, _) = app
        .send("POST", "/email-messages", "party_id=&email_id=&entity_type_id=&entity_id=&subject=Hello&message=Text")
        .await;
    assert_eq!(status, StatusCode::OK);
    let id = h["HX-Redirect"].to_str().unwrap().trim_start_matches("/email-messages/").to_owned();
    app.send("PATCH", &format!("/email-messages/{id}"), "subject=Hello&message=Text&account=system").await;
    app.secrets.delete(SecretKey::SystemSmtpPassword).unwrap();
    app.read().execute("UPDATE system_setting SET system_email_smtp_server = NULL", []).unwrap();
    let (status, _, html) = app.send("POST", &format!("/email-messages/{id}/send"), "").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Add at least one recipient"), "{html}");
    assert!(html.contains("Set the SMTP server of the system email account"), "{html}");
    assert!(html.contains(r#"name="password""#), "the password is asked");
    assert_eq!(server.connections.load(Ordering::SeqCst), 0);
    assert_eq!(app.sent_row(&id), (0, None, None));
}

/// 7. The user account takes the system server and port when its own are empty (337 s85-88), signs in with its own
/// user name, and the From header is the user's name and address; a password typed in the dialog is kept after a
/// successful send.
#[tokio::test]
async fn the_user_account_falls_back_to_the_system_server_and_keeps_a_typed_password() {
    let server = Capture::start(Mode::Ok);
    let app = App::new(server.port);
    app.secrets.delete(SecretKey::UserSmtpPassword).unwrap();
    let id = app.draft("user").await;
    let (status, _, html) = app.send("POST", &format!("/email-messages/{id}/send"), "").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "no user password yet: the dialog asks");
    assert!(html.contains(r#"name="password""#) && !html.contains("Not sent yet"), "{html}");
    let (status, _, _) = app.send("POST", &format!("/email-messages/{id}/send"), &format!("password={PASSWORD}")).await;
    assert_eq!(status, StatusCode::OK);
    let c = server.captured();
    assert!(c.mail_from[0].contains("<mara@ellisonstudio.example>"));
    assert!(c.data.contains("From: \"Mara Ellison\" <mara@ellisonstudio.example>"), "{}", c.data);
    let plain = c.auth.iter().find(|a| a.starts_with("AUTH PLAIN")).cloned().unwrap_or_default();
    if let Some(b64) = plain.split_whitespace().nth(2) {
        let decoded = base64_decode(b64);
        assert!(decoded.contains("mara"), "the user's own user name");
    }
    assert_eq!(
        app.secrets.get(SecretKey::UserSmtpPassword).unwrap().map(|s| s.expose().to_owned()).as_deref(),
        Some(PASSWORD)
    );
    assert_eq!(app.sent_row(&id).0, 1);
}

/// 8. T-08-08 (AP-08) on the send: a sent message is refused with AlreadySent and nothing connects.
#[tokio::test]
async fn t_08_08_a_sent_message_is_not_sent_again() {
    let server = Capture::start(Mode::Ok);
    let app = App::new(server.port);
    let (status, _, text) = app.send("POST", "/email-messages/a-mail-1048/send", "").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(text.contains("already been sent"));
    assert_eq!(server.connections.load(Ordering::SeqCst), 0);
    let id = app.draft("system").await;
    assert_eq!(app.send("POST", &format!("/email-messages/{id}/send"), "").await.0, StatusCode::OK);
    let before = server.connections.load(Ordering::SeqCst);
    let (status, _, _) = app.send("POST", &format!("/email-messages/{id}/send"), "").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(server.connections.load(Ordering::SeqCst), before, "no second connection");
    let (status, _, _) =
        app.send("PATCH", &format!("/email-messages/{id}"), "subject=x&message=y&account=system").await;
    assert_eq!(status, StatusCode::CONFLICT);
}

// ---------------------------------------------------------------------------------------------------------------
// Merge fields reach the mail (BR-TEMPLATE-01): a recipient never gets a literal {{Name}}
// ---------------------------------------------------------------------------------------------------------------

impl App {
    /// A template with this text; returns its id.
    async fn template(&self, message: &str) -> i64 {
        let (status, h, _) = self.send("POST", "/templates", "name=Merge+check").await;
        assert_eq!(status, StatusCode::CREATED);
        let id = h["HX-Redirect"].to_str().unwrap().trim_start_matches("/templates/").to_owned();
        let body = format!("name=Merge+check&subject=&message={}", urlencode(message));
        let (status, _, html) = self.send("PUT", &format!("/templates/{id}"), &body).await;
        assert_eq!(status, StatusCode::OK, "{html}");
        id.parse().unwrap()
    }

    async fn patch(&self, id: &str, subject: &str, message: &str) {
        let body = format!("subject={}&message={}&account=system", urlencode(subject), urlencode(message));
        let (status, _, html) = self.send("PATCH", &format!("/email-messages/{id}"), &body).await;
        assert_eq!(status, StatusCode::OK, "{html}");
    }

    fn message_of(&self, id: &str) -> String {
        self.read().query_row("SELECT message FROM activity WHERE id = ?1", [id], |r| r.get(0)).unwrap()
    }
}

/// Form encoding of what these bodies hold.
fn urlencode(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if c.is_ascii_alphanumeric() {
                vec![c.to_string()]
            } else {
                let mut b = [0; 4];
                c.encode_utf8(&mut b).bytes().map(|x| format!("%{x:02X}")).collect()
            }
        })
        .collect()
}

#[tokio::test]
async fn a_token_typed_by_hand_is_merged_in_the_mail_and_in_the_sent_record() {
    let server = Capture::start(Mode::Ok);
    let app = App::new(server.port);
    let id = app.draft("system").await;
    app.patch(&id, "For {{FirstName}}", "Hi {{FirstName}},\nfrom {{SenderName}}.").await;
    let (status, _, html) = app.send("POST", &format!("/email-messages/{id}/send"), "").await;
    assert_eq!(status, StatusCode::OK, "{html}");
    let data = server.captured().data;
    assert!(data.contains("Subject: For Elena"), "{data}");
    assert!(data.contains("Hi Elena,") && data.contains("from Mara Ellison."), "{data}");
    assert!(!data.contains("{{"), "no literal placeholder: {data}");
    assert!(app.message_of(&id).starts_with("Hi Elena,"), "the record says what went out");
    let page = app.page(&format!("/email-messages/{id}")).await;
    assert!(page.contains("For Elena") && !page.contains("{{FirstName}}"), "{page}");
}

#[tokio::test]
async fn a_template_applied_to_a_draft_reaches_the_mail_with_its_values() {
    let server = Capture::start(Mode::Ok);
    let app = App::new(server.port);
    let id = app.draft("system").await;
    let template = app.template("Dear {{FirstName}} of {{OrganizationName}}").await;
    let (status, _, _) =
        app.send("POST", &format!("/email-messages/{id}/template"), &format!("template_id={template}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(app.message_of(&id), "Dear Elena of Halden & Pryce Architects", "filled in on selection");
    let (status, _, html) = app.send("POST", &format!("/email-messages/{id}/send"), "").await;
    assert_eq!(status, StatusCode::OK, "{html}");
    let data = server.captured().data;
    assert!(data.contains("Dear Elena of Halden & Pryce Architects"), "{data}");
    assert!(!data.contains("{{"), "{data}");
}

/// New > Email has no recipient yet, so the template goes on first: its field waits for the recipient, not lost.
#[tokio::test]
async fn a_template_applied_before_the_recipient_fills_in_when_it_is_sent() {
    let server = Capture::start(Mode::Ok);
    let app = App::new(server.port);
    let (status, h, _) = app.send("POST", "/email-messages", "subject=Hello&message=x").await;
    assert_eq!(status, StatusCode::OK);
    let id = h["HX-Redirect"].to_str().unwrap().trim_start_matches("/email-messages/").to_owned();
    let template = app.template("Dear {{FirstName}}").await;
    app.send("POST", &format!("/email-messages/{id}/template"), &format!("template_id={template}")).await;
    assert_eq!(app.message_of(&id), "Dear {{FirstName}}", "kept, for the recipient still to come");

    let (cm, _) = app.email_of("p-hp-eh");
    let body = format!("role=55&other_id=p-hp-eh&q=&email_id={cm}");
    let (status, _, html) = app.send("PUT", &format!("/activities/{id}/parties"), &body).await;
    assert_eq!(status, StatusCode::OK, "{html}");
    let (status, _, html) = app.send("POST", &format!("/email-messages/{id}/send"), "").await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(server.captured().data.contains("Dear Elena"), "{:?}", server.captured().data);
}

impl App {
    /// What an invoice's Email button does: its dialog, with the customer's primary address, saved as a draft.
    async fn email_from_invoice(&self, doc: &str) -> String {
        let page = self.page(&format!("/sales/{doc}")).await;
        let button = r#"hx-get=""#;
        let start = page.find(r#"hx-get="/email-messages/new?party_id="#).expect("the Email button") + button.len();
        let uri = page[start..start + page[start..].find('"').unwrap()].replace("&amp;", "&");
        let (status, _, dialog) = self.send("GET", &uri, "").await;
        assert_eq!(status, StatusCode::OK);
        let radio = r#"name="email_id" value=""#;
        let at = dialog.find(radio).expect("the customer has an email address") + radio.len();
        let email_id = &dialog[at..at + dialog[at..].find('"').unwrap()];
        let body = format!("{}&email_id={email_id}&subject=About+{doc}&message=Draft", uri.split_once('?').unwrap().1);
        let (status, h, html) = self.send("POST", "/email-messages", &body).await;
        assert_eq!(status, StatusCode::OK, "{html}");
        h["HX-Redirect"].to_str().unwrap().trim_start_matches("/email-messages/").to_owned()
    }

    /// Applies each template to a new draft from the invoice's Email button, sends it, and checks what arrives.
    async fn every_template_sends(&self, server: &Capture, doc: &str, recipient: &str, templates: &[(i64, String)]) {
        for (template, name) in templates {
            let id = self.email_from_invoice(doc).await;
            let (status, _, html) =
                self.send("POST", &format!("/email-messages/{id}/template"), &format!("template_id={template}")).await;
            assert_eq!(status, StatusCode::OK, "{name}: {html}");
            let (status, _, html) = self.send("POST", &format!("/email-messages/{id}/send"), "").await;
            assert_eq!(status, StatusCode::OK, "{name} on {doc} did not send: {html}");
            let data = server.captured().data;
            assert!(!data.contains("{{") && !data.contains("}}"), "{name} on {doc}: {data}");
            if name == "Invoice email" || name == "Payment reminder" {
                assert!(data.contains(&format!("Hi {recipient},")), "{name} on {doc}: {data}");
            }
            if name == "Invoice email" {
                assert!(data.contains(&format!("invoice {doc} attached")), "{name} on {doc}: {data}");
            }
        }
    }
}

/// The demo's most obvious email flow: an invoice's Email button addresses an organization, which has no first name, so
/// a seeded template must use only fields that have a value there. The small seed has no templates of its own, so it
/// runs the big demo's text, as a template a user made there would.
#[tokio::test]
async fn every_seeded_template_sends_from_an_invoice_to_an_organization() {
    let server = Capture::start(Mode::Ok);
    let big = App::big_demo(server.port);
    let seeded: Vec<(i64, String, String)> = {
        let c = big.read();
        let mut stmt = c.prepare("SELECT id, name, COALESCE(message, '') FROM template ORDER BY id").unwrap();
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        rows.collect::<rusqlite::Result<_>>().unwrap()
    };
    assert_eq!(seeded.len(), 6);
    assert!(seeded.iter().any(|(_, name, _)| name == "Invoice email"));
    let templates: Vec<(i64, String)> = seeded.iter().map(|(id, name, _)| (*id, name.clone())).collect();
    let docs: Vec<(String, String)> = {
        let c = big.read();
        let mut stmt = c
            .prepare(
                "SELECT st.doc_number, p.name_cached FROM sales_transaction st
                 JOIN party_role r ON r.id = st.id_customer JOIN party p ON p.id = r.party_id
                 WHERE st.invoice_ind = 1 AND st.locked_ind = 1 AND p.organization_ind = 1 AND st.due_date IS NOT NULL
                   AND p.name_cached NOT LIKE '%&%'
                   AND EXISTS (SELECT 1 FROM contact_method cm JOIN network_address n ON n.id = cm.id
                               WHERE cm.party_id = p.id AND n.email_address_ind = 1)
                 GROUP BY p.id ORDER BY MAX(st.date) DESC LIMIT 3",
            )
            .unwrap();
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        rows.collect::<rusqlite::Result<_>>().unwrap()
    };
    assert_eq!(docs.len(), 3);
    for (doc, name) in &docs {
        big.every_template_sends(&server, doc, name, &templates).await;
    }

    let small = App::new(server.port);
    let own: i64 = small.read().query_row("SELECT COUNT(*) FROM template", [], |r| r.get(0)).unwrap();
    assert_eq!(own, 0, "the small seed has templates now: run them here too");
    let mut copies = Vec::new();
    for (_, name, message) in &seeded {
        copies.push((small.template(message).await, name.clone()));
    }
    small.every_template_sends(&server, "INV-1048", "Halden & Pryce Architects", &copies).await;
}

/// A field with no value, or a name that is not a field, stops the send and says which: never a literal in the mail
/// and never a silent gap.
#[tokio::test]
async fn a_token_with_no_value_stops_the_send_and_nothing_connects() {
    let server = Capture::start(Mode::Ok);
    let app = App::new(server.port);
    let id = app.draft("system").await;
    for (text, says) in [("Total {{InvoiceTotal}}", "{{InvoiceTotal}}"), ("Hi {{Nope}}", "{{Nope}}")] {
        app.patch(&id, "Subject", text).await;
        let (status, _, html) = app.send("POST", &format!("/email-messages/{id}/send"), "").await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{html}");
        assert!(html.contains(says), "{html}");
        assert_eq!(server.connections.load(Ordering::SeqCst), 0, "nothing connects");
        assert_eq!(app.sent_row(&id), (0, None, None));
        assert_eq!(app.message_of(&id), text, "the draft stays as written");
    }
}

/// A small base64 decoder for the AUTH PLAIN check (no extra crate).
fn base64_decode(s: &str) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut bits = 0u32;
    let mut n = 0;
    let mut out = Vec::new();
    for c in s.bytes().filter(|c| *c != b'=') {
        let Some(v) = A.iter().position(|a| *a == c) else { continue };
        bits = (bits << 6) | v as u32;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((bits >> n) as u8);
        }
    }
    String::from_utf8_lossy(&out).replace('\0', " ")
}
