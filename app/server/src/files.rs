//! FILE commands (UI-19, the party Files panel): add a file (OP-FILE-01 `file_add`), attach it (OP-FILE-02
//! `file_attach`) and detach it (OP-FILE-03 `file_detach`). BR-FILE-01..03.
//!
//! A file is copied into the file store of the data folder, `files/<YYYY>/<file id>.<ext>`, and the row keeps that
//! path relative to the data folder (D-17; the import's layout, `import::Loader::copy_container`). The upload itself
//! is never touched. The database write is one `BEGIN IMMEDIATE` transaction; a failed write removes the copied files,
//! so no file is left behind without a row.
//!
//! file and file_attachment have `created_by` / `modified_by` (the account name, table 195 #3/#5, 196 #3/#5) instead
//! of the stamp user columns, so the commands set them here (`audit::stamp_row` sets only the timestamps).

use crate::admin::{CommandError, FieldError};
use crate::audit::{self, Actor, Stamp};
use crate::backup::FILES_DIR;
use crate::ids::new_id;
use crate::reference::EntityType;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::path::{Path, PathBuf};

/// 617 s107-108: "max allowed 20MB", in KB.
pub const MAX_KB: f64 = 20480.0;

/// The size the source stores: KB = round(bytes / 1000, 2) (617 s104). In hundredths of a KB, rounded half up.
pub fn size_hundredths_kb(bytes: u64) -> u64 {
    (bytes + 5) / 10
}

/// BR-FILE-01: refused when the size in KB is over 20480 (617 s108). 20,480,004 bytes are 20480.00 KB (accepted);
/// 20,480,005 bytes are 20480.01 KB (refused).
pub fn too_large(bytes: u64) -> bool {
    size_hundredths_kb(bytes) > 2_048_000
}

/// The first byte count that is refused.
pub const FIRST_REFUSED_BYTES: u64 = 20_480_005;

/// The text after the last "." of the file name (617 s114); a name with no "." has none.
pub fn extension(file_name: &str) -> String {
    match file_name.rsplit_once('.') {
        Some((_, ext)) if !ext.is_empty() && !ext.contains(['/', '\\']) => ext.to_string(),
        _ => String::new(),
    }
}

/// The content type 617 s117 sets from the extension, exactly (jpg is "image/jpg"); anything else is empty.
/// FileMaker compares text without case, so "PDF" is "pdf".
pub fn content_type(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "gif" => "image/gif",
        "jpeg" => "image/jpeg",
        "jpg" => "image/jpg",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "txt" => "text/plain",
        "rtf" => "text/rtf",
        _ => "",
    }
}

/// The type a stored file is served with: "image/jpg" is not a registered type, so it is answered as image/jpeg; an
/// empty type is application/octet-stream (the browser then downloads instead of rendering it).
pub fn serve_type(stored: &str) -> &str {
    match stored {
        "image/jpg" => "image/jpeg",
        "" => "application/octet-stream",
        t => t,
    }
}

/// 617 s120: Image for png, jpg, gif, bmp, tif, tiff, **and jpeg** (617 leaves jpeg out: a bug, built as intended,
/// plan F3); anything else is Document.
pub fn category(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "tif" | "tiff" => "Image",
        _ => "Document",
    }
}

/// The types "Open" hands to the system app: documents, images and zip archives (the system only unpacks a zip and
/// runs nothing; Admiral's ruling, 2026-11-01, B6). Anything else (a script, an app, an installer, a disk image) could
/// run a program when the system opens it, so it is never opened from KarbonFM; the page says so.
pub fn openable(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "pdf"
            | "txt"
            | "rtf"
            | "csv"
            | "doc"
            | "docx"
            | "xls"
            | "xlsx"
            | "ppt"
            | "pptx"
            | "odt"
            | "ods"
            | "odp"
            | "pages"
            | "numbers"
            | "key"
            | "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "bmp"
            | "tif"
            | "tiff"
            | "heic"
            | "webp"
            | "zip"
    )
}

/// The entity types a file attaches to (plan F7): party, activity, sales transaction, opportunity.
pub const ATTACHABLE: &[EntityType] =
    &[EntityType::Party, EntityType::Activity, EntityType::SalesTransaction, EntityType::Opportunity];

/// A 200x200 fit of the image as PNG (617 s123 `GetThumbnail ( $Container ; 200 ; 200 )`). `None` when the bytes are
/// not an image this build decodes; the file is then kept without a thumbnail.
pub fn thumbnail(bytes: &[u8]) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?;
    let thumb = img.thumbnail(200, 200);
    let mut out = std::io::Cursor::new(Vec::new());
    thumb.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// The data folder's file store (`backup::files_dir`).
fn store_root(data_dir: &Path) -> PathBuf {
    crate::backup::files_dir(data_dir)
}

/// The full path of a stored file, only when it is inside the file store (no `..`, no absolute path, no link out of
/// it) and exists. An imported row could hold any text; the page never gets a path (AP-01).
pub fn resolve(data_dir: &Path, stored: &str) -> Option<PathBuf> {
    let rel = Path::new(stored);
    if rel.is_absolute() || stored.contains('\\') || stored.split('/').any(|p| p == ".." || p.is_empty()) {
        return None;
    }
    let first = stored.split('/').next()?;
    if first != FILES_DIR {
        return None;
    }
    let full = data_dir.join(rel).canonicalize().ok()?;
    let root = store_root(data_dir).canonicalize().ok()?;
    (full.starts_with(&root) && full.is_file()).then_some(full)
}

// ---------------------------------------------------------------------------------------------------------------
// OP-FILE-01 file_add (BR-FILE-01, BR-FILE-02)
// ---------------------------------------------------------------------------------------------------------------

/// An upload: the bytes, the name the user's file had, and the optional note, tag and link (617 takes the
/// FileAttachment array, s5-17; the page sends one link).
#[derive(Debug, Clone, Default)]
pub struct FileInput {
    pub file_name: String,
    pub bytes: Vec<u8>,
    pub note: String,
    pub tag: String,
    pub link: Option<LinkInput>,
}

/// Where a file is attached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkInput {
    pub entity_type_id: i64,
    pub entity_id: String,
    pub include_on_send: bool,
}

/// The year folder: the year of the stamp (`files/<YYYY>/`).
fn year_of(stamp: &Stamp) -> &str {
    &stamp.at[..4]
}

fn validation(field: &'static str, message: impl Into<String>) -> CommandError {
    CommandError::Validation(vec![FieldError { field, message: message.into() }])
}

/// Adds a file and its link in one transaction (plan F5, F6). The bytes are copied to the store first; when the
/// database write fails the copies are removed. Returns the new file id.
pub fn file_add(
    conn: &mut Connection,
    data_dir: &Path,
    input: &FileInput,
    actor: &Actor,
) -> Result<String, CommandError> {
    let name = input.file_name.trim();
    if name.is_empty() || input.bytes.is_empty() {
        return Err(validation("file", "Choose a file."));
    }
    if too_large(input.bytes.len() as u64) {
        return Err(validation("file", "The file is larger than 20 MB (20,480 KB), the most a file can be."));
    }
    // The name the user's computer gave, without any folder part.
    let name = name.rsplit(['/', '\\']).next().unwrap_or(name).to_string();
    if let Some(link) = &input.link {
        check_entity(conn, link)?;
    }
    let ext = extension(&name);
    let kind = category(&ext);
    let id = new_id();
    let stamp = Stamp::now(actor);
    let dir = format!("{FILES_DIR}/{}", year_of(&stamp));
    let stored = if ext.is_empty() { format!("{dir}/{id}") } else { format!("{dir}/{id}.{ext}") };
    let mut written: Vec<PathBuf> = Vec::new();
    let result = (|| -> Result<String, CommandError> {
        let full = data_dir.join(&stored);
        std::fs::create_dir_all(full.parent().expect("a folder")).map_err(io)?;
        std::fs::write(&full, &input.bytes).map_err(io)?;
        written.push(full);
        let thumb_path = if kind == "Image" {
            thumbnail(&input.bytes).and_then(|png| {
                let rel = format!("{dir}/{id}-thumbnail.png");
                let full = data_dir.join(&rel);
                std::fs::write(&full, png).ok()?;
                written.push(full);
                Some(rel)
            })
        } else {
            None
        };
        let kb = size_hundredths_kb(input.bytes.len() as u64) as f64 / 100.0;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO file (id, file_name, extension, content_type, category, file_size, note, tag, container_path, \
             thumbnail_path, created_by, modified_by) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
            params![
                id,
                name,
                non_empty(&ext),
                non_empty(content_type(&ext)),
                kind,
                kb,
                non_empty(input.note.trim()),
                non_empty(input.tag.trim()),
                stored,
                thumb_path,
                stamp.actor.account_name,
            ],
        )?;
        audit::stamp_created(&tx, "file", &id, &stamp)?;
        if let Some(link) = &input.link {
            attach_in(&tx, &id, link, &stamp)?;
        }
        tx.commit()?;
        Ok(id.clone())
    })();
    if result.is_err() {
        for f in &written {
            let _ = std::fs::remove_file(f);
        }
    }
    result
}

fn io(e: std::io::Error) -> CommandError {
    CommandError::Storage(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}

fn non_empty(s: &str) -> Option<&str> {
    (!s.is_empty()).then_some(s)
}

/// The link's entity type is attachable and its record exists.
fn check_entity(conn: &Connection, link: &LinkInput) -> Result<(), CommandError> {
    let Some(kind) = EntityType::from_id(link.entity_type_id).filter(|k| ATTACHABLE.contains(k)) else {
        return Err(validation(
            "entity_type_id",
            "A file attaches to a party, activity, sales transaction or opportunity.",
        ));
    };
    let table = match kind {
        EntityType::Party => "party",
        EntityType::Activity => "activity",
        EntityType::SalesTransaction => "sales_transaction",
        _ => "opportunity",
    };
    let exists: bool =
        conn.query_row(&format!("SELECT EXISTS (SELECT 1 FROM {table} WHERE id = ?1)"), [&link.entity_id], |r| {
            r.get(0)
        })?;
    if exists {
        Ok(())
    } else {
        Err(validation("entity_id", "That record does not exist."))
    }
}

/// The party a sales transaction or an opportunity belongs to: attaching to one also links the file to its party
/// (plan F7; kontacts 644 s6, 701 s27, 622/634 s6-7).
fn owner_party(conn: &Connection, link: &LinkInput) -> rusqlite::Result<Option<String>> {
    let sql = match EntityType::from_id(link.entity_type_id) {
        Some(EntityType::SalesTransaction) => {
            "SELECT pr.party_id FROM sales_transaction st JOIN party_role pr ON pr.id = st.id_customer WHERE st.id = ?1"
        }
        Some(EntityType::Opportunity) => "SELECT party_id FROM opportunity WHERE id = ?1",
        _ => return Ok(None),
    };
    Ok(conn.query_row(sql, [&link.entity_id], |r| r.get::<_, Option<String>>(0)).optional()?.flatten())
}

/// Writes the link row (616 s97-101) and, for a sales transaction or an opportunity, the party's link. There is no
/// duplicate check (617 s132). Returns the first link's id.
fn attach_in(conn: &Connection, file_id: &str, link: &LinkInput, stamp: &Stamp) -> Result<String, CommandError> {
    let first = insert_link(conn, file_id, link, stamp)?;
    if let Some(party) = owner_party(conn, link)? {
        let party_link = LinkInput { entity_type_id: EntityType::Party.id(), entity_id: party, include_on_send: false };
        insert_link(conn, file_id, &party_link, stamp)?;
    }
    Ok(first)
}

fn insert_link(conn: &Connection, file_id: &str, link: &LinkInput, stamp: &Stamp) -> rusqlite::Result<String> {
    let id = new_id();
    // entity_type = EntityType::Name (616 s75); include_on_send 0 unless given (table 196 #9).
    let name = EntityType::from_id(link.entity_type_id).map(EntityType::name);
    conn.execute(
        "INSERT INTO file_attachment (id, attachment_id, entity_type_id, entity_type, entity_id, include_on_send, \
         created_by, modified_by) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
        params![
            id,
            file_id,
            link.entity_type_id,
            name,
            link.entity_id,
            i64::from(link.include_on_send),
            stamp.actor.account_name
        ],
    )?;
    audit::stamp_created(conn, "file_attachment", &id, stamp)?;
    Ok(id)
}

// ---------------------------------------------------------------------------------------------------------------
// OP-FILE-02 file_attach, OP-FILE-03 file_detach (BR-FILE-03)
// ---------------------------------------------------------------------------------------------------------------

/// Attaches an existing file to a record. Returns the link id.
pub fn file_attach(
    conn: &mut Connection,
    file_id: &str,
    link: &LinkInput,
    actor: &Actor,
) -> Result<String, CommandError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let exists: bool = tx.query_row("SELECT EXISTS (SELECT 1 FROM file WHERE id = ?1)", [file_id], |r| r.get(0))?;
    if !exists {
        return Err(CommandError::NotFound);
    }
    check_entity(&tx, link)?;
    let id = attach_in(&tx, file_id, link, &Stamp::now(actor))?;
    tx.commit()?;
    Ok(id)
}

/// What a detach removed: the link's entity, so the page can refresh that record's panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detached {
    pub entity_type_id: Option<i64>,
    pub entity_id: Option<String>,
}

/// Removes one link (612 s92). The file row and the stored file stay (UI-19: no confirmation, it can be attached
/// again). 612 s83 can create a blank File row: a bug, not built.
pub fn file_detach(conn: &mut Connection, attachment_id: &str, _actor: &Actor) -> Result<Detached, CommandError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let d = tx
        .query_row("SELECT entity_type_id, entity_id FROM file_attachment WHERE id = ?1", [attachment_id], |r| {
            Ok(Detached { entity_type_id: r.get(0)?, entity_id: r.get(1)? })
        })
        .optional()?
        .ok_or(CommandError::NotFound)?;
    tx.execute("DELETE FROM file_attachment WHERE id = ?1", [attachment_id])?;
    tx.commit()?;
    Ok(d)
}

// ---------------------------------------------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------------------------------------------

/// One file as the lists show it.
#[derive(Debug, Clone, PartialEq)]
pub struct FileRow {
    pub id: String,
    /// The link this row came through (the party panel); empty in the file list.
    pub attachment_id: String,
    pub file_name: String,
    pub extension: String,
    pub category: String,
    pub size_kb: Option<f64>,
    pub created: String,
    pub note: String,
    pub tag: String,
    pub has_thumbnail: bool,
    /// The stored copy is in the file store.
    pub present: bool,
    /// What the file is attached to: "Halden & Pryce Architects", "INV-1048", ...
    pub attached_to: Vec<String>,
}

const ROW_SQL: &str = "SELECT f.id, COALESCE(f.file_name, ''), COALESCE(f.extension, ''), COALESCE(f.category, ''), \
     f.file_size, COALESCE(f.creation_timestamp, ''), COALESCE(f.note, ''), COALESCE(f.tag, ''), f.container_path, \
     f.thumbnail_path";

fn row_from(r: &rusqlite::Row, data_dir: Option<&Path>) -> rusqlite::Result<FileRow> {
    let stored: Option<String> = r.get(8)?;
    let thumb: Option<String> = r.get(9)?;
    let present = match (data_dir, &stored) {
        (Some(d), Some(s)) => resolve(d, s).is_some(),
        _ => false,
    };
    let has_thumbnail = match (data_dir, &thumb) {
        (Some(d), Some(t)) => resolve(d, t).is_some(),
        _ => false,
    };
    let size_kb = match r.get::<_, rusqlite::types::Value>(4)? {
        rusqlite::types::Value::Integer(i) => Some(i as f64),
        rusqlite::types::Value::Real(f) => Some(f),
        rusqlite::types::Value::Text(t) => t.trim().parse().ok(),
        _ => None,
    };
    Ok(FileRow {
        id: r.get(0)?,
        attachment_id: String::new(),
        file_name: r.get(1)?,
        extension: r.get(2)?,
        category: r.get(3)?,
        size_kb,
        created: r.get(5)?,
        note: r.get(6)?,
        tag: r.get(7)?,
        has_thumbnail,
        present,
        attached_to: Vec::new(),
    })
}

/// The files attached to a party, newest first (the party Files panel, customer.html).
pub fn party_files(conn: &Connection, data_dir: Option<&Path>, party_id: &str) -> rusqlite::Result<Vec<FileRow>> {
    entity_files(conn, data_dir, EntityType::Party.id(), party_id)
}

/// The files linked to one record, newest first (a party, or an opportunity's panel on layout 156).
pub fn entity_files(
    conn: &Connection,
    data_dir: Option<&Path>,
    entity_type_id: i64,
    entity_id: &str,
) -> rusqlite::Result<Vec<FileRow>> {
    let sql = format!(
        "{ROW_SQL}, fa.id FROM file_attachment fa JOIN file f ON f.id = fa.attachment_id \
         WHERE fa.entity_type_id = ?1 AND fa.entity_id = ?2 ORDER BY f.creation_timestamp DESC, f.id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params![entity_type_id, entity_id], |r| {
        let mut row = row_from(r, data_dir)?;
        row.attachment_id = r.get(10)?;
        Ok(row)
    })?;
    rows.collect()
}

/// One page of every file, newest first, with what each is attached to (`GET /files`, layouts 142/185).
pub struct FilePage {
    pub rows: Vec<FileRow>,
    pub matched: i64,
    pub page: i64,
    pub pages: i64,
}

pub const PAGE_SIZE: i64 = 50;

pub fn file_list(conn: &Connection, data_dir: Option<&Path>, query: &str, page: i64) -> rusqlite::Result<FilePage> {
    let like = format!("%{}%", query.trim().replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
    let filter =
        "(?1 = '%%' OR f.file_name LIKE ?1 ESCAPE '\\' OR f.tag LIKE ?1 ESCAPE '\\' OR f.note LIKE ?1 ESCAPE '\\')";
    let matched: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM file f WHERE {filter}"), [&like], |r| r.get(0))?;
    let pages = ((matched + PAGE_SIZE - 1) / PAGE_SIZE).max(1);
    let page = page.clamp(1, pages);
    let sql = format!(
        "{ROW_SQL} FROM file f WHERE {filter} ORDER BY f.creation_timestamp DESC, f.id LIMIT {PAGE_SIZE} OFFSET ?2"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows: Vec<FileRow> =
        stmt.query_map(params![like, (page - 1) * PAGE_SIZE], |r| row_from(r, data_dir))?.collect::<Result<_, _>>()?;
    let mut links = conn.prepare(
        "SELECT CASE fa.entity_type_id \
           WHEN 10 THEN (SELECT name_cached FROM party WHERE id = fa.entity_id) \
           WHEN 11 THEN (SELECT COALESCE(NULLIF(subject, ''), 'Activity') FROM activity WHERE id = fa.entity_id) \
           WHEN 12 THEN (SELECT doc_number FROM sales_transaction WHERE id = fa.entity_id) \
           WHEN 13 THEN (SELECT COALESCE(party_name_cached, 'Opportunity') FROM opportunity WHERE id = fa.entity_id) END \
         FROM file_attachment fa WHERE fa.attachment_id = ?1 ORDER BY fa.entity_type_id, fa.rowid",
    )?;
    for row in &mut rows {
        row.attached_to = links
            .query_map([&row.id], |r| r.get::<_, Option<String>>(0))?
            .filter_map(|n| n.transpose())
            .collect::<Result<_, _>>()?;
    }
    Ok(FilePage { rows, matched, page, pages })
}

/// A stored file to serve or open: its name, type and full path (`None` when the copy is missing).
#[derive(Debug, Clone)]
pub struct StoredFile {
    pub file_name: String,
    pub content_type: String,
    pub path: Option<PathBuf>,
    pub thumbnail: Option<PathBuf>,
}

pub fn stored_file(conn: &Connection, data_dir: Option<&Path>, id: &str) -> Result<StoredFile, CommandError> {
    let (file_name, content_type, stored, thumb): (String, String, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT COALESCE(file_name, ''), COALESCE(content_type, ''), container_path, thumbnail_path FROM file \
             WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?
        .ok_or(CommandError::NotFound)?;
    let find = |s: Option<String>| data_dir.zip(s).and_then(|(d, s)| resolve(d, &s));
    Ok(StoredFile { file_name, content_type, path: find(stored), thumbnail: find(thumb) })
}

/// "1.9 MB", "88 KB": the size the source stores is KB (617 s104), shown in 1000s as the source counts.
pub fn display_size(kb: Option<f64>) -> String {
    match kb {
        None => String::new(),
        Some(k) if k >= 1000.0 => format!("{:.1} MB", k / 1000.0),
        Some(k) if k >= 1.0 => format!("{} KB", k.round() as i64),
        Some(_) => "Under 1 KB".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_limit_is_20480_kb_rounded_to_hundredths() {
        assert_eq!(size_hundredths_kb(1_887_437), 188_744);
        assert!(!too_large(20_480_004));
        assert!(too_large(FIRST_REFUSED_BYTES));
        assert!(!too_large(1));
    }

    #[test]
    fn extension_type_and_category_follow_617() {
        assert_eq!(extension("Phase 2 proposal v3.pdf"), "pdf");
        assert_eq!(extension("archive.tar.gz"), "gz");
        assert_eq!(extension("README"), "");
        assert_eq!(extension("trailing."), "");
        assert_eq!(content_type("jpg"), "image/jpg");
        assert_eq!(content_type("JPEG"), "image/jpeg");
        assert_eq!(content_type("PDF"), "application/pdf");
        assert_eq!(content_type("bmp"), "", "617 s117 has no type for bmp or tif");
        assert_eq!(content_type("xlsx"), "");
        assert_eq!(serve_type("image/jpg"), "image/jpeg");
        // B6 (Admiral's ruling): a zip opens too; the system only unpacks it and runs nothing.
        assert!(openable("PDF") && openable("xlsx") && openable("jpeg") && openable("ZIP"));
        for ext in ["command", "sh", "exe", "app", "html", "js", "pkg", "dmg", "bat", "ps1", "jar", ""] {
            assert!(!openable(ext), "{ext}");
        }
        assert_eq!(serve_type(""), "application/octet-stream");
        for ext in ["png", "jpg", "jpeg", "gif", "bmp", "tif", "tiff", "PNG"] {
            assert_eq!(category(ext), "Image", "{ext}");
        }
        for ext in ["pdf", "docx", "zip", ""] {
            assert_eq!(category(ext), "Document", "{ext}");
        }
    }

    #[test]
    fn sizes_display_in_thousands() {
        assert_eq!(display_size(Some(1887.44)), "1.9 MB");
        assert_eq!(display_size(Some(90.11)), "90 KB");
        assert_eq!(display_size(Some(0.4)), "Under 1 KB");
        assert_eq!(display_size(None), "");
    }

    #[test]
    fn only_paths_inside_the_store_resolve() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("files/2026")).unwrap();
        std::fs::write(dir.path().join("files/2026/a.txt"), "a").unwrap();
        std::fs::write(dir.path().join("secret.txt"), "s").unwrap();
        assert!(resolve(dir.path(), "files/2026/a.txt").is_some());
        for bad in [
            "files/../secret.txt",
            "secret.txt",
            "/etc/hosts",
            "files/2026/missing.txt",
            "files//a",
            "files\\2026\\a.txt",
        ] {
            assert!(resolve(dir.path(), bad).is_none(), "{bad}");
        }
    }
}
