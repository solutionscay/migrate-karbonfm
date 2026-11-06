//! UI-19 Files: the file list (layouts 142/185), the party Files panel (design/customer.html), upload (OP-FILE-01),
//! attach and detach (OP-FILE-02/03), and serving or opening a stored file. Commands are in `crate::files`.
//!
//! The page never gets a path (AP-01): it names a file id; the server resolves the path inside the file store, and
//! "Open" asks the desktop shell to open it with the system app (`AppState::with_open`). Without the shell (the dev
//! server in a browser) `POST /files/{id}/open` answers 501 and the page opens `GET /files/{id}/content` instead.

use super::current_user::CurrentUser;
use super::party_forms::toast;
use super::Nav;
use crate::admin::{CommandError, FieldError};
use crate::files::{self, FileInput, FileRow, LinkInput};
use crate::invoice::display_date;
use crate::web::{is_htmx, render, urlencode, AppError};
use crate::AppState;
use askama::Template;
use axum::body::Body;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::Form;
use rusqlite::Connection;
use serde::Deserialize;

/// The text of a file whose stored copy is not in the file store (the small demo's seed rows, a damaged restore).
pub const MISSING: &str = "File missing from the file store";

fn lock(state: &AppState) -> Result<std::sync::MutexGuard<'_, Connection>, AppError> {
    state.db.lock().map_err(|e| AppError::Internal(e.to_string()))
}

fn command_error(op: &str, e: CommandError) -> AppError {
    match e {
        CommandError::NotFound => AppError::NotFound,
        e => AppError::Internal(format!("{op}: {e}")),
    }
}

/// A file row as the templates show it.
pub struct FileView {
    pub id: String,
    pub attachment_id: String,
    pub name: String,
    pub badge: String,
    pub badge_color: &'static str,
    pub size: String,
    pub date: String,
    pub category: String,
    pub tag: String,
    pub note: String,
    pub thumbnail: bool,
    pub present: bool,
    pub attached_to: String,
}

impl From<FileRow> for FileView {
    fn from(f: FileRow) -> Self {
        let ext = f.extension.to_ascii_uppercase();
        let badge_color = match ext.as_str() {
            "PDF" => "bg-error/10 text-error",
            "XLS" | "XLSX" | "CSV" => "bg-success/10 text-success",
            _ if f.category == "Image" => "bg-info/10 text-info",
            _ => "bg-base-200 muted",
        };
        Self {
            id: f.id,
            attachment_id: f.attachment_id,
            name: f.file_name,
            badge: if ext.is_empty() { "FILE".into() } else { ext.chars().take(4).collect() },
            badge_color,
            size: files::display_size(f.size_kb),
            date: display_date(f.created.get(..10).unwrap_or("")),
            category: f.category,
            tag: f.tag,
            note: f.note,
            thumbnail: f.has_thumbnail,
            present: f.present,
            attached_to: f.attached_to.join(", "),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The party Files panel
// ---------------------------------------------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "files/panel.html")]
pub struct FilesPanel {
    pub party_id: String,
    pub rows: Vec<FileView>,
    pub error: Option<String>,
    /// The upload needs the file store of a data folder (none in an in-memory dev server).
    pub can_add: bool,
}

impl FilesPanel {
    pub fn load(state: &AppState, conn: &Connection, party_id: &str) -> Result<Self, AppError> {
        let rows = files::party_files(conn, state.data_dir.as_deref(), party_id)?;
        Ok(Self {
            party_id: party_id.to_string(),
            rows: rows.into_iter().map(FileView::from).collect(),
            error: None,
            can_add: state.data_dir.is_some(),
        })
    }

    pub fn upload_href(&self) -> &'static str {
        "/files"
    }
}

// ---------------------------------------------------------------------------------------------------------------
// GET /files
// ---------------------------------------------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ListQuery {
    pub q: String,
    pub page: i64,
}

#[derive(Template)]
#[template(path = "files/table.html")]
struct FileTable {
    q: String,
    rows: Vec<FileView>,
    matched: i64,
    page: i64,
    pages: i64,
}

impl FileTable {
    fn href(&self, page: i64) -> String {
        format!("/files?q={}&page={page}", urlencode(&self.q))
    }
}

#[derive(Template)]
#[template(path = "files/list.html")]
struct ListPage {
    nav: Nav,
    demo: bool,
    table: FileTable,
}

/// `GET /files` (layouts 142/185): every file, newest first, with a thumbnail for images and what it is attached to.
pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Html<String>, AppError> {
    let demo = super::demo(&state)?;
    let conn = lock(&state)?;
    let p = files::file_list(&conn, state.data_dir.as_deref(), &query.q, query.page)?;
    let table = FileTable {
        q: query.q.clone(),
        rows: p.rows.into_iter().map(FileView::from).collect(),
        matched: p.matched,
        page: p.page,
        pages: p.pages,
    };
    if is_htmx(&headers) {
        return render(&table);
    }
    render(&ListPage { nav: Nav::Files, demo, table })
}

// ---------------------------------------------------------------------------------------------------------------
// POST /files (OP-FILE-01)
// ---------------------------------------------------------------------------------------------------------------

/// The upload's form, read from the multipart body.
#[derive(Default)]
struct Upload {
    input: FileInput,
    entity_type_id: String,
    entity_id: String,
    include_on_send: bool,
    too_large: bool,
}

fn bad_request(message: impl Into<String>) -> Response {
    (StatusCode::UNPROCESSABLE_ENTITY, message.into()).into_response()
}

/// Reads the multipart form: `file`, `note`, `tag`, `entity_type_id`, `entity_id`, `include_on_send`. The file is
/// read in chunks and reading stops past the limit (BR-FILE-01), so an oversized upload is a 422, never buffered whole.
async fn read_upload(mut multipart: Multipart) -> Result<Upload, Response> {
    let mut u = Upload::default();
    while let Some(mut field) = multipart.next_field().await.map_err(|e| bad_request(e.body_text()))? {
        let name = field.name().unwrap_or_default().to_string();
        match name.as_str() {
            "file" => {
                u.input.file_name = field.file_name().unwrap_or_default().to_string();
                while let Some(chunk) = field.chunk().await.map_err(|e| bad_request(e.body_text()))? {
                    if u.input.bytes.len() as u64 + chunk.len() as u64 >= files::FIRST_REFUSED_BYTES {
                        u.too_large = true;
                        // Keep reading the field (and drop it), so the rest of the form arrives.
                        u.input.bytes.clear();
                        while field.chunk().await.map_err(|e| bad_request(e.body_text()))?.is_some() {}
                        break;
                    }
                    u.input.bytes.extend_from_slice(&chunk);
                }
            }
            "note" | "tag" | "entity_type_id" | "entity_id" | "include_on_send" => {
                let text = field.text().await.map_err(|e| bad_request(e.body_text()))?;
                match name.as_str() {
                    "note" => u.input.note = text,
                    "tag" => u.input.tag = text,
                    "entity_type_id" => u.entity_type_id = text,
                    "entity_id" => u.entity_id = text,
                    _ => u.include_on_send = !text.is_empty(),
                }
            }
            other => return Err(bad_request(format!("unknown field `{other}`"))),
        }
    }
    Ok(u)
}

/// `POST /files` (`file_add`): adds the file and its link in one step. From the party panel (entity type 10) the
/// answer is the panel; from the file list it reloads the list. Field errors (422) come back in the same place.
pub async fn upload(
    State(state): State<AppState>,
    CurrentUser(actor): CurrentUser,
    multipart: Multipart,
) -> Result<Response, AppError> {
    let mut u = match read_upload(multipart).await {
        Ok(u) => u,
        Err(r) => return Ok(r),
    };
    let link = match (u.entity_type_id.trim(), u.entity_id.trim()) {
        ("", "") => None,
        (t, id) => match t.parse::<i64>() {
            Ok(t) => {
                Some(LinkInput { entity_type_id: t, entity_id: id.to_string(), include_on_send: u.include_on_send })
            }
            Err(_) => return Ok(bad_request("entity_type_id")),
        },
    };
    let party = link.as_ref().filter(|l| l.entity_type_id == 10).map(|l| l.entity_id.clone());
    u.input.link = link;
    let Some(data_dir) = state.data_dir.clone() else {
        return Ok(bad_request("This install has no file store (no data folder)."));
    };
    let result = if u.too_large {
        Err(CommandError::Validation(vec![FieldError {
            field: "file",
            message: "The file is larger than 20 MB (20,480 KB), the most a file can be.".into(),
        }]))
    } else {
        let mut conn = lock(&state)?;
        files::file_add(&mut conn, &data_dir, &u.input, &actor)
    };
    let error = match result {
        Ok(_) => None,
        Err(CommandError::Validation(errors)) => {
            Some(errors.into_iter().map(|e| e.message).collect::<Vec<_>>().join(" "))
        }
        Err(e) => return Err(command_error("file_add", e)),
    };
    let status = if error.is_some() { StatusCode::UNPROCESSABLE_ENTITY } else { StatusCode::OK };
    let conn = lock(&state)?;
    match party {
        Some(party_id) => {
            let mut panel = FilesPanel::load(&state, &conn, &party_id)?;
            panel.error = error.clone();
            let html = panel.render()?;
            if error.is_some() {
                Ok((status, Html(html)).into_response())
            } else {
                Ok((status, [("HX-Trigger", toast("File added", false))], Html(html)).into_response())
            }
        }
        None => match error {
            Some(e) => Ok((status, Html(UploadError { message: e }.render()?)).into_response()),
            None => {
                Ok((StatusCode::CREATED, [("HX-Redirect", "/files")], [("HX-Trigger", toast("File added", false))])
                    .into_response())
            }
        },
    }
}

#[derive(Template)]
#[template(
    source = r#"<div role="alert" class="alert alert-error alert-soft text-[13px] py-2">{{ message }}</div>"#,
    ext = "html"
)]
struct UploadError {
    message: String,
}

// ---------------------------------------------------------------------------------------------------------------
// GET /files/{id}/content, /thumbnail; POST /files/{id}/open
// ---------------------------------------------------------------------------------------------------------------

/// Types the window may show in place; anything else is a download.
fn inline(content_type: &str) -> bool {
    matches!(content_type, "application/pdf" | "text/plain" | "image/png" | "image/jpeg" | "image/gif")
}

fn file_response(path: &std::path::Path, content_type: &str, name: Option<&str>) -> Result<Response, AppError> {
    let bytes = std::fs::read(path).map_err(|e| AppError::Internal(format!("read stored file: {e}")))?;
    let mut res = Response::new(Body::from(bytes));
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(content_type).unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    if let Some(name) = name {
        let disposition = if inline(content_type) { "inline" } else { "attachment" };
        let safe: String = name
            .chars()
            .map(|c| if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' { c } else { '_' })
            .collect();
        let value = format!("{disposition}; filename=\"{safe}\"; filename*=UTF-8''{}", urlencode(name));
        h.insert(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&value).map_err(|e| AppError::Internal(e.to_string()))?,
        );
    }
    // A stored file runs no script in the app's origin, whatever it holds.
    if content_type != "application/pdf" {
        h.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("sandbox; default-src 'none'; img-src 'self'; style-src 'unsafe-inline'"),
        );
    }
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(res)
}

/// Open refuses a type that could run a program (`files::openable`): 409 with this text. It names the type, not a
/// category, since 617's category calls every non-image a Document (B6).
pub fn not_openable(ext: &str) -> String {
    let what = if ext.is_empty() { "Files with no extension".to_owned() } else { format!("“.{ext}” files") };
    format!("{what} could run a program, so KarbonFM does not open them.")
}

/// 410 Gone: the row is there, its stored copy is not.
fn missing() -> Response {
    (StatusCode::GONE, MISSING).into_response()
}

/// `GET /files/{id}/content`: the stored copy, with its type (image/jpg is answered as image/jpeg); 410 when the copy
/// is gone.
pub async fn content(State(state): State<AppState>, Path(id): Path<String>) -> Result<Response, AppError> {
    let f = {
        let conn = lock(&state)?;
        files::stored_file(&conn, state.data_dir.as_deref(), &id).map_err(|e| command_error("stored_file", e))?
    };
    match &f.path {
        Some(p) => file_response(p, files::serve_type(&f.content_type), Some(&f.file_name)),
        None => Ok(missing()),
    }
}

/// `GET /files/{id}/thumbnail`: the 200x200 PNG of an image; 204 for a file that has none (the page shows a badge
/// then and does not ask).
pub async fn thumbnail(State(state): State<AppState>, Path(id): Path<String>) -> Result<Response, AppError> {
    let f = {
        let conn = lock(&state)?;
        files::stored_file(&conn, state.data_dir.as_deref(), &id).map_err(|e| command_error("stored_file", e))?
    };
    match &f.thumbnail {
        Some(p) => file_response(p, "image/png", None),
        None => Ok(StatusCode::NO_CONTENT.into_response()),
    }
}

/// `POST /files/{id}/open`: asks the desktop shell to open the stored copy with the system app (204). 501 without
/// the shell (the page then opens the content route); 410 with "File missing from the file store" when the copy is
/// gone; 409 for a type that is not a document or an image (it could run a program).
pub async fn open(State(state): State<AppState>, Path(id): Path<String>) -> Result<Response, AppError> {
    let f = {
        let conn = lock(&state)?;
        files::stored_file(&conn, state.data_dir.as_deref(), &id).map_err(|e| command_error("stored_file", e))?
    };
    let Some(path) = f.path else { return Ok(missing()) };
    // The type the system goes by is the stored copy's extension.
    let ext = path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
    if !files::openable(&ext) {
        return Ok((StatusCode::CONFLICT, not_openable(&ext.to_ascii_lowercase())).into_response());
    }
    let Some(hook) = state.open.clone() else { return Ok(StatusCode::NOT_IMPLEMENTED.into_response()) };
    tokio::task::spawn_blocking(move || hook(path))
        .await
        .map_err(|e| AppError::Internal(format!("open task: {e}")))?
        .map_err(AppError::Internal)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

// ---------------------------------------------------------------------------------------------------------------
// POST /attachments, DELETE /attachments/{id} (OP-FILE-02, OP-FILE-03)
// ---------------------------------------------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct AttachForm {
    pub file_id: String,
    pub entity_type_id: String,
    pub entity_id: String,
    pub include_on_send: String,
}

/// `POST /attachments` (`file_attach`): links an existing file to a record (a party, activity, sales transaction or
/// opportunity; the last two also link the record's party).
pub async fn attach(
    State(state): State<AppState>,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<AttachForm>,
) -> Result<Response, AppError> {
    let Ok(entity_type_id) = form.entity_type_id.trim().parse::<i64>() else {
        return Ok(bad_request("Choose what to attach the file to."));
    };
    let link = LinkInput {
        entity_type_id,
        entity_id: form.entity_id.trim().to_string(),
        include_on_send: !form.include_on_send.is_empty(),
    };
    let mut conn = lock(&state)?;
    match files::file_attach(&mut conn, form.file_id.trim(), &link, &actor) {
        Ok(_) => Ok((StatusCode::CREATED, [("HX-Trigger", toast("File attached", false))]).into_response()),
        Err(CommandError::Validation(errors)) => {
            let message = errors.into_iter().map(|e| e.message).collect::<Vec<_>>().join(" ");
            Ok((StatusCode::UNPROCESSABLE_ENTITY, Html(UploadError { message }.render()?)).into_response())
        }
        Err(e) => Err(command_error("file_attach", e)),
    }
}

/// `DELETE /attachments/{id}` (`file_detach`): removes only the link, with no confirmation (UI-19); the file stays.
/// A party's link answers with that party's panel.
pub async fn detach(
    State(state): State<AppState>,
    Path(id): Path<String>,
    CurrentUser(actor): CurrentUser,
) -> Result<Response, AppError> {
    let mut conn = lock(&state)?;
    let d = files::file_detach(&mut conn, &id, &actor).map_err(|e| command_error("file_detach", e))?;
    let trigger = [("HX-Trigger", toast("File removed from this record", false))];
    match (d.entity_type_id, d.entity_id) {
        (Some(10), Some(party)) => {
            let html = FilesPanel::load(&state, &conn, &party)?.render()?;
            Ok((trigger, Html(html)).into_response())
        }
        _ => Ok((trigger, StatusCode::OK).into_response()),
    }
}
