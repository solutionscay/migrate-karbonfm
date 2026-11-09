//! The optional integrations of I6 (05 OP-INTEG-02, OP-INTEG-03, OP-INTEG-04; 07 BR-INTEG-02, BR-INTEG-03; 08 AP-12).
//!
//! - Address suggestions (EXT-02, OP-INTEG-02 `address_suggest`): Google Places or SmartyStreets, one at a time, as the
//!   source's Picker::AddressServiceProvider (Picker 203). Google answers with places, and the picked place's address
//!   comes from a second call to the same host (Picker 200); a SmartyStreets suggestion already holds the address.
//! - FullContact person enrichment (EXT-03, OP-INTEG-03 `party_enrich`): kontroller 746 and kontacts 865. The call
//!   changes nothing; the user picks what to keep (the source's Keep checkboxes) and [`party_enrich_apply`] writes it
//!   in one transaction (kontacts 393 s166-196).
//! - Accounting (EXT-04, OP-INTEG-04): [`accounting_integration_status`] says "not implemented" (BR-INTEG-03, D-02).
//!
//! Every call needs the switch of its integration on in Settings and its keys in the keychain (AP-12, AP-03); the
//! routes check both before a call, so an off integration reads no key and calls nothing (T-08-12). The call's
//! policy allows the one host of that integration (AP-02, T-08-02). No retry; a failure changes nothing.

use crate::admin::{CommandError, FieldError, SystemSettings};
use crate::applog::{log_write, Entry, Level};
use crate::audit::{self, Actor, Stamp};
use crate::contacts::{MethodInput, MethodKind};
use crate::net::{HttpError, HttpRequest, HttpsClient, Integration, OutboundPolicy};
use crate::secrets::{Secret, SecretError, SecretKey, SecretStore};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde_json::Value;

/// The address suggestion services of Picker::AddressServiceProvider.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressProvider {
    GooglePlaces,
    SmartyStreets,
}

impl AddressProvider {
    pub const ALL: [AddressProvider; 2] = [AddressProvider::GooglePlaces, AddressProvider::SmartyStreets];

    /// The value in system_setting.address_suggestion_provider and in the settings form (the source's text).
    pub fn stored(self) -> &'static str {
        match self {
            AddressProvider::GooglePlaces => "Google Places",
            AddressProvider::SmartyStreets => "SmartyStreets",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.stored() == text.trim())
    }

    pub fn integration(self) -> Integration {
        match self {
            AddressProvider::GooglePlaces => Integration::GooglePlaces,
            AddressProvider::SmartyStreets => Integration::SmartyStreets,
        }
    }
}

/// The keys an integration needs, with their labels on the settings page.
pub fn required_keys(integration: Integration) -> &'static [(SecretKey, &'static str)] {
    match integration {
        Integration::GooglePlaces => &[(SecretKey::GooglePlacesApiKey, "Google Places API key")],
        Integration::SmartyStreets => &[
            (SecretKey::SmartyStreetsAuthId, "SmartyStreets auth ID"),
            (SecretKey::SmartyStreetsApiKey, "SmartyStreets auth token"),
        ],
        Integration::FullContact => &[(SecretKey::FullContactApiKey, "FullContact API key")],
    }
}

/// The integration of an address search, when the user turned one on (AP-12). Reads no key.
pub fn address_integration(s: &SystemSettings) -> Option<AddressProvider> {
    s.address_provider
}

/// Whether FullContact enrichment is on (AP-12). Reads no key.
pub fn enrich_on(s: &SystemSettings) -> bool {
    s.fullcontact_enabled
}

/// The keys of `integration` in `required_keys` order, or [`IntegError::NoKey`] naming the first one missing. Blocks
/// (keychain): call it from `spawn_blocking`.
pub fn read_keys(store: &dyn SecretStore, integration: Integration) -> Result<Vec<Secret>, IntegError> {
    let mut keys = Vec::new();
    for (key, label) in required_keys(integration) {
        match store.get(*key) {
            Ok(Some(secret)) if !secret.expose().trim().is_empty() => keys.push(secret),
            Ok(_) | Err(SecretError::Unavailable(_)) => return Err(IntegError::NoKey(label)),
            Err(e) => return Err(IntegError::Failed(e.to_string())),
        }
    }
    Ok(keys)
}

/// The policy of one call: only the host of `integration` (AP-02).
pub fn policy_for(integration: Integration) -> OutboundPolicy {
    OutboundPolicy::deny_all().enable(integration)
}

/// Why an integration gave no answer. [`IntegError::message`] is the text the user sees: never a key or a URL.
#[derive(Debug)]
pub enum IntegError {
    /// Turned off in Settings: no call was made (AP-12).
    Off(Integration),
    /// On, but a key is missing: no call was made.
    NoKey(&'static str),
    /// The outbound policy refused the host: nothing was sent (AP-02).
    Refused(String),
    /// No answer: no connection, TLS refused, timeout.
    Failed(String),
    /// The service answered with an error status (a bad key, no credit, too many calls).
    Rejected { integration: Integration, status: u16, detail: String },
    /// The service answered with something the app cannot read.
    BadAnswer(Integration),
    /// The input cannot be sent (kontacts 865 s4: a name and an email or phone are needed).
    Validation(Vec<FieldError>),
}

impl IntegError {
    pub fn message(&self) -> String {
        match self {
            IntegError::Off(i) => format!("{} is turned off. Turn it on in Settings and save its key.", i.label()),
            IntegError::NoKey(label) => format!("No {label} is saved. Add it in Settings."),
            IntegError::Refused(_) => "The call was blocked: that service is not turned on.".into(),
            IntegError::Failed(_) => "The service could not be reached. Check the connection and try again.".into(),
            IntegError::Rejected { integration, status, detail } => {
                let why = match status {
                    401 | 403 => "it did not accept the key. Check the key in Settings.".to_owned(),
                    402 => "the account has no credit left.".to_owned(),
                    429 => "too many calls. Wait a moment and try again.".to_owned(),
                    _ if !detail.is_empty() => format!("{detail} (HTTP {status})."),
                    _ => format!("it answered with an error (HTTP {status})."),
                };
                format!("{} refused the call: {why}", integration.label())
            }
            IntegError::BadAnswer(i) => format!("{} sent an answer KarbonFM cannot read.", i.label()),
            IntegError::Validation(errors) => errors.iter().map(|e| e.message.as_str()).collect::<Vec<_>>().join(" "),
        }
    }
}

impl From<HttpError> for IntegError {
    fn from(e: HttpError) -> Self {
        match e {
            HttpError::Refused(r) => IntegError::Refused(r.to_string()),
            HttpError::Failed(m) => IntegError::Failed(m),
        }
    }
}

/// Sends one call and returns the JSON of a 2xx answer, `None` for a 404 (no match), or the error. A rejection is
/// logged at warn with the service and the status (no key, no URL).
async fn call(
    client: &HttpsClient,
    integration: Integration,
    req: HttpRequest,
    not_found_is_none: bool,
) -> Result<Option<Value>, IntegError> {
    let res = client.send(&policy_for(integration), req).await?;
    if res.status == 404 && not_found_is_none {
        return Ok(None);
    }
    let json: Option<Value> = serde_json::from_slice(&res.body).ok();
    if !(200..300).contains(&res.status) {
        let detail = json.as_ref().map(error_detail).unwrap_or_default();
        log_write(&Entry {
            level: Level::Warn,
            operation: "outbound https",
            code: Some("Integration"),
            message: &format!("{} answered HTTP {}", integration.host(), res.status),
            payload: None,
        });
        return Err(IntegError::Rejected { integration, status: res.status, detail });
    }
    json.map(Some).ok_or(IntegError::BadAnswer(integration))
}

/// The error text of a Google (`error.message`), FullContact (`message`) or SmartyStreets (`errors[0].message`)
/// answer, cut to one short line.
fn error_detail(json: &Value) -> String {
    let text = json
        .pointer("/error/message")
        .or_else(|| json.get("message"))
        .or_else(|| json.pointer("/errors/0/message"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let text = text.trim().trim_end_matches('.');
    text.chars().take(160).collect()
}

/// Percent-encodes a query value (RFC 3986 unreserved characters stay).
pub fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn text(v: &Value, pointer: &str) -> String {
    v.pointer(pointer).and_then(Value::as_str).unwrap_or_default().trim().to_owned()
}

// ---------------------------------------------------------------------------------------------------------------
// OP-INTEG-02 address_suggest
// ---------------------------------------------------------------------------------------------------------------

/// The longest search text: SmartyStreets takes 128 bytes (Picker 117 s7).
pub const MAX_QUERY: usize = 128;

/// The fields of an address contact method that a suggestion fills (UI-06 address picker).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AddressFields {
    pub line1: String,
    pub line2: String,
    pub city: String,
    pub state: String,
    pub postal_code: String,
    pub country: String,
}

/// What picking a suggestion does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// A Google place: its address comes from a details call (Picker 200).
    Place(String),
    /// A SmartyStreets suggestion: the address itself (US Autocomplete Pro answers with the ZIP code).
    Address(AddressFields),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressSuggestion {
    pub text: String,
    pub pick: Pick,
}

/// Checks the search text: trimmed, at most [`MAX_QUERY`] bytes. Empty means "no call" (Picker 117 s50).
pub fn check_query(q: &str) -> Result<&str, IntegError> {
    let q = q.trim();
    if q.len() > MAX_QUERY {
        return Err(IntegError::Validation(vec![FieldError {
            field: "q",
            message: format!("Type at most {MAX_QUERY} characters."),
        }]));
    }
    Ok(q)
}

/// OP-INTEG-02 `address_suggest` (Picker 117, 198, 203): the suggestions for `q` from `provider`, with its `keys` in
/// [`required_keys`] order. An empty `q` makes no call.
pub async fn address_suggest(
    client: &HttpsClient,
    provider: AddressProvider,
    keys: &[Secret],
    q: &str,
) -> Result<Vec<AddressSuggestion>, IntegError> {
    let q = check_query(q)?;
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let integration = provider.integration();
    match provider {
        AddressProvider::GooglePlaces => {
            // Places API (New) autocomplete: the source's legacy maps/api/place/autocomplete is retired.
            let req = HttpRequest::post_json(
                format!("https://{}/v1/places:autocomplete", integration.host()),
                serde_json::json!({ "input": q }),
            )
            .header("X-Goog-Api-Key", keys[0].expose());
            let json = call(client, integration, req, false).await?.unwrap_or_default();
            let list = match json.get("suggestions") {
                None => return Ok(Vec::new()),
                Some(Value::Array(list)) => list,
                Some(_) => return Err(IntegError::BadAnswer(integration)),
            };
            Ok(list
                .iter()
                .filter_map(|s| {
                    let place_id = text(s, "/placePrediction/placeId");
                    let label = text(s, "/placePrediction/text/text");
                    (valid_place_id(&place_id) && !label.is_empty())
                        .then_some(AddressSuggestion { text: label, pick: Pick::Place(place_id) })
                })
                .collect())
        }
        AddressProvider::SmartyStreets => {
            // US Autocomplete Pro (the source's us-autocomplete.api.smartystreets.com/suggest is retired); the auth ID
            // and token travel in the query, as in Picker 117 s61.
            let url = format!(
                "https://{}/lookup?search={}&auth-id={}&auth-token={}",
                integration.host(),
                encode(q),
                encode(keys[0].expose()),
                encode(keys[1].expose())
            );
            let json = call(client, integration, HttpRequest::get(url), false).await?.unwrap_or_default();
            let list = match json.get("suggestions") {
                None | Some(Value::Null) => return Ok(Vec::new()),
                Some(Value::Array(list)) => list,
                Some(_) => return Err(IntegError::BadAnswer(integration)),
            };
            Ok(list
                .iter()
                .filter_map(|s| {
                    let a = AddressFields {
                        line1: text(s, "/street_line"),
                        line2: text(s, "/secondary"),
                        city: text(s, "/city"),
                        state: text(s, "/state"),
                        postal_code: text(s, "/zipcode"),
                        country: "US".into(),
                    };
                    if a.line1.is_empty() {
                        return None;
                    }
                    let mut label = a.line1.clone();
                    if !a.line2.is_empty() {
                        label = format!("{label} {}", a.line2);
                    }
                    let label = format!("{label}, {} {} {}", a.city, a.state, a.postal_code).trim().to_owned();
                    Some(AddressSuggestion { text: label, pick: Pick::Address(a) })
                })
                .collect())
        }
    }
}

/// A Google place id: letters, digits, `-` and `_` (it goes into the URL path of the details call).
pub fn valid_place_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 512 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The address of a Google place (Picker 200): street number and route, subpremise, locality (else postal town, then
/// neighborhood, as the source's 2018 fix), state code, postal code with its suffix, country code.
pub async fn google_place_address(
    client: &HttpsClient,
    keys: &[Secret],
    place_id: &str,
) -> Result<AddressFields, IntegError> {
    let integration = Integration::GooglePlaces;
    if !valid_place_id(place_id) {
        return Err(IntegError::Validation(vec![FieldError {
            field: "place_id",
            message: "Pick a suggestion from the list.".into(),
        }]));
    }
    let req = HttpRequest::get(format!("https://{}/v1/places/{place_id}", integration.host()))
        .header("X-Goog-Api-Key", keys[0].expose())
        .header("X-Goog-FieldMask", "addressComponents");
    let json = call(client, integration, req, false).await?.unwrap_or_default();
    let Some(Value::Array(parts)) = json.get("addressComponents") else {
        return Err(IntegError::BadAnswer(integration));
    };
    let find = |kind: &str, short: bool| -> String {
        parts
            .iter()
            .find(|p| {
                p.get("types").and_then(Value::as_array).is_some_and(|t| t.iter().any(|t| t.as_str() == Some(kind)))
            })
            .map(|p| text(p, if short { "/shortText" } else { "/longText" }))
            .unwrap_or_default()
    };
    let line1 = [find("street_number", false), find("route", false)]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let city = ["locality", "postal_town", "sublocality", "neighborhood"]
        .into_iter()
        .map(|k| find(k, false))
        .find(|s| !s.is_empty())
        .unwrap_or_default();
    let mut postal_code = find("postal_code", false);
    let suffix = find("postal_code_suffix", false);
    if !postal_code.is_empty() && !suffix.is_empty() {
        postal_code = format!("{postal_code}-{suffix}");
    }
    Ok(AddressFields {
        line1,
        line2: find("subpremise", false),
        city,
        state: find("administrative_area_level_1", true),
        postal_code,
        country: find("country", true),
    })
}

// ---------------------------------------------------------------------------------------------------------------
// OP-INTEG-03 party_enrich
// ---------------------------------------------------------------------------------------------------------------

/// What FullContact is asked (kontroller 746 s120): the person's names, emails and phones.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnrichQuery {
    pub given: String,
    pub family: String,
    pub emails: Vec<String>,
    pub phones: Vec<String>,
}

/// What FullContact found (kontacts 865 s50-57). The avatar is not offered: it is on another host
/// (img.fullcontact.com), and each integration calls only its own host.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PersonMatch {
    pub full_name: String,
    pub job_title: String,
    pub gender: String,
    pub bio: String,
    pub linkedin: String,
    pub twitter: String,
    pub facebook: String,
}

impl PersonMatch {
    pub fn is_empty(&self) -> bool {
        [&self.job_title, &self.gender, &self.bio, &self.linkedin, &self.twitter, &self.facebook]
            .iter()
            .all(|s| s.is_empty())
    }
}

/// The current values of a person party that enrichment may fill, and what it is asked with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnrichTarget {
    pub name: String,
    pub job_title: String,
    pub gender: String,
    pub bio: String,
    /// The party's web addresses (lowercase, no trailing slash), so a found link already there is not offered.
    pub web: Vec<String>,
    pub query: EnrichQuery,
}

/// A phone as FullContact wants it (E.164; kontacts 865 s26): a US number of 10 digits gets +1, a number typed with +
/// keeps its digits, anything else is sent as its digits.
pub fn e164(number: &str, country_code: Option<i64>) -> String {
    let digits = crate::contacts::digits_only(number);
    if number.trim_start().starts_with('+') {
        return format!("+{digits}");
    }
    match (country_code, digits.len()) {
        (Some(cc), 10) => format!("+{cc}{digits}"),
        (Some(1), 11) if digits.starts_with('1') => format!("+{digits}"),
        _ => digits,
    }
}

/// Normal form of a web address for the duplicate check.
pub fn web_key(address: &str) -> String {
    let a = address.trim().to_lowercase();
    let a = a.strip_prefix("https://").or_else(|| a.strip_prefix("http://")).unwrap_or(&a);
    let a = a.strip_prefix("www.").unwrap_or(a);
    a.trim_end_matches('/').to_owned()
}

/// The person party `party_id` as enrichment sees it. NotFound for a missing party; Refused for an organization or an
/// archived party.
pub fn enrich_target(conn: &Connection, party_id: &str) -> Result<EnrichTarget, CommandError> {
    let d = crate::parties::party_get(conn, party_id)?;
    if d.archived {
        return Err(CommandError::Refused("This party is archived.".into()));
    }
    let Some(p) = &d.person else {
        return Err(CommandError::Refused("Enrichment looks up people; this party is an organization.".into()));
    };
    let (country_code, _) = crate::contacts::phone_settings(conn)?;
    let bio: Option<String> =
        conn.query_row("SELECT bio FROM person WHERE id = ?1", [party_id], |r| r.get(0)).optional()?.flatten();
    let gender: Option<String> =
        conn.query_row("SELECT gender FROM person WHERE id = ?1", [party_id], |r| r.get(0)).optional()?.flatten();
    let mut t = EnrichTarget {
        name: d.name.clone(),
        job_title: p.job_title.clone(),
        gender: gender.unwrap_or_default(),
        bio: bio.unwrap_or_default(),
        ..Default::default()
    };
    t.query.given = p.first_name.trim().to_owned();
    t.query.family = p.last_name.trim().to_owned();
    for m in &d.methods {
        match m.kind {
            MethodKind::Email => t.query.emails.push(m.address.trim().to_owned()),
            MethodKind::Phone => t.query.phones.push(e164(&m.number, m.country_code.or(country_code))),
            MethodKind::Web => t.web.push(web_key(&m.address)),
            MethodKind::Address => {}
        }
    }
    Ok(t)
}

/// kontacts 865 s3-7: a name, and an email or a phone, or no call.
pub fn check_enrich_query(q: &EnrichQuery) -> Result<(), IntegError> {
    let name = format!("{} {}", q.given, q.family);
    if name.trim().is_empty() || (q.emails.is_empty() && q.phones.is_empty()) {
        return Err(IntegError::Validation(vec![FieldError {
            field: "party",
            message: "FullContact needs the person's name and an email or a phone. Add one first.".into(),
        }]));
    }
    Ok(())
}

/// OP-INTEG-03 `party_enrich`, the call (kontroller 746): FullContact v3 person.enrich with a Bearer key. `None` when
/// FullContact has no match (404, or 202: it has no answer yet). Changes nothing.
pub async fn person_enrich(
    client: &HttpsClient,
    keys: &[Secret],
    q: &EnrichQuery,
) -> Result<Option<PersonMatch>, IntegError> {
    check_enrich_query(q)?;
    let integration = Integration::FullContact;
    let full = format!("{} {}", q.given, q.family).trim().to_owned();
    let body = serde_json::json!({
        "emails": q.emails,
        "phones": q.phones,
        "name": { "given": q.given, "family": q.family, "full": full },
    });
    let req = HttpRequest::post_json(format!("https://{}/v3/person.enrich", integration.host()), body)
        .header("Authorization", format!("Bearer {}", keys[0].expose()));
    let res = client.send(&policy_for(integration), req).await?;
    if res.status == 202 || res.status == 404 {
        return Ok(None);
    }
    let json: Option<Value> = serde_json::from_slice(&res.body).ok();
    if !(200..300).contains(&res.status) {
        let detail = json.as_ref().map(error_detail).unwrap_or_default();
        log_write(&Entry {
            level: Level::Warn,
            operation: "outbound https",
            code: Some("Integration"),
            message: &format!("{} answered HTTP {}", integration.host(), res.status),
            payload: None,
        });
        return Err(IntegError::Rejected { integration, status: res.status, detail });
    }
    let json = json.filter(Value::is_object).ok_or(IntegError::BadAnswer(integration))?;
    let found = PersonMatch {
        full_name: text(&json, "/fullName"),
        job_title: text(&json, "/title"),
        gender: text(&json, "/gender"),
        bio: text(&json, "/bio"),
        linkedin: text(&json, "/linkedin"),
        twitter: text(&json, "/twitter"),
        facebook: text(&json, "/facebook"),
    };
    Ok((!found.is_empty()).then_some(found))
}

/// What the user kept from a match (the source's FullContactPersonKeep* checkboxes). Empty fields are not written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnrichChoice {
    pub job_title: Option<String>,
    pub gender: Option<String>,
    pub bio: Option<String>,
    /// (label, address): LinkedIn, Twitter, Facebook as web addresses, not primary (kontacts 393 s176-190).
    pub web: Vec<(&'static str, String)>,
}

impl EnrichChoice {
    pub fn is_empty(&self) -> bool {
        self.job_title.is_none() && self.gender.is_none() && self.bio.is_none() && self.web.is_empty()
    }
}

/// OP-INTEG-03 `party_enrich`, the write (kontacts 393 s166-196): the kept values on the person, the kept links as
/// web addresses (a link the party already has is skipped), in one transaction with the stamps (BR-PARTY-07).
/// Returns how many values it wrote.
pub fn party_enrich_apply(
    conn: &mut Connection,
    party_id: &str,
    choice: &EnrichChoice,
    actor: &Actor,
) -> Result<usize, CommandError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let target = enrich_target(&tx, party_id)?;
    let stamp = Stamp::now(actor);
    let mut written = 0;
    for (column, value) in [("job_title", &choice.job_title), ("gender", &choice.gender), ("bio", &choice.bio)] {
        if let Some(v) = value.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            tx.execute(&format!("UPDATE person SET {column} = ?2 WHERE id = ?1"), params![party_id, v])?;
            written += 1;
        }
    }
    if written > 0 {
        audit::stamp_modified(&tx, "person", &party_id, &stamp)?;
    }
    let mut have = target.web.clone();
    for (label, address) in &choice.web {
        let key = web_key(address);
        if key.is_empty() || have.contains(&key) {
            continue;
        }
        let input = MethodInput {
            kind: Some(MethodKind::Web),
            address: address.trim().into(),
            label: (*label).into(),
            ..Default::default()
        };
        crate::contacts::insert_method(&tx, party_id, &input, &stamp)?;
        have.push(key);
        written += 1;
    }
    if written > 0 {
        crate::parties::refresh_caches(&tx, party_id)?;
        audit::stamp_modified(&tx, "party", &party_id, &stamp)?;
    }
    tx.commit()?;
    Ok(written)
}

// ---------------------------------------------------------------------------------------------------------------
// OP-INTEG-04 accounting_integration_status (05 "Accounting integration stub")
// ---------------------------------------------------------------------------------------------------------------

/// The accounting stub's answer (EXT-04; BR-INTEG-03, D-02).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountingStatus {
    pub provider: Option<String>,
    pub implemented: bool,
    pub message: String,
}

/// OP-INTEG-04 `accounting_integration_status`: QuickBooks Online is out of scope (D-02). It reads and writes nothing.
pub fn accounting_integration_status() -> AccountingStatus {
    AccountingStatus { provider: None, implemented: false, message: "Accounting integration is not implemented".into() }
}

/// An external accounting reference (external_link, type 5 = QBO) that a later integration would return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalRef {
    pub external_link_type_id: i64,
    pub external_id: String,
}

/// Extension point for a later accounting integration (05; not implemented, no implementor ships).
pub trait AccountingProvider {
    fn push_sales_transaction(&self, id: &str) -> Result<ExternalRef, CommandError>;
    fn push_customer(&self, party_role_id: &str) -> Result<ExternalRef, CommandError>;
    fn push_payment(&self, id: &str) -> Result<ExternalRef, CommandError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn providers_round_trip_and_unknown_is_off() {
        for p in AddressProvider::ALL {
            assert_eq!(AddressProvider::parse(p.stored()), Some(p));
        }
        assert_eq!(AddressProvider::parse(""), None);
        assert_eq!(AddressProvider::parse("Bing"), None);
    }

    #[test]
    fn encode_keeps_unreserved_only() {
        assert_eq!(encode("12 Fore St, #3&x=1"), "12%20Fore%20St%2C%20%233%26x%3D1");
        assert_eq!(encode("Tomás"), "Tom%C3%A1s");
    }

    #[test]
    fn place_ids_are_checked() {
        assert!(valid_place_id("ChIJN1t_tDeuEmsRUsoyG83frY4"));
        assert!(!valid_place_id("../x"));
        assert!(!valid_place_id("a?key=1"));
        assert!(!valid_place_id(""));
    }

    #[test]
    fn query_is_trimmed_and_capped() {
        assert_eq!(check_query("  12 Fore ").unwrap(), "12 Fore");
        assert!(matches!(check_query(&"x".repeat(129)), Err(IntegError::Validation(_))));
    }

    #[test]
    fn phones_go_as_e164() {
        assert_eq!(e164("(207) 555-0101", Some(1)), "+12075550101");
        assert_eq!(e164("+44 20 7946 0958", Some(1)), "+442079460958");
        assert_eq!(e164("1 207 555 0101", Some(1)), "+12075550101");
        assert_eq!(e164("555-0101", Some(1)), "5550101");
    }

    #[test]
    fn web_key_ignores_scheme_www_case_and_slash() {
        assert_eq!(web_key("https://www.LinkedIn.com/in/x/"), "linkedin.com/in/x");
        assert_eq!(web_key("linkedin.com/in/x"), "linkedin.com/in/x");
    }

    #[test]
    fn enrich_needs_a_name_and_an_email_or_phone() {
        let mut q = EnrichQuery { given: "Elena".into(), ..Default::default() };
        assert!(check_enrich_query(&q).is_err());
        q.emails.push("elena@example.com".into());
        assert!(check_enrich_query(&q).is_ok());
        q.given.clear();
        assert!(check_enrich_query(&q).is_err());
    }

    #[test]
    fn messages_never_carry_keys_or_urls() {
        let e = IntegError::Rejected { integration: Integration::GooglePlaces, status: 403, detail: String::new() };
        assert_eq!(
            e.message(),
            "Google Places refused the call: it did not accept the key. Check the key in Settings."
        );
        let e = IntegError::Failed("could not connect (error sending request)".into());
        assert!(!e.message().contains("http"));
        assert_eq!(
            IntegError::Off(Integration::FullContact).message(),
            "FullContact is turned off. Turn it on in Settings and save its key."
        );
    }

    #[test]
    fn the_accounting_stub_is_not_implemented() {
        let s = accounting_integration_status();
        assert!(!s.implemented && s.provider.is_none());
        assert_eq!(s.message, "Accounting integration is not implemented");
    }
}
