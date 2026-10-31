//! UI-17 Settings (OP-ADMIN-03 `settings_get_update`, BR-ADMIN-05; secrets AP-03). `GET /settings` shows the form;
//! `PUT /settings` saves it and answers with the form again. The SMTP password and the integration API keys are
//! write-only `SecretField`s: the page gets only a has_secret flag, and a value goes straight to the keychain.
//! Keychain calls block (and may show an OS prompt), so they run on `spawn_blocking`, off the async workers.

use super::current_user::CurrentUser;
use super::Nav;
use crate::admin::{self, CommandError, FieldError, SystemSettings};
use crate::secrets::{SecretField, SecretKey, SecretStore};
use crate::web::{is_htmx, render, AppError};
use crate::AppState;
use askama::Template;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::Form;
use serde::Deserialize;
use std::sync::Arc;

/// The secrets the page manages: the system email account password (BR-ADMIN-05) and the optional integration keys
/// (EXT-02, EXT-03; integrations stay off until I6, AP-12).
const SECRETS: [(SecretKey, &str); 4] = [
    (SecretKey::SystemSmtpPassword, "SMTP password"),
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
    pub google_places: String,
    pub smartystreets: String,
    pub fullcontact: String,
}

impl SettingsForm {
    fn from_settings(s: &SystemSettings) -> Self {
        Self {
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
            SecretKey::UserSmtpPassword => "",
        }
    }

    /// The form to render again: the secret values are dropped, so they never go back to the page.
    fn without_secrets(mut self) -> Self {
        for s in
            [&mut self.system_smtp_password, &mut self.google_places, &mut self.smartystreets, &mut self.fullcontact]
        {
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
    /// The rendered `SecretField`s (name and has_secret only): SMTP password first, then the API keys.
    pub smtp_password: String,
    pub api_keys: Vec<String>,
    pub errors: Vec<FieldError>,
    pub saved: bool,
    pub accounting: &'static str,
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
    body: String,
    /// The Backup panel (OP-ADMIN-06, OP-ADMIN-07), outside the settings form.
    backup: String,
}

/// The `SecretField`s, read from the store off the async workers.
async fn secret_fields(store: Arc<dyn SecretStore>) -> Result<Vec<SecretField>, AppError> {
    tokio::task::spawn_blocking(move || {
        SECRETS.iter().map(|(k, label)| store.field(*k, label)).collect::<Result<Vec<_>, _>>()
    })
    .await
    .map_err(|e| AppError::Internal(format!("keychain task: {e}")))?
    .map_err(|e| AppError::Internal(e.to_string()))
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
    let mut fields = secret_fields(state.secrets.clone()).await?.into_iter().map(|f| f.render());
    let smtp_password = fields.next().transpose()?.unwrap_or_default();
    let api_keys = fields.collect::<Result<Vec<_>, _>>()?;
    Ok(FormView {
        form: form.without_secrets(),
        owners,
        smtp_password,
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
        render(&SettingsPage { nav: Nav::Settings, body, backup })?
    };
    Ok((status, html).into_response())
}

/// `GET /settings`.
pub async fn page(
    State(state): State<AppState>,
    headers: HeaderMap,
    _user: CurrentUser,
    Query(restored): Query<super::backup::RestoredQuery>,
) -> Result<Response, AppError> {
    let settings = {
        let conn = state.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
        admin::settings_get(&conn)?
    };
    let view = view(&state, SettingsForm::from_settings(&settings), Vec::new(), false).await?;
    respond(&state, &headers, view, StatusCode::OK, restored.notice())
}

/// `PUT /settings`: checks the form, writes the typed secrets to the keychain, then saves the settings row.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<SettingsForm>,
) -> Result<Response, AppError> {
    let settings = match form.parse() {
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
    let store = state.secrets.clone();
    tokio::task::spawn_blocking(move || typed.iter().try_for_each(|(k, v)| store.update_from_form(*k, v)))
        .await
        .map_err(|e| AppError::Internal(format!("keychain task: {e}")))?
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let result = {
        let mut conn = state.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
        admin::settings_update(&mut conn, &settings, &actor)
    };
    match result {
        Ok(()) => {
            let saved = SettingsForm::from_settings(&settings);
            let view = view(&state, saved, Vec::new(), true).await?;
            let mut res = respond(&state, &headers, view, StatusCode::OK, None)?;
            res.headers_mut().insert("HX-Trigger", r#"{"karbon-toast":"Settings saved"}"#.parse().unwrap());
            Ok(res)
        }
        Err(CommandError::Validation(errors)) => {
            let view = view(&state, form, errors, false).await?;
            respond(&state, &headers, view, StatusCode::UNPROCESSABLE_ENTITY, None)
        }
        Err(CommandError::AlreadySetUp) => Err(AppError::Internal("settings_update: unexpected AlreadySetUp".into())),
        Err(CommandError::Storage(e)) => Err(AppError::Internal(format!("settings_update: {e}"))),
    }
}
