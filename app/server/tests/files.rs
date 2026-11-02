//! I4 S6: files and attachments (UI-19, OP-FILE-01..03, BR-FILE-01..03). Every run uses a temp data folder
//! (`KARBONFM_DATA_DIR` is never read here; the state gets the folder directly).

use axum::body::Body;
use axum::http::{header, HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use karbon_server::admin::{current_user, CommandError};
use karbon_server::audit::Actor;
use karbon_server::files::{self, FileInput, LinkInput};
use karbon_server::{db, router, AppState, OpenHook};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

/// The small demo in a temp data folder: (folder, connection for the commands, actor).
fn demo() -> (tempfile::TempDir, Connection, Actor) {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open_demo(Some(&db::database_path(dir.path()))).unwrap();
    let actor = current_user(&conn).unwrap().unwrap();
    (dir, conn, actor)
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x % 255) as u8, (y % 255) as u8, 128]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn jpeg(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbImage::from_fn(w, h, |x, _| image::Rgb([(x % 255) as u8, 40, 200]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Jpeg).unwrap();
    out.into_inner()
}

fn upload(name: &str, bytes: Vec<u8>, link: Option<LinkInput>) -> FileInput {
    FileInput { file_name: name.into(), bytes, link, ..Default::default() }
}

fn party(id: &str) -> Option<LinkInput> {
    Some(LinkInput { entity_type_id: 10, entity_id: id.into(), include_on_send: false })
}

/// Every regular file under the data folder's file store.
fn stored(dir: &Path) -> Vec<PathBuf> {
    fn walk(p: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(rd) = std::fs::read_dir(p) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out)
                } else {
                    out.push(p)
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(&dir.join("files"), &mut out);
    out.sort();
    out
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

// ---------------------------------------------------------------------------------------------------------------
// OP-FILE-01 (BR-FILE-01, BR-FILE-02)
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn twenty_480_004_bytes_are_accepted_and_20_480_005_refused() {
    let (dir, mut conn, actor) = demo();
    let ok = files::file_add(&mut conn, dir.path(), &upload("big.bin", vec![7; 20_480_004], None), &actor).unwrap();
    let kb: f64 = conn.query_row("SELECT file_size FROM file WHERE id = ?1", [&ok], |r| r.get(0)).unwrap();
    assert_eq!(kb, 20480.0);
    let before = stored(dir.path()).len();
    let rows = count(&conn, "SELECT COUNT(*) FROM file");
    let e =
        files::file_add(&mut conn, dir.path(), &upload("bigger.bin", vec![7; 20_480_005], None), &actor).unwrap_err();
    match e {
        CommandError::Validation(v) => assert!(v[0].message.contains("20 MB"), "{v:?}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(stored(dir.path()).len(), before, "nothing stored");
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM file"), rows, "no row");
}

#[test]
fn a_file_is_copied_to_files_year_id_ext_with_its_row() {
    let (dir, mut conn, actor) = demo();
    let bytes = b"%PDF-1.4 a proposal".to_vec();
    let id = files::file_add(
        &mut conn,
        dir.path(),
        &FileInput {
            note: " Phase 3 ".into(),
            tag: "proposal".into(),
            ..upload("Proposal v4.PDF", bytes.clone(), party("p-hp"))
        },
        &actor,
    )
    .unwrap();
    let (path, ext, ctype, cat, thumb, note, tag, by, mod_by, created): (
        String,
        String,
        String,
        String,
        Option<String>,
        String,
        String,
        String,
        String,
        String,
    ) = conn
        .query_row(
            "SELECT container_path, extension, content_type, category, thumbnail_path, note, tag, created_by, \
             modified_by, creation_timestamp FROM file WHERE id = ?1",
            [&id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                    r.get(9)?,
                ))
            },
        )
        .unwrap();
    let year = &created[..4];
    assert_eq!(path, format!("files/{year}/{id}.PDF"));
    assert_eq!(std::fs::read(dir.path().join(&path)).unwrap(), bytes, "the bytes are equal");
    assert_eq!((ext.as_str(), ctype.as_str(), cat.as_str()), ("PDF", "application/pdf", "Document"));
    assert_eq!(thumb, None, "a PDF gets no thumbnail");
    assert_eq!((note.as_str(), tag.as_str()), ("Phase 3", "proposal"));
    assert_eq!((by.as_str(), mod_by.as_str()), (actor.account_name.as_str(), actor.account_name.as_str()));
    let (et, ename, send, fa_by): (i64, String, i64, String) = conn
        .query_row(
            "SELECT entity_type_id, entity_type, include_on_send, created_by FROM file_attachment WHERE attachment_id = ?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!((et, ename.as_str(), send, fa_by.as_str()), (10, "Party", 0, actor.account_name.as_str()));
    assert!(db::foreign_key_check(&conn).unwrap().is_empty());
}

#[test]
fn images_get_a_200_px_thumbnail_and_jpeg_is_an_image() {
    let (dir, mut conn, actor) = demo();
    for (name, bytes, ctype) in [
        ("site.png", png(800, 400), "image/png"),
        ("site.jpeg", jpeg(300, 600), "image/jpeg"),
        ("x.jpg", jpeg(20, 20), "image/jpg"),
    ] {
        let id = files::file_add(&mut conn, dir.path(), &upload(name, bytes, None), &actor).unwrap();
        let (cat, ct, thumb): (String, String, String) = conn
            .query_row("SELECT category, content_type, thumbnail_path FROM file WHERE id = ?1", [&id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!((cat.as_str(), ct.as_str()), ("Image", ctype), "{name}");
        assert!(thumb.ends_with(&format!("{id}-thumbnail.png")), "{thumb}");
        let t = image::open(dir.path().join(&thumb)).unwrap();
        assert!(
            t.width() <= 200 && t.height() <= 200 && (t.width() == 200 || t.height() == 200 || name == "x.jpg"),
            "{name}: {}x{}",
            t.width(),
            t.height()
        );
    }
    // A file named .png that is no image: kept, no thumbnail.
    let id =
        files::file_add(&mut conn, dir.path(), &upload("fake.png", b"not an image".to_vec(), None), &actor).unwrap();
    let thumb: Option<String> =
        conn.query_row("SELECT thumbnail_path FROM file WHERE id = ?1", [&id], |r| r.get(0)).unwrap();
    assert_eq!(thumb, None);
}

#[test]
fn a_failed_insert_leaves_no_file_behind() {
    let (dir, mut conn, actor) = demo();
    conn.execute_batch(
        "CREATE TEMP TRIGGER fail BEFORE INSERT ON file_attachment BEGIN SELECT RAISE(ABORT, 'boom'); END;",
    )
    .unwrap();
    let rows = count(&conn, "SELECT COUNT(*) FROM file");
    let e = files::file_add(&mut conn, dir.path(), &upload("p.png", png(50, 50), party("p-hp")), &actor).unwrap_err();
    assert!(matches!(e, CommandError::Storage(_)), "{e:?}");
    assert_eq!(stored(dir.path()), Vec::<PathBuf>::new(), "the copy and the thumbnail are removed");
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM file"), rows, "the file row rolled back with the link");
}

#[test]
fn bad_links_and_empty_files_are_refused_before_anything_is_stored() {
    let (dir, mut conn, actor) = demo();
    for (input, field) in [
        (
            upload(
                "a.txt",
                b"a".to_vec(),
                Some(LinkInput { entity_type_id: 14, entity_id: "x".into(), include_on_send: false }),
            ),
            "entity_type_id",
        ),
        (upload("a.txt", b"a".to_vec(), party("p-nobody")), "entity_id"),
        (upload("a.txt", Vec::new(), None), "file"),
        (upload("  ", b"a".to_vec(), None), "file"),
    ] {
        match files::file_add(&mut conn, dir.path(), &input, &actor) {
            Err(CommandError::Validation(v)) => assert_eq!(v[0].field, field),
            other => panic!("{other:?}"),
        }
    }
    assert!(stored(dir.path()).is_empty());
}

// ---------------------------------------------------------------------------------------------------------------
// OP-FILE-02, OP-FILE-03 (BR-FILE-03)
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn attaching_to_a_sales_transaction_also_links_its_customer_and_detach_keeps_the_file() {
    let (dir, mut conn, actor) = demo();
    let id = files::file_add(&mut conn, dir.path(), &upload("inv.pdf", b"%PDF".to_vec(), None), &actor).unwrap();
    let link = LinkInput { entity_type_id: 12, entity_id: "t-inv-1035".into(), include_on_send: true };
    files::file_attach(&mut conn, &id, &link, &actor).unwrap();
    let links: Vec<(i64, String, i64)> = conn
        .prepare("SELECT entity_type_id, entity_id, include_on_send FROM file_attachment WHERE attachment_id = ?1 ORDER BY entity_type_id DESC")
        .unwrap()
        .query_map([&id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(links, [(12, "t-inv-1035".to_string(), 1), (10, "p-hp".to_string(), 0)]);
    // No duplicate check: a second attach makes a second link.
    files::file_attach(
        &mut conn,
        &id,
        &LinkInput { entity_type_id: 11, entity_id: "a-mail-1048".into(), include_on_send: false },
        &actor,
    )
    .unwrap();
    let party_link: String = conn
        .query_row("SELECT id FROM file_attachment WHERE attachment_id = ?1 AND entity_type_id = 10", [&id], |r| {
            r.get(0)
        })
        .unwrap();
    let d = files::file_detach(&mut conn, &party_link, &actor).unwrap();
    assert_eq!((d.entity_type_id, d.entity_id.as_deref()), (Some(10), Some("p-hp")));
    assert_eq!(count(&conn, &format!("SELECT COUNT(*) FROM file_attachment WHERE attachment_id = '{id}'")), 2);
    assert_eq!(count(&conn, &format!("SELECT COUNT(*) FROM file WHERE id = '{id}'")), 1, "the file row stays");
    assert_eq!(stored(dir.path()).len(), 1, "the stored copy stays");
    assert!(matches!(files::file_detach(&mut conn, &party_link, &actor), Err(CommandError::NotFound)));
    assert!(matches!(files::file_attach(&mut conn, "nope", &link, &actor), Err(CommandError::NotFound)));
}

#[test]
fn the_backup_includes_a_new_file() {
    let (dir, mut conn, actor) = demo();
    files::file_add(&mut conn, dir.path(), &upload("p.png", png(30, 30), None), &actor).unwrap();
    let out = tempfile::tempdir().unwrap();
    let b = karbon_server::backup::create(&conn, &karbon_server::backup::files_dir(dir.path()), out.path()).unwrap();
    assert_eq!(b.manifest.files, 2, "the file and its thumbnail");
}

// ---------------------------------------------------------------------------------------------------------------
// Routes (UI-19)
// ---------------------------------------------------------------------------------------------------------------

async fn send(app: &axum::Router, req: Request<Body>) -> (StatusCode, HeaderMap, Vec<u8>) {
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let body = res.into_body().collect().await.unwrap().to_bytes().to_vec();
    (status, headers, body)
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

const BOUNDARY: &str = "----karbon-test-boundary";

/// A multipart body: text fields, then the file.
fn multipart(fields: &[(&str, &str)], file: Option<(&str, &[u8])>) -> Request<Body> {
    let mut body = Vec::new();
    for (k, v) in fields {
        body.extend_from_slice(
            format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n").as_bytes(),
        );
    }
    if let Some((name, bytes)) = file {
        body.extend_from_slice(
            format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n")
                .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    Request::post("/files")
        .header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={BOUNDARY}"))
        .header("HX-Request", "true")
        .body(Body::from(body))
        .unwrap()
}

fn app(dir: &Path, open: Option<OpenHook>) -> axum::Router {
    let conn = Connection::open(db::database_path(dir)).unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    let mut state = AppState::new(conn).with_data_dir(dir);
    if let Some(o) = open {
        state = state.with_open(o);
    }
    router(state)
}

#[tokio::test]
async fn the_party_panel_shows_the_seed_files_as_missing() {
    let (dir, conn, _) = demo();
    drop(conn);
    let app = app(dir.path(), None);
    let (status, _, html) = send(&app, Request::get("/parties/p-hp").body(Body::empty()).unwrap()).await;
    let html = text(&html);
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"id="party-files""#));
    for name in ["Phase 2 proposal v3.pdf", "Lighting schedule.xlsx", "Signed engagement letter.pdf"] {
        assert!(html.contains(name), "{name}");
    }
    assert_eq!(html.matches("File missing from the file store").count(), 4, "the seed rows point at no stored copy");
    let (status, _, body) = send(&app, Request::get("/files/f-001/content").body(Body::empty()).unwrap()).await;
    assert_eq!((status, text(&body).as_str()), (StatusCode::GONE, "File missing from the file store"));
    let (status, _, _) = send(&app, Request::post("/files/f-001/open").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::GONE);
}

#[tokio::test]
async fn upload_from_the_party_panel_then_open_serve_and_detach() {
    let (dir, conn, _) = demo();
    drop(conn);
    let opened: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
    let seen = opened.clone();
    let hook: OpenHook = Arc::new(move |p| {
        seen.lock().unwrap().push(p);
        Ok(())
    });
    let app = app(dir.path(), Some(hook));
    let image = png(640, 320);
    let (status, headers, html) = send(
        &app,
        multipart(
            &[("entity_type_id", "10"), ("entity_id", "p-hp"), ("tag", "site")],
            Some(("Reception photo.png", &image)),
        ),
    )
    .await;
    let html = text(&html);
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(headers["HX-Trigger"].to_str().unwrap().contains("File added"));
    assert!(html.contains(r#"id="party-files""#) && html.contains("Reception photo.png"));
    assert!(html.contains(r#"<span class="chip">5</span>"#), "the 4 seed files and the new one");

    let check = Connection::open(db::database_path(dir.path())).unwrap();
    let (id, link): (String, String) = check
        .query_row(
            "SELECT f.id, fa.id FROM file f JOIN file_attachment fa ON fa.attachment_id = f.id WHERE f.file_name = 'Reception photo.png'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(html.contains(&format!(r#"src="/files/{id}/thumbnail""#)));

    let (status, headers, body) =
        send(&app, Request::get(format!("/files/{id}/content")).body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "image/png");
    assert!(headers[header::CONTENT_DISPOSITION].to_str().unwrap().starts_with("inline;"));
    assert_eq!(body, image);
    let (status, headers, body) =
        send(&app, Request::get(format!("/files/{id}/thumbnail")).body(Body::empty()).unwrap()).await;
    assert_eq!((status, headers[header::CONTENT_TYPE].to_str().unwrap()), (StatusCode::OK, "image/png"));
    let t = image::load_from_memory(&body).unwrap();
    assert_eq!((t.width(), t.height()), (200, 100));

    let (status, _, _) = send(&app, Request::post(format!("/files/{id}/open")).body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let opened = opened.lock().unwrap().clone();
    assert_eq!(opened.len(), 1);
    let store = dir.path().join("files").canonicalize().unwrap();
    assert!(opened[0].starts_with(&store) && opened[0].ends_with(format!("{id}.png")), "{:?}", opened[0]);

    let (status, headers, html) = send(
        &app,
        Request::delete(format!("/attachments/{link}")).header("HX-Request", "true").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers["HX-Trigger"].to_str().unwrap().contains("File removed"));
    assert!(!text(&html).contains("Reception photo.png"));
    assert_eq!(count(&check, &format!("SELECT COUNT(*) FROM file WHERE id = '{id}'")), 1, "the file stays");

    let (_, _, html) = send(&app, Request::get("/files?q=reception").body(Body::empty()).unwrap()).await;
    let html = text(&html);
    assert!(html.contains(r#"href="/files" aria-current="page""#));
    assert!(
        html.contains("Reception photo.png")
            && html.contains("Reception — site photos.zip")
            && html.contains("2 files"),
        "{html}"
    );
}

#[tokio::test]
async fn open_never_hands_a_program_to_the_system() {
    let (dir, mut conn, actor) = demo();
    let script =
        files::file_add(&mut conn, dir.path(), &upload("run me.command", b"#!/bin/sh\necho hi".to_vec(), None), &actor)
            .unwrap();
    drop(conn);
    let calls: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
    let seen = calls.clone();
    let hook: OpenHook = Arc::new(move |p| {
        seen.lock().unwrap().push(p);
        Ok(())
    });
    let app = app(dir.path(), Some(hook));
    let (status, _, body) =
        send(&app, Request::post(format!("/files/{script}/open")).body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(text(&body).contains("documents and images only"));
    assert!(calls.lock().unwrap().is_empty(), "the hook is never called");
    let (_, headers, _) =
        send(&app, Request::get(format!("/files/{script}/content")).body(Body::empty()).unwrap()).await;
    assert!(headers[header::CONTENT_DISPOSITION].to_str().unwrap().starts_with("attachment;"), "it only downloads");
}

#[tokio::test]
async fn open_answers_501_without_the_desktop_shell() {
    let (dir, mut conn, actor) = demo();
    let id = files::file_add(&mut conn, dir.path(), &upload("n.txt", b"note".to_vec(), None), &actor).unwrap();
    drop(conn);
    let app = app(dir.path(), None);
    let (status, _, _) = send(&app, Request::post(format!("/files/{id}/open")).body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    let (status, headers, body) =
        send(&app, Request::get(format!("/files/{id}/content")).body(Body::empty()).unwrap()).await;
    assert_eq!((status, body.as_slice()), (StatusCode::OK, b"note".as_slice()));
    assert_eq!(headers[header::CONTENT_TYPE], "text/plain");
    assert!(
        headers[header::CONTENT_SECURITY_POLICY].to_str().unwrap().starts_with("sandbox"),
        "a stored file runs no script"
    );
    let (status, _, _) = send(&app, Request::get(format!("/files/{id}/thumbnail")).body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "a text file has no thumbnail");
}

#[tokio::test]
async fn an_unknown_type_downloads_and_a_path_outside_the_store_is_never_served() {
    let (dir, mut conn, actor) = demo();
    let id = files::file_add(
        &mut conn,
        dir.path(),
        &upload("page.html", b"<script>alert(1)</script>".to_vec(), None),
        &actor,
    )
    .unwrap();
    std::fs::write(dir.path().join("secret.txt"), "secret").unwrap();
    conn.execute("UPDATE file SET container_path = 'files/../secret.txt' WHERE id = 'f-001'", []).unwrap();
    drop(conn);
    let app = app(dir.path(), None);
    let (status, headers, _) =
        send(&app, Request::get(format!("/files/{id}/content")).body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "application/octet-stream");
    assert!(headers[header::CONTENT_DISPOSITION].to_str().unwrap().starts_with("attachment;"));
    let (status, _, body) = send(&app, Request::get("/files/f-001/content").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::GONE);
    assert!(!text(&body).contains("secret"));
    for uri in ["/files/..%2F..%2Fsecret.txt/content", "/files/..%2Fsecret.txt/open"] {
        let req = if uri.ends_with("open") { Request::post(uri) } else { Request::get(uri) };
        let (status, _, body) = send(&app, req.body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        assert!(!text(&body).contains("secret"));
    }
}

#[tokio::test]
async fn upload_errors_are_422_and_store_nothing() {
    let (dir, conn, _) = demo();
    drop(conn);
    let app = app(dir.path(), None);
    let big = vec![1u8; files::FIRST_REFUSED_BYTES as usize];
    let (status, _, html) =
        send(&app, multipart(&[("entity_type_id", "10"), ("entity_id", "p-hp")], Some(("big.bin", &big)))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(text(&html).contains("larger than 20 MB") && text(&html).contains(r#"id="party-files""#));
    let (status, _, _) = send(&app, multipart(&[("colour", "red")], Some(("a.txt", b"a")))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "an unknown field");
    let (status, _, html) = send(&app, multipart(&[], None)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(text(&html).contains("Choose a file."));
    assert!(stored(dir.path()).is_empty());
    // From the file list (no link): created, and the list reloads.
    let (status, headers, _) = send(&app, multipart(&[("note", "loose")], Some(("loose.txt", b"x")))).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(headers["HX-Redirect"], "/files");
}
