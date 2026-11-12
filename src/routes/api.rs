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

/// A path that exists only for connected apps (#167 to #169): the bearer
/// API, `/mcp`, OAuth and its discovery documents, the profile's Make a
/// token and Revoke. The owner's own switch (`/manage/apps…`) is not one.
fn for_apps(path: &str) -> bool {
    let path = path.strip_prefix(crate::base_path()).filter(|p| p.starts_with('/')).unwrap_or(path);
    path == "/mcp" || ["/api/v1/", "/.well-known/", "/oauth/", "/profile/apps/"].iter().any(|p| path.starts_with(p))
}

fn bearer_only(path: &str) -> bool {
    let path = path.strip_prefix(crate::base_path()).filter(|p| p.starts_with('/')).unwrap_or(path);
    path == "/mcp" || path.starts_with("/api/v1/")
}

/// The owner's switch, asked before anything else reads the request (#170):
/// while it is off, every path of `for_apps` answers 404, whatever the
/// method, body, cookie or token (the handlers ask it too, after their
/// extractors, which could answer 415 or the sign-in page first).
pub async fn switch(axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::AppState>>, req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    if !for_apps(req.uri().path()) {
        return next.run(req).await;
    }
    match state.db.call(|c| crate::settings::apps_on(c)).await {
        Ok(true) => next.run(req).await,
        // The bearer routes answer as they do themselves: an empty 404.
        Ok(false) if bearer_only(req.uri().path()) => crate::auth::api::ApiError::Off.into_response(),
        Ok(false) => crate::error::AppError::NotFound.into_response(),
        Err(e) => e.into_response(),
    }
}

#[cfg(test)]
mod tests;
