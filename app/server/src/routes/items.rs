//! UI-15 Items: the list (layout 86), the item card (layouts 135, 136) and the item picker. OP-SALES-10.

use super::current_user::CurrentUser;
use super::parties::page_error;
use super::party_forms::{go, toast};
use super::Nav;
use crate::admin::{CommandError, FieldError};
use crate::items::{self, ItemInput, ItemRow};
use crate::money::usd;
use crate::web::{is_htmx, render, urlencode, AppError};
use crate::AppState;
use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::Form;
use karbon_money::Cents;
use serde::Deserialize;

fn lock(state: &AppState) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>, AppError> {
    state.db.lock().map_err(|e| AppError::Internal(e.to_string()))
}

/// One row as the list shows it.
pub struct ItemView {
    pub id: String,
    pub name: String,
    pub sku: String,
    pub description: String,
    pub price: String,
    pub active: bool,
    pub edit_href: String,
    /// The row goes out of band (the answer to a save), so htmx puts it back in the list.
    pub oob: bool,
}

impl From<ItemRow> for ItemView {
    fn from(i: ItemRow) -> Self {
        ItemView {
            edit_href: format!("/items/{}/edit", urlencode(&i.id)),
            id: i.id,
            name: i.name,
            sku: i.sku,
            description: i.description,
            price: usd(i.unit_price),
            active: i.active,
            oob: false,
        }
    }
}

#[derive(Deserialize, Default, Clone)]
#[serde(default)]
pub struct ItemsQuery {
    pub q: String,
    pub page: i64,
}

impl ItemsQuery {
    pub fn href(&self, page: i64) -> String {
        format!("/items?q={}&page={page}", urlencode(&self.q))
    }
}

#[derive(Template)]
#[template(path = "items/table.html")]
struct ItemsTable<'a> {
    query: &'a ItemsQuery,
    rows: Vec<ItemView>,
    page: i64,
    pages: i64,
    matched: i64,
    first: i64,
    last: i64,
}

#[derive(Template)]
#[template(path = "items/list.html")]
struct ItemsPage<'a> {
    nav: Nav,
    demo: bool,
    table: ItemsTable<'a>,
}

/// `GET /items` (`?q=&page=`).
pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ItemsQuery>,
) -> Result<Html<String>, AppError> {
    let conn = lock(&state)?;
    let found = items::item_list(&conn, &query.q, query.page, 0).map_err(|e| page_error("item_list", e))?;
    let (first, last) = found.span();
    let table = ItemsTable {
        query: &query,
        page: found.page,
        pages: found.pages,
        matched: found.matched,
        first,
        last,
        rows: found.rows.into_iter().map(ItemView::from).collect(),
    };
    if is_htmx(&headers) {
        return render(&table);
    }
    render(&ItemsPage { nav: Nav::Items, demo: crate::db::has_demo_data(&conn)?, table })
}

/// The card's fields; anything else is refused (05). A checkbox is present when it is checked.
#[derive(Deserialize, Default, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct ItemForm {
    name: String,
    sku: String,
    description: String,
    unit_price: String,
    taxable: String,
    item_type: String,
    purchase_cost: String,
    purchase_desc: String,
    qty_on_hand: String,
    track_qty_on_hand: String,
    inv_start_date: String,
    level: String,
    active: String,
}

impl From<ItemForm> for ItemInput {
    fn from(f: ItemForm) -> Self {
        ItemInput {
            name: f.name,
            sku: f.sku,
            description: f.description,
            unit_price: f.unit_price,
            taxable: !f.taxable.is_empty(),
            item_type: f.item_type,
            purchase_cost: f.purchase_cost,
            purchase_desc: f.purchase_desc,
            qty_on_hand: f.qty_on_hand,
            track_qty_on_hand: !f.track_qty_on_hand.is_empty(),
            inv_start_date: f.inv_start_date,
            level: f.level,
            active: !f.active.is_empty(),
        }
    }
}

#[derive(Template)]
#[template(path = "items/card.html")]
struct ItemCard {
    /// The stored item (an edit); none for a new one.
    id: Option<String>,
    form: ItemInput,
    types: Vec<String>,
    errors: Vec<FieldError>,
}

impl ItemCard {
    fn new(id: Option<&str>, form: ItemInput, errors: Vec<FieldError>) -> Self {
        // Value list 4, plus a stored value outside it (an import), which stays until it is changed.
        let mut types: Vec<String> = items::ITEM_TYPES.iter().map(|t| (*t).to_owned()).collect();
        if !form.item_type.is_empty() && !types.contains(&form.item_type) {
            types.push(form.item_type.clone());
        }
        ItemCard { id: id.map(str::to_owned), form, types, errors }
    }
    fn action(&self) -> String {
        self.id.as_deref().map_or_else(|| "/items".to_owned(), |id| format!("/items/{}", urlencode(id)))
    }
    fn error(&self, field: &str) -> Option<&str> {
        self.errors.iter().find(|e| e.field == field).map(|e| e.message.as_str())
    }
}

/// `GET /items/new`. Q7: a new item is active.
pub async fn new_dialog() -> Result<Html<String>, AppError> {
    render(&ItemCard::new(None, ItemInput { active: true, ..Default::default() }, Vec::new()))
}

/// `GET /items/{id}/edit`.
pub async fn edit_dialog(State(state): State<AppState>, Path(id): Path<String>) -> Result<Html<String>, AppError> {
    let conn = lock(&state)?;
    let form = items::item_get(&conn, &id).map_err(|e| page_error("item_get", e))?;
    render(&ItemCard::new(Some(&id), form, Vec::new()))
}

/// The card again with its errors (D-25).
fn refused(id: Option<&str>, form: ItemInput, errors: Vec<FieldError>) -> Result<Response, AppError> {
    let html = ItemCard::new(id, form, errors).render()?;
    Ok((StatusCode::UNPROCESSABLE_ENTITY, Html(html)).into_response())
}

/// `POST /items` (OP-SALES-10): the list again, with the new item in its place by name.
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<ItemForm>,
) -> Result<Response, AppError> {
    let input = ItemInput::from(form);
    let mut conn = lock(&state)?;
    match items::item_save(&mut conn, None, &input, &actor) {
        Ok(_) => {
            let mut res = go(&headers, "/items")?;
            res.headers_mut().insert("HX-Trigger", toast("Item created", false));
            Ok(res)
        }
        Err(CommandError::Validation(errors)) => refused(None, input, errors),
        Err(e) => Err(page_error("item_save", e)),
    }
}

#[derive(Template)]
#[template(path = "items/row.html")]
struct ItemRowFragment {
    i: ItemView,
}

/// `PUT /items/{id}` (OP-SALES-10, BR-SALES-16: the Active box deactivates): the row, out of band; the dialog closes.
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<ItemForm>,
) -> Result<Response, AppError> {
    let input = ItemInput::from(form);
    let mut conn = lock(&state)?;
    match items::item_save(&mut conn, Some(&id), &input, &actor) {
        Ok(_) => {
            let row = items::item_row(&conn, &id).map_err(|e| page_error("item_row", e))?;
            let html = ItemRowFragment { i: ItemView { oob: true, ..row.into() } }.render()?;
            Ok(([("HX-Trigger", toast("Item saved", true))], Html(html)).into_response())
        }
        Err(CommandError::Validation(errors)) => refused(Some(&id), input, errors),
        Err(e) => Err(page_error("item_save", e)),
    }
}

/// The item picker (`GET /pickers/item`): a search box and radio rows named `item_id`. Each row carries what a new
/// line copies from the item (X4: unit price, description, taxable).
#[derive(Template)]
#[template(path = "items/picker.html")]
struct ItemPicker {
    q: String,
    selected: String,
    rows: Vec<PickerRow>,
}

struct PickerRow {
    id: String,
    name: String,
    sku: String,
    description: String,
    price: String,
    input_price: String,
    taxable: bool,
}

const PICKER_ROWS: i64 = 30;

pub fn item_picker(conn: &rusqlite::Connection, q: &str, selected: &str) -> Result<String, AppError> {
    let rows = items::active_items(conn, q, PICKER_ROWS)
        .map_err(|e| page_error("active_items", e))?
        .into_iter()
        .map(|i| PickerRow {
            price: usd(i.unit_price),
            input_price: Cents(i.unit_price).to_input_value(),
            id: i.id,
            name: i.name,
            sku: i.sku,
            description: i.description,
            taxable: i.taxable,
        })
        .collect();
    Ok(ItemPicker { q: q.to_owned(), selected: selected.to_owned(), rows }.render()?)
}
