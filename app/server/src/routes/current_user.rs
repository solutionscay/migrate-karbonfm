//! OP-ADMIN-04 `current_user`: the request extractor that gives a handler the single local user as the actor of its
//! command (BR-ADMIN-04, stamps of BR-PARTY-07). Before setup there is no user; the extractor then sends the page
//! to /setup, as the setup gate does.

use super::setup::redirect;
use crate::audit::Actor;
use crate::web::AppError;
use crate::AppState;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};

/// The local user. Every write handler takes it and passes `.0` to its command.
pub struct CurrentUser(pub Actor);

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let conn = state.db.lock().map_err(|e| AppError::Internal(e.to_string()).into_response())?;
        match crate::admin::current_user(&conn) {
            Ok(Some(actor)) => Ok(CurrentUser(actor)),
            Ok(None) => Err(redirect(&parts.headers, "/setup")),
            Err(e) => Err(AppError::from(e).into_response()),
        }
    }
}
