//! OP-SALES-10 `item_save` (BR-SALES-16): items are created and edited on the card, made inactive and never deleted.
//! Plain SQL, one `BEGIN IMMEDIATE` transaction per write; `item` has no stamp columns in 04, so the stamp helpers set
//! nothing there. Form parsing and rendering are in `routes::items`.

use crate::admin::{CommandError, FieldError};
use crate::audit::{self, Actor, Stamp};
use crate::ids::new_id;
use crate::sales_list::{paging, Page};
use karbon_money::{Cents, Qty};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, TransactionBehavior};

/// Value list 4 "ItemType" of karbon_data.
pub const ITEM_TYPES: [&str; 3] = ["Inventory", "Service", "NonInventory"];

/// The name BR-SALES-07 matches on. Q4: an item with this name keeps it.
pub const RETAINER: &str = "Retainer";

/// The card as typed (layouts 135, 136 without the QBO account fields, D-02). Money and numbers stay text until
/// [`item_save`] parses them, so a refused value goes back into the form unchanged.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemInput {
    pub name: String,
    pub sku: String,
    pub description: String,
    pub unit_price: String,
    pub taxable: bool,
    pub item_type: String,
    pub purchase_cost: String,
    pub purchase_desc: String,
    pub qty_on_hand: String,
    pub track_qty_on_hand: bool,
    pub inv_start_date: String,
    pub level: String,
    pub active: bool,
}

/// One row of the list and the picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemRow {
    pub id: String,
    pub name: String,
    pub sku: String,
    pub description: String,
    pub unit_price: i64,
    pub taxable: bool,
    pub active: bool,
}

const ROW_COLUMNS: &str = "id, COALESCE(name, ''), COALESCE(sku, ''), COALESCE(description, ''),
    COALESCE(unit_price, 0), COALESCE(taxable, 0) <> 0, COALESCE(active, 0) <> 0";

fn read_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ItemRow> {
    Ok(ItemRow {
        id: r.get(0)?,
        name: r.get(1)?,
        sku: r.get(2)?,
        description: r.get(3)?,
        unit_price: r.get(4)?,
        taxable: r.get(5)?,
        active: r.get(6)?,
    })
}

/// A contains-search over name, SKU and description, as a WHERE fragment and its one bound value.
fn text_filter(q: &str) -> (String, Vec<String>) {
    let text = q.trim().to_lowercase();
    if text.is_empty() {
        return ("1 = 1".to_owned(), Vec::new());
    }
    let sql = "(instr(lower(COALESCE(name, '')), ?1) > 0 OR instr(lower(COALESCE(sku, '')), ?1) > 0 \
               OR instr(lower(COALESCE(description, '')), ?1) > 0)";
    (sql.to_owned(), vec![text])
}

/// The list (layout 86): every item, inactive ones too, by name (kontacts 345).
pub fn item_list(conn: &Connection, q: &str, page: i64, page_size: i64) -> Result<Page<ItemRow>, CommandError> {
    let (filter, values) = text_filter(q);
    let matched: i64 =
        conn.query_row(&format!("SELECT COUNT(*) FROM item WHERE {filter}"), params_from_iter(values.iter()), |r| {
            r.get(0)
        })?;
    let (page, pages, size) = paging(matched, page, page_size);
    let sql = format!(
        "SELECT {ROW_COLUMNS} FROM item WHERE {filter} ORDER BY lower(COALESCE(name, '')), id
         LIMIT {size} OFFSET {}",
        (page - 1) * size
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(values.iter()), read_row)?.collect::<rusqlite::Result<_>>()?;
    Ok(Page { rows, matched, page, pages, page_size: size, total: 0, balance: 0 })
}

/// The item picker: active items only (BR-SALES-16), by name.
pub fn active_items(conn: &Connection, q: &str, limit: i64) -> Result<Vec<ItemRow>, CommandError> {
    let (filter, values) = text_filter(q);
    let sql = format!(
        "SELECT {ROW_COLUMNS} FROM item WHERE COALESCE(active, 0) <> 0 AND {filter}
         ORDER BY lower(COALESCE(name, '')), id LIMIT {limit}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(values.iter()), read_row)?.collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// One row, for the answer after a save.
pub fn item_row(conn: &Connection, id: &str) -> Result<ItemRow, CommandError> {
    conn.query_row(&format!("SELECT {ROW_COLUMNS} FROM item WHERE id = ?1"), [id], read_row)
        .optional()?
        .ok_or(CommandError::NotFound)
}

/// The card's values of a stored item, as the form shows them.
pub fn item_get(conn: &Connection, id: &str) -> Result<ItemInput, CommandError> {
    conn.query_row(
        "SELECT COALESCE(name, ''), COALESCE(sku, ''), COALESCE(description, ''), unit_price, COALESCE(taxable, 0) <> 0,
                COALESCE(item_type, ''), purchase_cost, COALESCE(purchase_desc, ''),
                COALESCE(CAST(qty_on_hand AS TEXT), ''), COALESCE(track_qty_on_hand, 0) <> 0,
                COALESCE(inv_start_date, ''), COALESCE(CAST(level AS TEXT), ''), COALESCE(active, 0) <> 0
         FROM item WHERE id = ?1",
        [id],
        |r| {
            let money = |v: Option<i64>| v.map(|c| Cents(c).to_input_value()).unwrap_or_default();
            Ok(ItemInput {
                name: r.get(0)?,
                sku: r.get(1)?,
                description: r.get(2)?,
                unit_price: money(r.get(3)?),
                taxable: r.get(4)?,
                item_type: r.get(5)?,
                purchase_cost: money(r.get(6)?),
                purchase_desc: r.get(7)?,
                qty_on_hand: r.get(8)?,
                track_qty_on_hand: r.get(9)?,
                inv_start_date: r.get(10)?,
                level: r.get(11)?,
                active: r.get(12)?,
            })
        },
    )
    .optional()?
    .ok_or(CommandError::NotFound)
}

fn invalid(errors: &mut Vec<FieldError>, field: &'static str, message: &str) {
    errors.push(FieldError { field, message: message.to_owned() });
}

/// Empty text is NULL, as a FileMaker empty field is.
fn text(s: &str) -> Option<&str> {
    let t = s.trim();
    (!t.is_empty()).then_some(t)
}

/// Creates (`id` none) or updates an item and returns its id. M1: the name is required (kontroller 412 s76; the income
/// account rule went with QBO, D-02). M3: `name_for_active_value_list` is written on every save (field 35). M5: prices
/// are cents with at most 2 decimals typed (BR-SALES-01, OD-04-02). Q4: an item named "Retainer" keeps its name.
pub fn item_save(
    conn: &mut Connection,
    id: Option<&str>,
    input: &ItemInput,
    actor: &Actor,
) -> Result<String, CommandError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let stored: Option<(String, String)> = match id {
        Some(id) => Some(
            tx.query_row("SELECT COALESCE(name, ''), COALESCE(item_type, '') FROM item WHERE id = ?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?
            .ok_or(CommandError::NotFound)?,
        ),
        None => None,
    };

    let mut errors = Vec::new();
    let name = input.name.trim();
    if name.is_empty() {
        invalid(&mut errors, "name", "Enter a name.");
    }
    if let Some((old, _)) = &stored {
        if old == RETAINER && name != RETAINER && !name.is_empty() {
            invalid(&mut errors, "name", "The Retainer item keeps its name: retainer amounts are found by it.");
        }
    }
    let unit_price = Cents::parse_optional(&input.unit_price);
    if unit_price.is_err() {
        invalid(&mut errors, "unit_price", "Enter an amount with at most 2 decimals, like 1,200.00.");
    }
    let purchase_cost = Cents::parse_optional(&input.purchase_cost);
    if purchase_cost.is_err() {
        invalid(&mut errors, "purchase_cost", "Enter an amount with at most 2 decimals, like 1,200.00.");
    }
    let item_type = input.item_type.trim();
    // A stored type outside the list (an import) stays until it is changed.
    let known = ITEM_TYPES.contains(&item_type) || stored.as_ref().is_some_and(|(_, old)| old == item_type);
    if !item_type.is_empty() && !known {
        invalid(&mut errors, "item_type", "Choose Inventory, Service or NonInventory.");
    }
    let qty = Qty::parse_optional(&input.qty_on_hand);
    if qty.is_err() {
        invalid(&mut errors, "qty_on_hand", "Enter a number with at most 4 decimals.");
    }
    let level = match text(&input.level) {
        None => Ok(None),
        Some(t) => t.parse::<i64>().map(Some),
    };
    if level.is_err() {
        invalid(&mut errors, "level", "Enter a whole number.");
    }
    let start = text(&input.inv_start_date);
    if start.is_some_and(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_err()) {
        invalid(&mut errors, "inv_start_date", "Enter a date.");
    }
    if !errors.is_empty() {
        return Err(CommandError::Validation(errors));
    }

    let cents = |v: Result<Option<Cents>, _>| v.ok().flatten().map(|c: Cents| c.0);
    let new = new_id();
    let row_id = id.unwrap_or(&new);
    let stamp = Stamp::now(actor);
    if id.is_none() {
        tx.execute("INSERT INTO item (id) VALUES (?1)", [row_id])?;
    }
    tx.execute(
        "UPDATE item SET name = ?2, sku = ?3, description = ?4, unit_price = ?5, taxable = ?6, item_type = ?7,
                purchase_cost = ?8, purchase_desc = ?9, qty_on_hand = ?10, track_qty_on_hand = ?11,
                inv_start_date = ?12, level = ?13, active = ?14, name_for_active_value_list = ?15
         WHERE id = ?1",
        params![
            row_id,
            name,
            text(&input.sku),
            text(&input.description),
            cents(unit_price),
            i64::from(input.taxable),
            text(item_type),
            cents(purchase_cost),
            text(&input.purchase_desc),
            qty.ok().flatten().map(|q| q.to_input_value()),
            i64::from(input.track_qty_on_hand),
            start,
            level.ok().flatten(),
            i64::from(input.active),
            input.active.then_some(name),
        ],
    )?;
    if id.is_none() {
        audit::stamp_created(&tx, "item", &row_id, &stamp)?;
    } else {
        audit::stamp_modified(&tx, "item", &row_id, &stamp)?;
    }
    tx.commit()?;
    Ok(row_id.to_owned())
}
