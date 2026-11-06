//! UI-12 editor and the UI-13 write actions: new invoice or receipt (kontacts 272, 514), save (OP-SALES-04, 05), post and
//! unpost (OP-SALES-06), delete (OP-SALES-07), remove lines (OP-SALES-08). A field change saves the whole form and the
//! answer is the editor again, so the page never does money arithmetic (S-03). A locked transaction answers 409 (AP-05).

use super::activities::command_error;
use super::contact_forms::ConfirmDialog;
use super::current_user::CurrentUser;
use super::party_forms::{go, toast};
use crate::admin::{CommandError, FieldError};
use crate::items::{self, ItemRow};
use crate::sales::{self, Kind, LineInput, TxnEdit, TxnInput};
use crate::web::{urlencode, AppError, FormPairs};
use crate::AppState;
use askama::Template;
use axum::extract::{Path, Query, RawQuery, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::Form;
use karbon_money::Cents;
use serde::Deserialize;

fn lock_db(state: &AppState) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>, AppError> {
    state.db.lock().map_err(|e| AppError::Internal(e.to_string()))
}

pub fn doc_href(doc: &str) -> String {
    format!("/sales/{}", urlencode(doc))
}

/// Every field of the editor's form. Anything else, a payment or a total included, is refused (BR-SALES-02, AP-06,
/// T-08-06).
const FORM_FIELDS: &[&str] = &[
    "doc_number",
    "date",
    "due_date",
    "term",
    "tax_rate",
    "ship_amt",
    "ship_date",
    "customer_memo",
    "private_note",
    "bill_email_address",
    "bill_line_1",
    "bill_line_2",
    "bill_city",
    "bill_state",
    "bill_postal_code",
    "bill_country",
    "ship_line_1",
    "ship_line_2",
    "ship_city",
    "ship_state",
    "ship_postal_code",
    "ship_country",
    "id_account_deposit",
    "payment_ref_num",
    "line_id",
    "line_item",
    "line_description",
    "line_qty",
    "line_price",
    "line_taxable",
    "add_line",
    "then_lock",
];

fn input_of(form: &FormPairs) -> Result<TxnInput, String> {
    let ids = form.all("line_id");
    let (item, description, qty, price) =
        (form.all("line_item"), form.all("line_description"), form.all("line_qty"), form.all("line_price"));
    if [item.len(), description.len(), qty.len(), price.len()].iter().any(|n| *n != ids.len()) {
        return Err("Every line needs an id, item, description, quantity and rate.".into());
    }
    let taxable = form.all("line_taxable");
    let lines = ids
        .iter()
        .enumerate()
        .map(|(i, id)| LineInput {
            id: id.clone(),
            item_id: item[i].clone(),
            description: description[i].clone(),
            qty: qty[i].clone(),
            unit_price: price[i].clone(),
            taxable: taxable.contains(id),
        })
        .collect();
    let address = |p: &str| sales::Address {
        line_1: form.one(&format!("{p}_line_1")),
        line_2: form.one(&format!("{p}_line_2")),
        city: form.one(&format!("{p}_city")),
        state: form.one(&format!("{p}_state")),
        postal_code: form.one(&format!("{p}_postal_code")),
        country: form.one(&format!("{p}_country")),
    };
    Ok(TxnInput {
        doc_number: form.one("doc_number"),
        date: form.one("date"),
        due_date: form.one("due_date"),
        term: form.one("term"),
        tax_rate: form.one("tax_rate"),
        ship_amt: form.one("ship_amt"),
        ship_date: form.one("ship_date"),
        customer_memo: form.one("customer_memo"),
        private_note: form.one("private_note"),
        bill_email_address: form.one("bill_email_address"),
        bill: address("bill"),
        ship: address("ship"),
        id_account_deposit: form.one("id_account_deposit"),
        payment_ref_num: form.one("payment_ref_num"),
        lines,
    })
}

/// One line of the editor.
pub struct LineRow {
    pub line: LineInput,
    pub amount: String,
    /// The items this line may use: the active ones, plus its own when inactive (BR-SALES-16).
    pub choices: Vec<ItemRow>,
}

#[derive(Template)]
#[template(path = "sales/editor.html")]
pub struct Editor {
    pub t: TxnEdit,
    pub rows: Vec<LineRow>,
    pub terms: Vec<String>,
    pub errors: Vec<FieldError>,
    pub status_label: &'static str,
    /// The retainer the invoice could use, and the customer's accounts receivable (layout 69's popover).
    pub retainer_available: Cents,
    pub receivable: Cents,
}

impl Editor {
    pub fn error(&self, name: &str) -> Option<&str> {
        self.errors.iter().find(|e| e.field == name).map(|e| e.message.as_str())
    }
    pub fn line_errors(&self) -> Vec<&str> {
        self.errors.iter().filter(|e| e.field == "lines").map(|e| e.message.as_str()).collect()
    }
    pub fn is_invoice(&self) -> bool {
        self.t.kind == Kind::Invoice
    }
    pub fn save_path(&self) -> String {
        format!("{}/{}", self.t.kind.path(), urlencode(&self.t.id))
    }
    pub fn subtotal_before_retainer(&self) -> Cents {
        self.t.totals.subtotal - self.t.retainer_credit
    }
    pub fn total_before_retainer(&self) -> Cents {
        self.subtotal_before_retainer() + self.t.totals.total_tax + self.ship()
    }
    pub fn ship(&self) -> Cents {
        self.t.totals.total_amt - self.t.totals.subtotal - self.t.totals.total_tax
    }
    pub fn has_retainer(&self) -> bool {
        self.t.retainer_credit.0 != 0
    }
    pub fn has_paid(&self) -> bool {
        self.t.paid.0 != 0
    }
    pub fn retainer_credit(&self) -> Cents {
        -self.t.retainer_credit
    }
    pub fn bill_fields(&self) -> Vec<(&'static str, &'static str, &str)> {
        address_fields("bill", &self.t.input.bill)
    }
    pub fn ship_fields(&self) -> Vec<(&'static str, &'static str, &str)> {
        address_fields("ship", &self.t.input.ship)
    }
    pub fn tax_label(&self) -> String {
        crate::admin::format_percent(self.t.tax_rate_bp)
    }
}

/// (field name, label, value) of an address's inputs.
fn address_fields<'a>(prefix: &str, a: &'a sales::Address) -> Vec<(&'static str, &'static str, &'a str)> {
    let names: [&'static str; 6] = if prefix == "bill" {
        ["bill_line_1", "bill_line_2", "bill_city", "bill_state", "bill_postal_code", "bill_country"]
    } else {
        ["ship_line_1", "ship_line_2", "ship_city", "ship_state", "ship_postal_code", "ship_country"]
    };
    let labels = ["Address line 1", "Address line 2", "City", "State", "Postal code", "Country"];
    let values = [&a.line_1, &a.line_2, &a.city, &a.state, &a.postal_code, &a.country];
    (0..6).map(|i| (names[i], labels[i], values[i].as_str())).collect()
}

/// The editor for a stored transaction; with `typed`, the form as it was sent (a refused save), with its errors.
pub fn editor(
    conn: &rusqlite::Connection,
    id: &str,
    typed: Option<TxnInput>,
    errors: Vec<FieldError>,
) -> Result<Editor, AppError> {
    let mut t = sales::transaction_get(conn, id).map_err(|e| match e {
        CommandError::NotFound => AppError::NotFound,
        e => AppError::Internal(format!("transaction_get: {e}")),
    })?;
    let stored = t.input.lines.clone();
    if let Some(input) = typed {
        t.input = input;
    }
    let active = items::active_items(conn, "", 500).map_err(|e| AppError::Internal(format!("active_items: {e}")))?;
    let mut rows = Vec::with_capacity(t.input.lines.len());
    for line in &t.input.lines {
        let i = stored.iter().position(|s| s.id == line.id);
        let mut choices = active.clone();
        if !line.item_id.is_empty() && !choices.iter().any(|c| c.id == line.item_id) {
            if let Ok(own) = items::item_row(conn, &line.item_id) {
                choices.push(own);
            }
        }
        rows.push(LineRow {
            line: line.clone(),
            amount: i.and_then(|i| t.amounts.get(i)).copied().unwrap_or(Cents::ZERO).to_string(),
            choices,
        });
    }
    let retainer_available = sales::retainer_available(conn, &t.customer_role_id)
        .map_err(|e| AppError::Internal(format!("retainer_available: {e}")))?;
    let receivable: i64 = rusqlite::OptionalExtension::optional(conn.query_row(
        "SELECT COALESCE(balance, 0) FROM customer WHERE id = ?1",
        [&t.customer_role_id],
        |r| r.get(0),
    ))?
    .unwrap_or(0);
    Ok(Editor {
        rows,
        terms: sales::term_choices(conn)?,
        errors,
        status_label: "Draft",
        retainer_available,
        receivable: Cents(receivable),
        t,
    })
}

// ---------------------------------------------------------------------------------------------------------------
// New: the customer dialog (kontacts 272, 514)
// ---------------------------------------------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "sales/customer_picker.html")]
struct CustomerPicker {
    q: String,
    selected: String,
    rows: Vec<(String, String)>,
}

pub fn customer_picker(conn: &rusqlite::Connection, q: &str, selected: &str) -> Result<String, AppError> {
    let rows = sales::customer_search(conn, q, 8)?;
    Ok(CustomerPicker { q: q.into(), selected: selected.into(), rows }.render()?)
}

#[derive(Template)]
#[template(path = "sales/new_dialog.html")]
struct NewDialog {
    title: &'static str,
    action: &'static str,
    picker: String,
    error: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct NewQuery {
    customer_id: String,
}

fn new_dialog(state: &AppState, kind: Kind, selected: &str, error: Option<String>) -> Result<NewDialog, AppError> {
    let conn = lock_db(state)?;
    // Opened from a customer: that customer is listed and chosen.
    let name: Option<String> = rusqlite::OptionalExtension::optional(conn.query_row(
        "SELECT p.name_cached FROM party_role r JOIN party p ON p.id = r.party_id WHERE r.id = ?1",
        [selected],
        |r| r.get(0),
    ))?;
    Ok(NewDialog {
        title: if kind == Kind::Invoice { "New invoice" } else { "New sales receipt" },
        action: kind.path(),
        picker: customer_picker(&conn, name.as_deref().unwrap_or(""), selected)?,
        error,
    })
}

pub async fn new_invoice(State(state): State<AppState>, Query(q): Query<NewQuery>) -> Result<Html<String>, AppError> {
    Ok(Html(new_dialog(&state, Kind::Invoice, &q.customer_id, None)?.render()?))
}

pub async fn new_receipt(State(state): State<AppState>, Query(q): Query<NewQuery>) -> Result<Html<String>, AppError> {
    Ok(Html(new_dialog(&state, Kind::Receipt, &q.customer_id, None)?.render()?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewForm {
    #[serde(default)]
    customer_id: String,
    /// The picker's search box, sent with the form.
    #[serde(default)]
    #[allow(dead_code)]
    q: String,
}

async fn create(
    state: AppState,
    headers: HeaderMap,
    actor: crate::audit::Actor,
    kind: Kind,
    form: NewForm,
) -> Result<Response, AppError> {
    let today = state.today();
    let result = {
        let mut conn = lock_db(&state)?;
        sales::transaction_create(&mut conn, kind, &form.customer_id, &today, &actor)
    };
    match result {
        Ok((_, doc)) => go(&headers, &doc_href(&doc)),
        Err(CommandError::Validation(errors)) => {
            let message = errors.first().map(|e| e.message.clone());
            let dialog = new_dialog(&state, kind, &form.customer_id, message)?;
            Ok((StatusCode::UNPROCESSABLE_ENTITY, Html(dialog.render()?)).into_response())
        }
        Err(e) => command_error("transaction_create", e),
    }
}

pub async fn create_invoice(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<NewForm>,
) -> Result<Response, AppError> {
    create(state, headers, actor, Kind::Invoice, form).await
}

pub async fn create_receipt(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<NewForm>,
) -> Result<Response, AppError> {
    create(state, headers, actor, Kind::Receipt, form).await
}

// ---------------------------------------------------------------------------------------------------------------
// Save (OP-SALES-04, 05)
// ---------------------------------------------------------------------------------------------------------------

async fn save(
    state: AppState,
    headers: HeaderMap,
    actor: crate::audit::Actor,
    kind: Kind,
    id: String,
    form: FormPairs,
) -> Result<Response, AppError> {
    if let Err(refused) = form.only(FORM_FIELDS) {
        return Ok(*refused);
    }
    let input = match input_of(&form) {
        Ok(i) => i,
        Err(message) => return Ok((StatusCode::UNPROCESSABLE_ENTITY, message).into_response()),
    };
    let then_lock = form.one("then_lock") == "true";
    let add_line = form.one("add_line") == "true";
    let mut conn = lock_db(&state)?;
    match sales::transaction_save(&mut conn, kind, &id, &input, add_line, &actor) {
        Ok(()) => {
            if then_lock {
                // Lock saves the form first (kontacts 377 s3, s8): two commands, as there.
                return match sales::transaction_lock(&mut conn, &id, true, &actor) {
                    Ok(doc) => {
                        let mut res = go(&headers, &doc_href(&doc))?;
                        res.headers_mut().insert("HX-Trigger", toast("Posted", false));
                        Ok(res)
                    }
                    Err(e) => command_error("transaction_lock", e),
                };
            }
            let view = editor(&conn, &id, None, Vec::new())?;
            let url = HeaderValue::from_str(&doc_href(&view.t.input.doc_number))
                .map_err(|e| AppError::Internal(e.to_string()))?;
            Ok(([("HX-Replace-Url", url)], Html(view.render()?)).into_response())
        }
        Err(CommandError::Validation(errors)) => {
            let view = editor(&conn, &id, Some(input), errors)?;
            Ok((StatusCode::UNPROCESSABLE_ENTITY, Html(view.render()?)).into_response())
        }
        Err(e) => command_error("transaction_save", e),
    }
}

pub async fn save_invoice(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    CurrentUser(actor): CurrentUser,
    form: FormPairs,
) -> Result<Response, AppError> {
    save(state, headers, actor, Kind::Invoice, id, form).await
}

pub async fn save_receipt(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    CurrentUser(actor): CurrentUser,
    form: FormPairs,
) -> Result<Response, AppError> {
    save(state, headers, actor, Kind::Receipt, id, form).await
}

/// `POST /invoices/{id}/apply-retainer` (OP-SALES-12). The form is saved first (kontacts 223 s5 commits, then s7 applies);
/// a request with no form applies at once. The answer is the editor with the new line and totals.
pub async fn apply_retainer(
    State(state): State<AppState>,
    Path(id): Path<String>,
    CurrentUser(actor): CurrentUser,
    form: FormPairs,
) -> Result<Response, AppError> {
    if let Err(refused) = form.only(FORM_FIELDS) {
        return Ok(*refused);
    }
    let mut conn = lock_db(&state)?;
    if !form.0.is_empty() {
        let input = match input_of(&form) {
            Ok(i) => i,
            Err(message) => return Ok((StatusCode::UNPROCESSABLE_ENTITY, message).into_response()),
        };
        match sales::transaction_save(&mut conn, Kind::Invoice, &id, &input, false, &actor) {
            Ok(()) => {}
            Err(CommandError::Validation(errors)) => {
                let view = editor(&conn, &id, Some(input), errors)?;
                return Ok((StatusCode::UNPROCESSABLE_ENTITY, Html(view.render()?)).into_response());
            }
            Err(e) => return command_error("transaction_save", e),
        }
    }
    match sales::retainer_apply(&mut conn, &id, &actor) {
        Ok(_) => {
            let view = editor(&conn, &id, None, Vec::new())?;
            Ok(([("HX-Trigger", toast("Retainer applied", false))], Html(view.render()?)).into_response())
        }
        Err(e) => command_error("retainer_apply", e),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Post, unpost (OP-SALES-06)
// ---------------------------------------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockForm {
    lock: String,
    /// Unpost only: "true" once the dialog has been answered.
    #[serde(default)]
    confirm: String,
}

/// The question before an unpost (AP-07 pattern): a stray click on the lock icon or Unlock changes nothing.
#[derive(Template)]
#[template(path = "sales/unpost_confirm.html")]
struct UnpostConfirm {
    id: String,
    doc_number: String,
    summary: String,
}

/// `POST /sales-transactions/{id}/lock`: the page opens again as the editor or the read-only view (kontacts 335, 377).
/// Unpost asks first (user call over the source, 2026-11-06): without `confirm=true` the answer is the dialog and
/// nothing changes.
pub async fn lock(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<LockForm>,
) -> Result<Response, AppError> {
    let posting = match form.lock.as_str() {
        "true" => true,
        "false" => false,
        _ => return Ok((StatusCode::UNPROCESSABLE_ENTITY, "lock must be true or false").into_response()),
    };
    let mut conn = lock_db(&state)?;
    if !posting && form.confirm != "true" {
        return match sales::unpost_summary(&conn, &id) {
            Ok(Some((doc_number, summary))) => {
                Ok(Html(UnpostConfirm { id, doc_number, summary }.render()?).into_response())
            }
            // Already a draft: nothing to ask, the page opens as it is.
            Ok(None) => {
                let doc = sales::transaction_get(&conn, &id).map_err(|e| AppError::Internal(e.to_string()))?;
                go(&headers, &doc_href(&doc.input.doc_number))
            }
            Err(e) => command_error("transaction_lock", e),
        };
    }
    match sales::transaction_lock(&mut conn, &id, posting, &actor) {
        Ok(doc) => {
            let mut res = go(&headers, &doc_href(&doc))?;
            res.headers_mut().insert("HX-Trigger", toast(if posting { "Posted" } else { "Unposted" }, false));
            Ok(res)
        }
        Err(e) => command_error("transaction_lock", e),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Delete, remove lines (OP-SALES-07, 08; AP-07)
// ---------------------------------------------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ConfirmQuery {
    confirm: String,
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<ConfirmQuery>,
    headers: HeaderMap,
    CurrentUser(_actor): CurrentUser,
) -> Result<Response, AppError> {
    let mut conn = lock_db(&state)?;
    match sales::transaction_delete(&mut conn, &id, q.confirm == "true") {
        Ok(()) => {
            let mut res = go(&headers, "/sales")?;
            res.headers_mut().insert("HX-Trigger", toast("Deleted", true));
            Ok(res)
        }
        Err(CommandError::ConfirmationRequired(summary)) => Ok(Html(
            ConfirmDialog {
                title: "Delete this transaction?".into(),
                summary,
                action: format!("/sales-transactions/{}?confirm=true", urlencode(&id)),
                button: "Delete".into(),
            }
            .render()?,
        )
        .into_response()),
        Err(e) => command_error("transaction_delete", e),
    }
}

/// `DELETE /sales-transactions/{id}/lines?line_id=..&confirm=true`.
pub async fn remove_lines(
    State(state): State<AppState>,
    Path(id): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    CurrentUser(actor): CurrentUser,
) -> Result<Response, AppError> {
    let query = query.unwrap_or_default();
    let form = FormPairs::parse(&query);
    if let Err(refused) = form.only(&["line_id", "confirm"]) {
        return Ok(*refused);
    }
    let ids = form.all("line_id");
    let mut conn = lock_db(&state)?;
    match sales::lines_remove(&mut conn, &id, &ids, form.one("confirm") == "true", &actor) {
        Ok(()) => {
            let doc = sales::transaction_get(&conn, &id)
                .map_err(|e| AppError::Internal(format!("transaction_get: {e}")))?
                .input
                .doc_number;
            let mut res = go(&headers, &doc_href(&doc))?;
            res.headers_mut().insert("HX-Trigger", toast("Line removed", true));
            Ok(res)
        }
        Err(CommandError::ConfirmationRequired(summary)) => {
            let lines: String = ids.iter().map(|l| format!("line_id={}&", urlencode(l))).collect();
            Ok(Html(
                ConfirmDialog {
                    title: "Remove this line?".into(),
                    summary,
                    action: format!("/sales-transactions/{}/lines?{lines}confirm=true", urlencode(&id)),
                    button: "Remove".into(),
                }
                .render()?,
            )
            .into_response())
        }
        Err(CommandError::Validation(errors)) => {
            let text = errors.first().map(|e| e.message.clone()).unwrap_or_default();
            Ok((StatusCode::UNPROCESSABLE_ENTITY, text).into_response())
        }
        Err(e) => command_error("lines_remove", e),
    }
}
