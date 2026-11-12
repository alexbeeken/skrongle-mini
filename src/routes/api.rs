//! `/api/v1/…`: the routes a connected app calls with its bearer token
//! (`auth::api::ApiUser`, #167). Never a session cookie: the extractor reads
//! only `Authorization: Bearer`. While the owner's switch is off every one
//! answers 404.

use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::auth::api::{ApiUser, Scope};

/// GET /api/v1/whoami: the probe. The account's id, the app's name and
/// client id, its scopes and its course list (empty: all of the account's
/// own). Any scope will do.
pub async fn whoami(user: ApiUser) -> Response {
    let scopes: Vec<&str> = user.scopes.iter().map(|s| Scope::as_str(*s)).collect();
    let body = serde_json::json!({
        "user_id": user.id,
        "client_id": user.client_id,
        "client_name": user.client_name,
        "scopes": scopes,
        "courses": user.courses,
    });
    ([(axum::http::header::CACHE_CONTROL, "no-store")], Json(body)).into_response()
}

#[cfg(test)]
mod tests;
