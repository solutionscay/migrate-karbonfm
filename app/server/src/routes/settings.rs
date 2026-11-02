//! UI-17 Settings (OP-ADMIN-03 `settings_get_update`, BR-ADMIN-05; secrets AP-03). `GET /settings` shows the form;
//! `PUT /settings` saves it and answers with the form again. The SMTP password and the integration API keys are
//! write-only `SecretField`s: the page gets only a has_secret flag, and a value goes straight to the keychain.
//! Keychain calls block (and may show an OS prompt), so they run on `spawn_blocking`, off the async workers.
//!
//! Without a keychain (D-09, issue #7) the page still opens: the secret fields are disabled, a notice offers the
//! opt-in `secrets.toml` (`secrets_store.rs`), and a secret typed anyway is a field error (422), never a 500 and
//! never "Settings saved".

use super::current_user::CurrentUser;
use super::secrets_store::StorePanel;
use super::Nav;
use crate::admin::{self, CommandError, FieldError, SystemSettings};
use crate::secrets::{SecretError, SecretField, SecretKey, SecretStore, Secrets, StoreKind};
use crate::web::{is_htmx, render, AppError};
use crate::AppState;
use askama::Template;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::Form;
use serde::Deserialize;

/// The secrets the page manages: the system email account password (BR-ADMIN-05) and the optional integration keys
/// (EXT-02, EXT-03; integrations stay off until I6, AP-12).
const SECRETS: [(SecretKey, &str); 5] = [
    (SecretKey::SystemSmtpPassword, "SMTP password"),
    (SecretKey::UserSmtpPassword, "Your SMTP password"),
    (SecretKey::GooglePlacesApiKey, "Google Places API key"),
    (SecretKey::SmartyStreetsApiKey, "SmartyStreets API key"),
    (SecretKey::FullContactApiKey, "FullContact API key"),
];

pub const ENCRYPTION: [&str; 3] = ["None", "STARTTLS", "SSL/TLS"];
pub const AUTHENTICATION: [&str; 2] = ["None", "Password"];

/// OP-INTEG-04 `accounting_integration_status`: QuickBooks Online is out of scope (D-02); the stub only says so.
pub fn accounting_integration_status() -> &'static str {
    "Not implemented"
}

/// The form as typed (05: typed form, unknown fields refused). The secret fields are named by `SecretKey::account`.
#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct SettingsForm {
    pub owner_id: String,
    pub tax_rate: String,
    pub default_tax_code: String,
    pub country_code: String,
    pub area_code: String,
    pub format_phone_numbers: String,
    pub email_address: String,
    pub email_full_name: String,
    pub email_user_name: String,
    pub smtp_server: String,
    pub smtp_port: String,
    pub smtp_encryption: String,
    pub smtp_authentication: String,
    pub system_smtp_password: String,
    /// The user's own email account (BR-INTEG-01; user.smtp_*). Empty server, port, encryption or authentication
    /// mean "as the system account" (337 s85-88).
    pub user_email_address: String,
    pub user_full_name: String,
    pub user_user_name: String,
    pub user_smtp_server: String,
    pub user_smtp_port: String,
    pub user_smtp_encryption: String,
    pub user_smtp_authentication: String,
    pub user_smtp_password: String,
    pub google_places: String,
    pub smartystreets: String,
    pub fullcontact: String,
}

impl SettingsForm {
    fn from_settings(s: &SystemSettings, u: &admin::UserEmail) -> Self {
        Self {
            user_email_address: u.address.clone().unwrap_or_default(),
            user_full_name: u.full_name.clone().unwrap_or_default(),
            user_user_name: u.user_name.clone().unwrap_or_default(),
            user_smtp_server: u.server.clone().unwrap_or_default(),
            user_smtp_port: u.port.map(|p| p.to_string()).unwrap_or_default(),
            user_smtp_encryption: u.encryption.clone().unwrap_or_default(),
            user_smtp_authentication: u.authentication.clone().unwrap_or_default(),
            owner_id: s.owner_id.clone().unwrap_or_default(),
            tax_rate: admin::format_percent(s.tax_rate_bp),
            default_tax_code: s.default_tax_code.clone().unwrap_or_default(),
            country_code: s.country_code.clone().unwrap_or_default(),
            area_code: s.area_code.map(|a| format!("{a:03}")).unwrap_or_default(),
            format_phone_numbers: if s.format_phone_numbers { "on".into() } else { String::new() },
            email_address: s.email_address.clone().unwrap_or_default(),
            email_full_name: s.email_full_name.clone().unwrap_or_default(),
            email_user_name: s.email_user_name.clone().unwrap_or_default(),
            smtp_server: s.smtp_server.clone().unwrap_or_default(),
            smtp_port: s.smtp_port.map(|p| p.to_string()).unwrap_or_default(),
            smtp_encryption: s.smtp_encryption.clone().unwrap_or_else(|| "STARTTLS".into()),
            smtp_authentication: s.smtp_authentication.clone().unwrap_or_else(|| "Password".into()),
            ..Default::default()
        }
    }

    /// The user's email account to save, or the field errors.
    fn parse_user(&self) -> Result<admin::UserEmail, Vec<FieldError>> {
        let mut errors = Vec::new();
        let address = admin::parse_email(&self.user_email_address);
        if let Err(message) = &address {
            errors.push(FieldError { field: "user_email_address", message: message.clone() });
        }
        let port = match self.user_smtp_port.trim() {
            "" => None,
            p => match p.parse::<u16>().ok().filter(|p| *p > 0) {
                Some(p) => Some(i64::from(p)),
                None => {
                    errors.push(FieldError {
                        field: "user_smtp_port",
                        message: "Enter a port from 1 to 65535, or leave it empty.".into(),
                    });
                    None
                }
            },
        };
        if !self.user_smtp_encryption.is_empty() && !ENCRYPTION.contains(&self.user_smtp_encryption.as_str()) {
            errors.push(FieldError { field: "user_smtp_encryption", message: "Choose an encryption type.".into() });
        }
        if !self.user_smtp_authentication.is_empty()
            && !AUTHENTICATION.contains(&self.user_smtp_authentication.as_str())
        {
            errors.push(FieldError {
                field: "user_smtp_authentication",
                message: "Choose an authentication method.".into(),
            });
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(admin::UserEmail {
            address: address.unwrap_or_default(),
            full_name: admin::optional(&self.user_full_name),
            user_name: admin::optional(&self.user_user_name),
            server: admin::optional(&self.user_smtp_server),
            port,
            encryption: admin::optional(&self.user_smtp_encryption),
            authentication: admin::optional(&self.user_smtp_authentication),
        })
    }

    /// The settings to save, or the field errors.
    fn parse(&self) -> Result<SystemSettings, Vec<FieldError>> {
        let mut errors = Vec::new();
        let mut check = |field: &'static str, r: Result<(), String>| {
            if let Err(message) = r {
                errors.push(FieldError { field, message });
            }
        };
        let tax = admin::parse_percent_bp(&self.tax_rate);
        check("tax_rate", tax.clone().map(drop));
        let country = admin::parse_country_code(&self.country_code);
        check("country_code", country.clone().map(drop));
        let area = admin::parse_area_code(&self.area_code);
        check("area_code", area.clone().map(drop));
        let email = admin::parse_email(&self.email_address);
        check("email_address", email.clone().map(drop));
        let port = match self.smtp_port.trim() {
            "" => Ok(None),
            p => p
                .parse::<u16>()
                .ok()
                .filter(|p| *p > 0)
                .map(|p| Some(i64::from(p)))
                .ok_or_else(|| "Enter a port from 1 to 65535, like 587.".to_owned()),
        };
        check("smtp_port", port.clone().map(drop));
        check(
            "smtp_encryption",
            ENCRYPTION
                .contains(&self.smtp_encryption.as_str())
                .then_some(())
                .ok_or_else(|| "Choose an encryption type.".to_owned()),
        );
        check(
            "smtp_authentication",
            AUTHENTICATION
                .contains(&self.smtp_authentication.as_str())
                .then_some(())
                .ok_or_else(|| "Choose an authentication method.".to_owned()),
        );
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(SystemSettings {
            owner_id: admin::optional(&self.owner_id),
            tax_rate_bp: tax.unwrap_or_default(),
            default_tax_code: admin::optional(&self.default_tax_code),
            country_code: country.ok(),
            area_code: area.unwrap_or_default(),
            format_phone_numbers: self.format_phone_numbers == "on",
            email_address: email.unwrap_or_default(),
            email_full_name: admin::optional(&self.email_full_name),
            email_user_name: admin::optional(&self.email_user_name),
            smtp_server: admin::optional(&self.smtp_server),
            smtp_port: port.unwrap_or_default(),
            smtp_encryption: Some(self.smtp_encryption.clone()),
            smtp_authentication: Some(self.smtp_authentication.clone()),
        })
    }

    /// The typed value of a secret field (write-only: never rendered back).
    fn secret(&self, key: SecretKey) -> &str {
        match key {
            SecretKey::SystemSmtpPassword => &self.system_smtp_password,
            SecretKey::GooglePlacesApiKey => &self.google_places,
            SecretKey::SmartyStreetsApiKey => &self.smartystreets,
            SecretKey::FullContactApiKey => &self.fullcontact,
            SecretKey::UserSmtpPassword => &self.user_smtp_password,
        }
    }

    /// The form to render again: the secret values are dropped, so they never go back to the page.
    fn without_secrets(mut self) -> Self {
        for s in [
            &mut self.system_smtp_password,
            &mut self.user_smtp_password,
            &mut self.google_places,
            &mut self.smartystreets,
            &mut self.fullcontact,
        ] {
            s.clear();
        }
        self
    }
}

#[derive(Template)]
#[template(path = "settings/form.html")]
pub struct FormView {
    pub form: SettingsForm,
    pub owners: Vec<(String, String)>,
    /// The rendered `SecretField`s (name and has_secret only): SMTP password first, then the user's, then the API
    /// keys.
    pub smtp_password: String,
    pub user_smtp_password: String,
    pub api_keys: Vec<String>,
    pub errors: Vec<FieldError>,
    pub saved: bool,
    pub accounting: &'static str,
    /// The rendered `StorePanel`: the no-keychain notice, or "Secrets: local file" (empty with the keychain).
    pub store: String,
    /// Where the integration keys are kept, for the Integrations panel.
    pub keys_note: &'static str,
}

impl FormView {
    pub fn error(&self, field: &str) -> Option<&str> {
        self.errors.iter().find(|e| e.field == field).map(|e| e.message.as_str())
    }
}

#[derive(Template)]
#[template(path = "settings/page.html")]
struct SettingsPage {
    nav: Nav,
    demo: bool,
    body: String,
    /// The Backup panel (OP-ADMIN-06, OP-ADMIN-07), outside the settings form.
    backup: String,
}

/// The `SecretField`s and the store notice, read off the async workers. A field error of `errors` goes on its field.
async fn secret_fields(store: Secrets, errors: Vec<FieldError>) -> Result<(Vec<SecretField>, StorePanel), AppError> {
    tokio::task::spawn_blocking(move || {
        let fields = SECRETS
            .iter()
            .map(|(k, label)| {
                let mut field = store.field(*k, label)?;
                field.error = errors.iter().find(|e| e.field == field.name).map(|e| e.message.clone());
                Ok(field)
            })
            .collect::<Result<Vec<_>, SecretError>>()?;
        Ok((fields, StorePanel::for_store(&store, None)))
    })
    .await
    .map_err(|e| AppError::Internal(format!("keychain task: {e}")))?
    .map_err(|e: SecretError| AppError::Internal(e.to_string()))
}

async fn view(
    state: &AppState,
    form: SettingsForm,
    errors: Vec<FieldError>,
    saved: bool,
) -> Result<FormView, AppError> {
    let owners = {
        let conn = state.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
        admin::owner_choices(&conn)?
    };
    let (fields, panel) = secret_fields(state.secret_store(), errors.clone()).await?;
    let keys_note = match panel.kind {
        StoreKind::Keychain => "Keys are kept in the system keychain.",
        StoreKind::File => "Keys are kept in KarbonFM's secrets file on this computer.",
        StoreKind::Unavailable => "Keys need a system keychain or the file option.",
    };
    let store = panel.render()?;
    let mut fields = fields.into_iter().map(|f| f.render());
    let smtp_password = fields.next().transpose()?.unwrap_or_default();
    let user_smtp_password = fields.next().transpose()?.unwrap_or_default();
    let api_keys = fields.collect::<Result<Vec<_>, _>>()?;
    Ok(FormView {
        store,
        keys_note,
        form: form.without_secrets(),
        owners,
        smtp_password,
        user_smtp_password,
        api_keys,
        errors,
        saved,
        accounting: accounting_integration_status(),
    })
}

fn respond(
    state: &AppState,
    headers: &HeaderMap,
    view: FormView,
    status: StatusCode,
    notice: Option<super::backup::Notice>,
) -> Result<Response, AppError> {
    let body = view.render()?;
    let html = if is_htmx(headers) {
        Html(body)
    } else {
        let backup = super::backup::Panel::for_state(state, notice).render()?;
        render(&SettingsPage { nav: Nav::Settings, demo: super::demo(state)?, body, backup })?
    };
    Ok((status, html).into_response())
}

/// `GET /settings`.
pub async fn page(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(actor): CurrentUser,
    Query(restored): Query<super::backup::RestoredQuery>,
) -> Result<Response, AppError> {
    let (settings, user) = {
        let conn = state.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
        (admin::settings_get(&conn)?, admin::user_email_get(&conn, &actor)?)
    };
    let view = view(&state, SettingsForm::from_settings(&settings, &user), Vec::new(), false).await?;
    respond(&state, &headers, view, StatusCode::OK, restored.notice())
}

/// `PUT /settings`: checks the form, writes the typed secrets to the keychain, then saves the settings row.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<SettingsForm>,
) -> Result<Response, AppError> {
    let parsed = match (form.parse(), form.parse_user()) {
        (Ok(s), Ok(u)) => Ok((s, u)),
        (Err(mut a), Err(b)) => {
            a.extend(b);
            Err(a)
        }
        (Err(a), _) | (_, Err(a)) => Err(a),
    };
    let (settings, user) = match parsed {
        Ok(s) => s,
        Err(errors) => {
            let view = view(&state, form, errors, false).await?;
            return respond(&state, &headers, view, StatusCode::UNPROCESSABLE_ENTITY, None);
        }
    };
    // Owner first: a bad owner must not leave a half-saved form (secrets saved, settings not).
    let owner_ok = {
        let conn = state.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
        admin::owner_choices(&conn)?.iter().any(|(id, _)| Some(id) == settings.owner_id.as_ref())
    };
    if !owner_ok {
        let errors =
            vec![FieldError { field: "owner_id", message: "Choose the organization that owns this install.".into() }];
        let view = view(&state, form, errors, false).await?;
        return respond(&state, &headers, view, StatusCode::UNPROCESSABLE_ENTITY, None);
    }
    let typed: Vec<(SecretKey, String)> = SECRETS.iter().map(|(k, _)| (*k, form.secret(*k).to_owned())).collect();
    let store = state.secret_store();
    let written = tokio::task::spawn_blocking(move || {
        typed.iter().try_for_each(|(k, v)| store.update_from_form(*k, v).map_err(|e| (*k, e)))
    })
    .await
    .map_err(|e| AppError::Internal(format!("keychain task: {e}")))?;
    match written {
        Ok(()) => {}
        // No keychain and no file: the field says so and nothing is saved (D-09). Not a 500, not "saved".
        Err((key, SecretError::Unavailable(_))) => {
            let message =
                "Not saved: this computer has no system keychain. Install one, or choose the file option above.";
            let errors = vec![FieldError { field: key.account(), message: message.into() }];
            let view = view(&state, form, errors, false).await?;
            return respond(&state, &headers, view, StatusCode::UNPROCESSABLE_ENTITY, None);
        }
        Err((_, e)) => return Err(AppError::Internal(e.to_string())),
    }
    let result = {
        let mut conn = state.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
        admin::settings_update(&mut conn, &settings, &actor)
            .and_then(|()| admin::user_email_update(&mut conn, &user, &actor))
    };
    match result {
        Ok(()) => {
            let saved = SettingsForm::from_settings(&settings, &user);
            let view = view(&state, saved, Vec::new(), true).await?;
            let mut res = respond(&state, &headers, view, StatusCode::OK, None)?;
            res.headers_mut().insert("HX-Trigger", r#"{"karbon-toast":"Settings saved"}"#.parse().unwrap());
            Ok(res)
        }
        Err(CommandError::Validation(errors)) => {
            let view = view(&state, form, errors, false).await?;
            respond(&state, &headers, view, StatusCode::UNPROCESSABLE_ENTITY, None)
        }
        Err(other) => Err(AppError::Internal(format!("settings_update: {other}"))),
    }
}
