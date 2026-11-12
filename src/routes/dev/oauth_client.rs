//! Dev mode: `/dev/oauth-client` (#169), a public OAuth client that plays
//! the whole flow against this server, so it can be tried with no vendor.
//! The page's script does what an app's client does: reads the discovery
//! documents, registers itself, builds the authorize address with PKCE,
//! takes the answer at `/dev/oauth-client/callback`, trades the code for
//! tokens, calls `/api/v1/whoami`, refreshes, replays the old refresh token
//! and the code, and disconnects (`/oauth/revoke`). Its state lives in the
//! tab (`sessionStorage`).

use askama::Template;
use axum::response::{IntoResponse, Response};

use crate::auth::{CurrentUser, MaybeUser};
use crate::routes::Page;

#[derive(Template)]
#[template(path = "dev_oauth_client.html")]
pub struct DevClientTemplate {
    pub user: Option<CurrentUser>,
}

/// GET /dev/oauth-client and /dev/oauth-client/callback: the same page; its
/// script reads the answer from the query.
pub async fn page(MaybeUser(user): MaybeUser) -> Response {
    ([(axum::http::header::CACHE_CONTROL, "no-store")], Page(DevClientTemplate { user })).into_response()
}
