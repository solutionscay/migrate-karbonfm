//! Routes of the optional integrations (05 OP-INTEG-02, OP-INTEG-03, OP-INTEG-04; AP-12, AP-13).
//!
//! - `GET /address-suggestions?q=` and `GET /address-suggestions/pick`: the UI-06 address picker of the address dialog.
//! - `POST /parties/{id}/enrich` and `POST /parties/{id}/enrich/apply`: FullContact on a person's page (UI-04).
//! - `GET /integrations/accounting`: the accounting stub's status line (UI-17).
//!
//! Each handler reads the settings first. An integration that is off gets no keychain read and no call (T-08-12); one
//! that is on reads its keys off the async workers, then calls through `AppState::http` with the policy of that one
//! host (AP-02). A failure is a 422 fragment with the reason, and nothing is written (05: error fragment with status).

use super::current_user::CurrentUser;
use super::parties::{page_error, party_href, Header};
use super::party_forms::toast;
use crate::admin::{self, CommandError};
use crate::integrations::{
    self, AddressFields, AddressProvider, EnrichChoice, EnrichTarget, IntegError, PersonMatch, Pick,
};
use crate::net::Integration;
use crate::secrets::Secret;
use crate::web::{urlencode, AppError};
use crate::AppState;
use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::Form;
use serde::Deserialize;

fn lock(state: &AppState) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>, AppError> {
    state.db.lock().map_err(|e| AppError::Internal(e.to_string()))
}

/// The keys of an integration that is on, read off the async workers (keychain calls block).
async fn keys(state: &AppState, integration: Integration) -> Result<Result<Vec<Secret>, IntegError>, AppError> {
    let store = state.secret_store();
    tokio::task::spawn_blocking(move || integrations::read_keys(&store, integration))
        .await
        .map_err(|e| AppError::Internal(format!("keychain task: {e}")))
}

fn fragment(view: impl Template, status: StatusCode) -> Result<Response, AppError> {
    Ok((status, Html(view.render()?)).into_response())
}

// ---------------------------------------------------------------------------------------------------------------
// OP-INTEG-02 address_suggest
// ---------------------------------------------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "integrations/address_suggestions.html")]
struct SuggestionList {
    q: String,
    /// (text, query string of the pick URL).
    items: Vec<(String, String)>,
    error: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct SuggestQuery {
    q: String,
}

/// The query string that picks one suggestion: a Google place id, or a SmartyStreets address itself.
fn pick_query(pick: &Pick) -> String {
    match pick {
        Pick::Place(id) => format!("place_id={}", urlencode(id)),
        Pick::Address(a) => [
            ("line1", &a.line1),
            ("line2", &a.line2),
            ("city", &a.city),
            ("state", &a.state),
            ("postal_code", &a.postal_code),
            ("country", &a.country),
        ]
        .iter()
        .map(|(k, v)| format!("{k}={}", urlencode(v)))
        .collect::<Vec<_>>()
        .join("&"),
    }
}

/// The address service that is on, or the error of an off one (AP-12). Reads no key.
fn address_provider(state: &AppState) -> Result<Result<AddressProvider, IntegError>, AppError> {
    let settings = admin::settings_get(&*lock(state)?)?;
    // Off: which service is off does not matter to the user; Google Places names the feature.
    Ok(integrations::address_integration(&settings).ok_or(IntegError::Off(Integration::GooglePlaces)))
}

fn off_message(e: &IntegError) -> String {
    match e {
        IntegError::Off(_) => "Address suggestions are turned off. Turn them on in Settings.".into(),
        e => e.message(),
    }
}

/// `GET /address-suggestions?q=` (OP-INTEG-02 `address_suggest`): the suggestion list fragment.
pub async fn suggest(State(state): State<AppState>, Query(query): Query<SuggestQuery>) -> Result<Response, AppError> {
    let q = query.q.trim().to_owned();
    let failed = |q: String, e: IntegError| {
        fragment(
            SuggestionList { q, items: Vec::new(), error: Some(off_message(&e)) },
            StatusCode::UNPROCESSABLE_ENTITY,
        )
    };
    let provider = match address_provider(&state)? {
        Ok(p) => p,
        Err(e) => return failed(q, e),
    };
    if let Err(e) = integrations::check_query(&q) {
        return failed(q, e);
    }
    if q.is_empty() {
        return fragment(SuggestionList { q, items: Vec::new(), error: None }, StatusCode::OK);
    }
    let keys = match keys(&state, provider.integration()).await? {
        Ok(k) => k,
        Err(e) => return failed(q, e),
    };
    match integrations::address_suggest(state.http(), provider, &keys, &q).await {
        Ok(list) => {
            let items = list.iter().map(|s| (s.text.clone(), pick_query(&s.pick))).collect();
            fragment(SuggestionList { q, items, error: None }, StatusCode::OK)
        }
        Err(e) => failed(q, e),
    }
}

#[derive(Template)]
#[template(path = "integrations/address_pick.html")]
struct AddressPick {
    form: AddressFields,
    service: &'static str,
}

impl AddressPick {
    pub fn error(&self, _field: &str) -> Option<&str> {
        None
    }
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct PickQuery {
    place_id: String,
    line1: String,
    line2: String,
    city: String,
    state: String,
    postal_code: String,
    country: String,
}

/// `GET /address-suggestions/pick`: the address fields filled with the picked suggestion. A Google place is looked
/// up first (Picker 200); a SmartyStreets suggestion carries its address. On a failure the list shows the reason
/// (`HX-Retarget`) and the fields keep what was typed.
pub async fn pick(State(state): State<AppState>, Query(p): Query<PickQuery>) -> Result<Response, AppError> {
    let failed = |e: IntegError| -> Result<Response, AppError> {
        let html = SuggestionList { q: String::new(), items: Vec::new(), error: Some(off_message(&e)) }.render()?;
        Ok((
            StatusCode::UNPROCESSABLE_ENTITY,
            [("HX-Retarget", "#address-suggestions"), ("HX-Reswap", "innerHTML")],
            Html(html),
        )
            .into_response())
    };
    let provider = match address_provider(&state)? {
        Ok(p) => p,
        Err(e) => return failed(e),
    };
    let form = if p.place_id.is_empty() {
        AddressFields {
            line1: p.line1,
            line2: p.line2,
            city: p.city,
            state: p.state,
            postal_code: p.postal_code,
            country: p.country,
        }
    } else {
        if provider != AddressProvider::GooglePlaces {
            return failed(IntegError::Off(Integration::GooglePlaces));
        }
        let keys = match keys(&state, Integration::GooglePlaces).await? {
            Ok(k) => k,
            Err(e) => return failed(e),
        };
        match integrations::google_place_address(state.http(), &keys, &p.place_id).await {
            Ok(a) => a,
            Err(e) => return failed(e),
        }
    };
    fragment(AddressPick { form, service: provider.stored() }, StatusCode::OK)
}

// ---------------------------------------------------------------------------------------------------------------
// OP-INTEG-03 party_enrich
// ---------------------------------------------------------------------------------------------------------------

struct EnrichRow {
    field: &'static str,
    label: &'static str,
    found: String,
    current: String,
    checked: bool,
}

#[derive(Template)]
#[template(path = "integrations/enrich_dialog.html")]
struct EnrichDialog {
    href: String,
    name: String,
    found_name: String,
    rows: Vec<EnrichRow>,
    error: Option<String>,
}

impl EnrichDialog {
    fn failed(href: String, name: String, message: String) -> Self {
        EnrichDialog { href, name, found_name: String::new(), rows: Vec::new(), error: Some(message) }
    }

    /// The rows of a match: each value found, with the party's current one. A link the party has is not offered;
    /// a value the party has is unchecked (kontacts 393 s166: what was typed wins).
    fn of(href: String, t: &EnrichTarget, m: Option<PersonMatch>) -> Self {
        let mut rows = Vec::new();
        let found_name = m.as_ref().map(|m| m.full_name.clone()).unwrap_or_default();
        if let Some(m) = m {
            for (field, label, found, current) in [
                ("job_title", "Job title", m.job_title, t.job_title.clone()),
                ("gender", "Gender", m.gender, t.gender.clone()),
                ("bio", "Bio", m.bio, t.bio.clone()),
            ] {
                if !found.is_empty() && found != current {
                    rows.push(EnrichRow { field, label, checked: current.is_empty(), found, current });
                }
            }
            for (field, label, found) in [
                ("linkedin", "LinkedIn", m.linkedin),
                ("twitter", "Twitter", m.twitter),
                ("facebook", "Facebook", m.facebook),
            ] {
                if !found.is_empty() && !t.web.contains(&integrations::web_key(&found)) {
                    rows.push(EnrichRow { field, label, found, current: String::new(), checked: true });
                }
            }
        }
        EnrichDialog { href, name: t.name.clone(), found_name, rows, error: None }
    }
}

/// `POST /parties/{id}/enrich` (OP-INTEG-03 `party_enrich`, kontacts 865): asks FullContact about the person and
/// answers with what it found, in the dialog. Writes nothing.
pub async fn enrich(State(state): State<AppState>, Path(id): Path<String>) -> Result<Response, AppError> {
    let href = party_href(&id);
    let (on, target) = {
        let conn = lock(&state)?;
        let on = integrations::enrich_on(&admin::settings_get(&conn)?);
        let target = match integrations::enrich_target(&conn, &id) {
            Ok(t) => t,
            Err(CommandError::Refused(message)) => {
                let name = crate::parties::party_get(&conn, &id).map(|d| d.name).unwrap_or_default();
                return fragment(EnrichDialog::failed(href, name, message), StatusCode::UNPROCESSABLE_ENTITY);
            }
            Err(e) => return Err(page_error("party_enrich", e)),
        };
        (on, target)
    };
    let failed = |e: IntegError| {
        fragment(EnrichDialog::failed(href.clone(), target.name.clone(), e.message()), StatusCode::UNPROCESSABLE_ENTITY)
    };
    if !on {
        return failed(IntegError::Off(Integration::FullContact));
    }
    if let Err(e) = integrations::check_enrich_query(&target.query) {
        return failed(e);
    }
    let keys = match keys(&state, Integration::FullContact).await? {
        Ok(k) => k,
        Err(e) => return failed(e),
    };
    match integrations::person_enrich(state.http(), &keys, &target.query).await {
        Ok(found) => fragment(EnrichDialog::of(href.clone(), &target, found), StatusCode::OK),
        Err(e) => failed(e),
    }
}

/// The Keep boxes and the values of the dialog.
#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ApplyForm {
    keep_job_title: String,
    job_title: String,
    keep_gender: String,
    gender: String,
    keep_bio: String,
    bio: String,
    keep_linkedin: String,
    linkedin: String,
    keep_twitter: String,
    twitter: String,
    keep_facebook: String,
    facebook: String,
}

impl ApplyForm {
    fn choice(&self) -> EnrichChoice {
        let kept =
            |keep: &str, value: &str| (keep == "on" && !value.trim().is_empty()).then(|| value.trim().to_owned());
        let mut web = Vec::new();
        for (label, keep, value) in [
            ("LinkedIn", &self.keep_linkedin, &self.linkedin),
            ("Twitter", &self.keep_twitter, &self.twitter),
            ("Facebook", &self.keep_facebook, &self.facebook),
        ] {
            if let Some(v) = kept(keep, value) {
                web.push((label, v));
            }
        }
        EnrichChoice {
            job_title: kept(&self.keep_job_title, &self.job_title),
            gender: kept(&self.keep_gender, &self.gender),
            bio: kept(&self.keep_bio, &self.bio),
            web,
        }
    }
}

/// `POST /parties/{id}/enrich/apply` (OP-INTEG-03, kontacts 393 s166-196): writes the kept values in one transaction
/// and answers with the party header (out of band; the dialog closes). Makes no outbound call.
pub async fn enrich_apply(
    State(state): State<AppState>,
    Path(id): Path<String>,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<ApplyForm>,
) -> Result<Response, AppError> {
    let choice = form.choice();
    let mut conn = lock(&state)?;
    let written = if choice.is_empty() {
        crate::parties::party_get(&conn, &id).map_err(|e| page_error("party_get", e))?;
        0
    } else {
        match integrations::party_enrich_apply(&mut conn, &id, &choice, &actor) {
            Ok(n) => n,
            Err(CommandError::Refused(message)) => {
                let name = crate::parties::party_get(&conn, &id).map(|d| d.name).unwrap_or_default();
                return fragment(
                    EnrichDialog::failed(party_href(&id), name, message),
                    StatusCode::UNPROCESSABLE_ENTITY,
                );
            }
            Err(CommandError::Validation(errors)) => {
                let name = crate::parties::party_get(&conn, &id).map(|d| d.name).unwrap_or_default();
                let message = errors.into_iter().map(|e| e.message).collect::<Vec<_>>().join(" ");
                return fragment(
                    EnrichDialog::failed(party_href(&id), name, message),
                    StatusCode::UNPROCESSABLE_ENTITY,
                );
            }
            Err(e) => return Err(page_error("party_enrich", e)),
        }
    };
    let d = crate::parties::party_get(&conn, &id).map_err(|e| page_error("party_get", e))?;
    let header = Header { oob: true, ..Header::for_party(&conn, &d)? }.render()?;
    let text = match written {
        0 => "Nothing changed".to_owned(),
        1 => "Kept 1 value from FullContact".to_owned(),
        n => format!("Kept {n} values from FullContact"),
    };
    Ok(([("HX-Trigger", toast(&text, true))], Html(header)).into_response())
}

// ---------------------------------------------------------------------------------------------------------------
// OP-INTEG-04 accounting_integration_status
// ---------------------------------------------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "integrations/accounting.html")]
struct AccountingLine {
    line: &'static str,
}

/// `GET /integrations/accounting` (OP-INTEG-04, BR-INTEG-03): one status line, "Not implemented". Changes nothing.
pub async fn accounting() -> Result<Response, AppError> {
    fragment(AccountingLine { line: super::settings::accounting_line() }, StatusCode::OK)
}
