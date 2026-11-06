//! UI-14 the payment card (layout 85): a new payment from an invoice (kontacts 221, 276), the same card to edit one
//! (kontacts 216), save (OP-SALES-09). The card lists the customer's unpaid invoices and the payment's own: an amount
//! on a row allocates to it (kontacts 280: Choose fills the invoice's balance).

use super::activities::command_error;
use super::party_forms::toast;
use crate::admin::{CommandError, FieldError};
use crate::invoice::display_date;
use crate::money::usd;
use crate::payments::{self, InvoicePayment, PaymentInput, DEPOSIT_ACCOUNTS, METHODS, NO_CUSTOMER};
use crate::web::{urlencode, AppError, FormPairs};
use crate::AppState;
use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use karbon_money::Cents;
use serde::Deserialize;

fn lock(state: &AppState) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>, AppError> {
    state.db.lock().map_err(|e| AppError::Internal(e.to_string()))
}

/// Every field of the card. Anything else is refused.
const FIELDS: &[&str] =
    &["customer_id", "date", "amount", "method", "reference", "deposit", "line_invoice", "line_amount"];

pub struct CardRow {
    pub invoice_id: String,
    pub doc_number: String,
    pub date: String,
    pub original: String,
    pub balance: String,
    /// The invoice's balance as an input value, for the Choose button (kontacts 280 s12).
    pub balance_input: String,
    pub amount: String,
}

#[derive(Template)]
#[template(path = "sales/payment_card.html")]
pub struct Card {
    pub id: Option<String>,
    pub customer: String,
    pub input: PaymentInput,
    pub rows: Vec<CardRow>,
    pub methods: Vec<String>,
    pub deposits: Vec<String>,
    pub errors: Vec<FieldError>,
}

impl Card {
    pub fn error(&self, name: &str) -> Option<&str> {
        self.errors.iter().find(|e| e.field == name).map(|e| e.message.as_str())
    }
    pub fn line_errors(&self) -> Vec<&str> {
        self.errors.iter().filter(|e| e.field == "lines").map(|e| e.message.as_str()).collect()
    }
    pub fn action(&self) -> String {
        match &self.id {
            Some(id) => format!("/payments/{}", urlencode(id)),
            None => "/payments".into(),
        }
    }
}

/// A fixed list, plus a stored value that is not on it (Y9).
fn choices(list: &[&str], stored: &str) -> Vec<String> {
    let mut all: Vec<String> = list.iter().map(|s| s.to_string()).collect();
    if !stored.is_empty() && !all.iter().any(|s| s == stored) {
        all.push(stored.to_owned());
    }
    all
}

fn card(
    conn: &rusqlite::Connection,
    id: Option<String>,
    input: PaymentInput,
    errors: Vec<FieldError>,
) -> Result<Card, AppError> {
    let customer = payments::customer_name(conn, &input.customer_id)?;
    let mut invoices = payments::open_invoices(conn, &input.customer_id)?;
    // The payment's own invoices stay on the card when they are paid in full.
    for (invoice, _) in &input.lines {
        if !invoices.iter().any(|i| i.id == *invoice) {
            invoices.extend(payments::invoice_row(conn, invoice)?);
        }
    }
    let rows = invoices
        .into_iter()
        .map(|i| CardRow {
            amount: input.lines.iter().find(|(id, _)| *id == i.id).map(|(_, a)| a.clone()).unwrap_or_default(),
            balance_input: Cents(i.balance).to_input_value(),
            invoice_id: i.id,
            doc_number: i.doc_number,
            date: display_date(&i.date),
            original: usd(i.total),
            balance: usd(i.balance),
        })
        .collect();
    Ok(Card {
        customer,
        rows,
        methods: choices(&METHODS, &input.method),
        deposits: choices(&DEPOSIT_ACCOUNTS, &input.deposit),
        errors,
        id,
        input,
    })
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct NewQuery {
    invoice_id: String,
}

/// `GET /payments/new?invoice_id=`: amount = the invoice's balance, date today, one line at that balance (kontacts 276
/// s26-32). An invoice with no customer is refused with the source's text (kontacts 221 s3-4).
pub async fn new_card(State(state): State<AppState>, Query(q): Query<NewQuery>) -> Result<Response, AppError> {
    let today = state.today();
    // A payment starts from an invoice (kontacts 221): the source has no New on the payments list (layout 126).
    if q.invoice_id.is_empty() {
        return Ok((StatusCode::UNPROCESSABLE_ENTITY, "Open a payment from its invoice.").into_response());
    }
    let conn = lock(&state)?;
    let Some(invoice) = payments::invoice_row(&conn, &q.invoice_id)? else {
        return Err(AppError::NotFound);
    };
    let Some(Some(customer_id)) = payments::customer_of(&conn, &q.invoice_id)? else {
        return Ok((StatusCode::UNPROCESSABLE_ENTITY, NO_CUSTOMER).into_response());
    };
    let balance = Cents(invoice.balance).to_input_value();
    let input = PaymentInput {
        customer_id,
        date: today,
        amount: balance.clone(),
        lines: vec![(invoice.id, balance)],
        ..Default::default()
    };
    Ok(Html(card(&conn, None, input, Vec::new())?.render()?).into_response())
}

/// `GET /payments/{id}/edit`: the same card (kontacts 216).
pub async fn edit_card(State(state): State<AppState>, Path(id): Path<String>) -> Result<Response, AppError> {
    let conn = lock(&state)?;
    let input = match payments::payment_get(&conn, &id) {
        Ok(p) => p,
        Err(e) => return command_error("payment_get", e),
    };
    Ok(Html(card(&conn, Some(id), input, Vec::new())?.render()?).into_response())
}

fn input_of(form: &FormPairs) -> Result<PaymentInput, String> {
    let invoices = form.all("line_invoice");
    let amounts = form.all("line_amount");
    if invoices.len() != amounts.len() {
        return Err("Every line needs an invoice and an amount.".into());
    }
    Ok(PaymentInput {
        customer_id: form.one("customer_id"),
        date: form.one("date"),
        amount: form.one("amount"),
        method: form.one("method"),
        reference: form.one("reference"),
        deposit: form.one("deposit"),
        lines: invoices.into_iter().zip(amounts).collect(),
    })
}

fn save(state: &AppState, id: Option<String>, form: FormPairs) -> Result<Response, AppError> {
    if let Err(refused) = form.only(FIELDS) {
        return Ok(*refused);
    }
    let input = match input_of(&form) {
        Ok(i) => i,
        Err(m) => return Ok((StatusCode::UNPROCESSABLE_ENTITY, m).into_response()),
    };
    let mut conn = lock(state)?;
    match payments::payment_save(&mut conn, id.as_deref(), &input) {
        // The page behind the card shows the new balance.
        Ok(_) => Ok(([("HX-Trigger", toast("Payment saved", true))], [("HX-Refresh", "true")], "").into_response()),
        Err(CommandError::Validation(errors)) => {
            let view = card(&conn, id, input, errors)?;
            Ok((StatusCode::UNPROCESSABLE_ENTITY, Html(view.render()?)).into_response())
        }
        Err(e) => command_error("payment_save", e),
    }
}

pub async fn create(
    State(state): State<AppState>,
    _user: super::current_user::CurrentUser,
    form: FormPairs,
) -> Result<Response, AppError> {
    save(&state, None, form)
}

pub async fn update(
    State(state): State<AppState>,
    _user: super::current_user::CurrentUser,
    Path(id): Path<String>,
    form: FormPairs,
) -> Result<Response, AppError> {
    save(&state, Some(id), form)
}

/// One payment of an invoice's panel.
pub struct PaymentRowView {
    pub payment_id: String,
    pub date: String,
    pub reference: String,
    pub amount: String,
}

/// The Payments panel of the invoice page (layouts 69, 143). "Receive Payment" shows only on a posted invoice (layout
/// 143 has the button, layouts 69, 94 do not).
#[derive(Template)]
#[template(path = "sales/payments_panel.html")]
pub struct PaymentsPanel {
    pub invoice_id: String,
    pub can_receive: bool,
    pub rows: Vec<PaymentRowView>,
}

impl PaymentsPanel {
    pub fn load(conn: &rusqlite::Connection, invoice_id: &str, can_receive: bool) -> Result<Self, AppError> {
        let rows = payments::payments_of_invoice(conn, invoice_id)?
            .into_iter()
            .map(|p: InvoicePayment| PaymentRowView {
                payment_id: p.payment_id,
                date: display_date(&p.date),
                reference: p.reference,
                amount: usd(p.amount),
            })
            .collect();
        Ok(Self { invoice_id: invoice_id.to_owned(), can_receive, rows })
    }
}
