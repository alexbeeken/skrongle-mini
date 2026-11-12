# Security: connected apps

This page is for the person running a Skrongle mini who wants to know what
can go wrong when they turn on **Let apps connect** (Manage Users), and what
stops it. It covers the three parts that switch opens: tokens made on the
profile, the MCP server at `/mcp` ([The MCP server](api)), and OAuth sign-in
for apps in a vendor's cloud. What the feature does is in
[Features](features#connected-apps); how to turn it on and run it behind a
path is in [Self-hosting](self-hosting#accounts-and-ai-keys).

Everything below is off until you turn the switch on. Off, every address the
feature added answers 404 and every token stops working; turning it on
again brings the connections back.

Each row names the test that proves it. To run one:

```sh
cargo test --features dev <test name>
```

## Threats and what stops them

| Threat | What stops it | Where in the code | Test(s) |
|---|---|---|---|
| A stolen copy of the database gives working tokens | Only the SHA-256 of a token or a code is stored, as for sessions. A copy holds nothing that signs in. | `auth::api::mint`, `oauth_server::allow`, `auth::hash_token` | `auth::api::tests::tokens_are_stored_hashed_only`, `routes::api::tests::make_a_token_and_use_it`, `oauth_server::tests::a_code_is_bound_to_its_request` |
| A token leaks through a log, a browser history or a link | A token in the address (`?access_token=`, `?token=`) is refused with 400. No log line names a token, a code, a PKCE verifier or a hash, down to TRACE. A token is shown once, `no-store`, and no page or script holds one. | `auth::api::token_in_query`, `routes::profile::apps::make_token` | `routes::api::tests::a_token_in_the_address_is_refused`, `routes::api::tests::no_token_or_hash_in_the_log`, `routes::oauth::tests::no_secret_in_the_trace_log_of_a_full_flow`, `routes::api::tests::no_template_shows_a_token_but_the_one_time_dialog`, `mcp::tests::one_log_line_per_write` |
| A web page uses a signed-in browser against the API | API routes read only `Authorization: Bearer`; a session cookie authorises nothing there and a token opens no page. Every route that changes something takes one kind of sign-in: pages take the session with the CSRF check, `/mcp` a bearer token, the OAuth program endpoints neither. | `auth::api::ApiUser`, `routes::mcp::endpoint`, `security::csrf` | `routes::api::tests::bearer_only_never_the_cookie_or_the_proxy_header`, `routes::oauth::tests::changing_routes_take_one_kind_of_sign_in`, `mcp::tests::the_gate` |
| A web page on the home network reaches the box by DNS rebinding | `/mcp` refuses any request with an `Origin` header (a browser always sends one). No route sends an `Access-Control-*` header, so no page can read an answer. | `routes::mcp::endpoint` | `mcp::tests::the_gate`, `mcp::tests::the_checklist_requests_are_refused`, `routes::oauth::tests::every_new_route_is_404_off_and_none_sends_cors` |
| An app does more than the person allowed | Two fixed scopes (a missing one is the one 403, `insufficient_scope`), an optional course list, the account's own courses only. A course list whose courses are all gone is refused rather than read as "all courses". Duplicate JSON keys are read the same way by the scope check and the call. | `auth::api::ApiUser::require`, `ApiUser::allows_course`, `auth::api::own_courses`, `mcp::tools` | `mcp::tests::scopes_are_enforced`, `mcp::tests::own_courses_only`, `routes::oauth::tests::a_course_list_never_widens_to_all_courses`, `mcp::tests::odd_json_rpc_is_refused_or_harmless` |
| A shared course's text steers the assistant | A course shared with the account is not readable through the API. Course text reaches the assistant only as JSON values in a tool result, never in the instructions or a tool description. | `mcp::tools::reachable`, `mcp::INSTRUCTIONS` | `mcp::tests::own_courses_only`, `mcp::tests::course_text_is_data` |
| Tokens are guessed | 256 random bits behind `skr_`, looked up by hash in one indexed search: an unknown, expired, revoked token and a refresh token used as a bearer get the same answer. 10 bad tokens per address in 10 minutes, then a bad token is 429; 30 refused token requests per address in 10 minutes, then a refusal is 429. The token or code is checked first, so a good one is served whatever its address's neighbours did, and a request with no `Authorization` header (how an MCP client finds `resource_metadata`) is a 401 that is not counted (#185). | `auth::api::user_for_token`, `security::API_BAD_TOKENS_PER_IP`, `security::OAUTH_TOKEN_FAILURES_PER_IP` | `auth::api::tests::tokens_are_looked_up_by_hash_through_the_index`, `auth::api::tests::unknown_expired_and_revoked_tokens_are_one_answer`, `routes::api::tests::bad_tokens_are_limited_per_address`, `routes::oauth::tests::guessing_codes_is_rate_limited` |
| One app floods a Pi Zero | 300 requests per grant in 10 minutes; the editor's course-write limit and course size limits; a body capped at the course size limit plus 64 KiB while it is read; JSON nested deeper than 32 or a batch over 10 refused before it runs; no event stream, so no connection stays open. | `routes::mcp::endpoint`, `mcp::parse`, `security::MCP_CALLS_PER_GRANT` | `mcp::tests::requests_are_limited_per_grant`, `mcp::tests::the_body_is_capped_while_it_is_read`, `mcp::tests::depth_and_batches_are_bounded`, `mcp::tests::the_editors_gates`, `mcp::tests::the_checklist_requests_are_refused` |
| A child's account connects an unknown app | Off by default and after an upgrade. Connecting needs Create courses, asked again on every request. The owner sees every grant on Manage Users and can revoke any of them. | `settings::APPS_ON`, `auth::CurrentUser::connects_apps`, `routes::manage::apps` | `settings::tests::apps_are_off_until_turned_on`, `routes::oauth::tests::an_account_without_create_courses_cannot_connect`, `routes::api::tests::permission_and_temporary_password_are_checked_each_request`, `routes::api::tests::the_owner_switches_apps_on_and_revokes_any_grant` |
| A forgotten connection lives for ever | Every grant expires (30 days, 90 or a year; there is no "until revoked"). The hourly housekeeping deletes expired grants and their tokens. | `auth::api::purge_expired`, `jobs::housekeeping` | `auth::api::tests::expired_grants_and_tokens_are_purged`, `auth::api::tests::the_hourly_job_purges_an_expired_grant`, `auth::api::tests::scopes_and_course_lists` |
| A password change or a temporary password leaves an attacker's app connected | Changing the password, a temporary password from the owner or `skrongle reset-password`, and the owner turning two-factor off all revoke every grant of the account. An account that loses Create courses is refused at its next request, mid-batch and mid-sign-in included. | `auth::api::revoke_all_for_user`, `owner::set_temporary_password` | `routes::api::tests::password_and_two_factor_events_revoke_every_grant`, `mcp::tests::losing_the_permission_or_a_temporary_password_cuts_off_mcp`, `routes::oauth::tests::an_account_changed_mid_flow_cannot_trade`, `mcp::tests::a_batch_stops_when_the_connection_ends` |
| A stolen authorization code is used | PKCE with S256 is required (`plain` is refused). A code works once, for ten minutes, bound to its client, redirect address and resource; a second use revokes the grant and every token under it. | `oauth_server::exchange_code`, `oauth_server::pkce_ok` | `oauth_server::tests::pkce_is_s256`, `oauth_server::tests::a_code_used_twice_revokes_its_grant`, `oauth_server::tests::a_code_lasts_ten_minutes`, `routes::oauth::tests::oauth_refuses_the_checklist`, `routes::oauth::tests::code_refresh_and_replay` |
| A stolen refresh token is used (#184) | Refresh tokens rotate with reuse detection (RFC 9700 §4.14.2): a refresh marks the token used instead of deleting it, and a used one presented again by the grant's own client revokes the grant and every token under it, logs one warning naming the grant and the account (no token, no hash), and answers `invalid_grant`. Whichever of the thief and the real app comes second ends the connection, and it leaves Connected apps. Another client's attempt changes nothing. A grant keeps only its newest used refresh token, so it holds at most four token rows. | `oauth_server::refresh` | `oauth_server::tests::a_reused_refresh_token_revokes_its_grant`, `oauth_server::tests::a_used_refresh_token_from_another_client_revokes_nothing`, `oauth_server::tests::refresh_tokens_rotate`, `oauth_server::tests::used_refresh_tokens_go_with_their_grant`, `routes::oauth::tests::a_reused_refresh_token_disconnects_the_app`, `db::tests::v8_down_drops_used_refresh_tokens_only` |
| The code is sent to an attacker | Redirect addresses match exactly; only `https://`, or `http://` on `localhost`, `127.0.0.1` or `[::1]` where only the port may differ. No user name, fragment, backslash, look-alike loopback name or other scheme. A failed check is shown on a page here and never redirected. | `oauth_server::valid_redirect_uri`, `oauth_server::redirect_matches` | `oauth_server::tests::redirect_addresses_are_https_or_loopback_only`, `oauth_server::tests::redirect_addresses_match_exactly_but_for_a_loopback_port`, `oauth_server::tests::redirect_tricks_do_not_pass`, `routes::oauth::tests::a_failed_check_never_redirects` |
| A look-alike app asks for consent | The consent page names the app and the host the answer goes to, cannot be framed, grants nothing on GET, and its Allow needs the session and this site's own page. An app's or a token's name cannot hide text-direction or invisible characters. | `routes::oauth::authorize`, `routes::oauth::decide`, `auth::api::is_hidden_char` | `routes::oauth::tests::the_consent_page_says_who_asks_and_grants_nothing_on_get`, `routes::oauth::tests::the_consent_post_needs_the_session_and_this_sites_page`, `routes::oauth::tests::app_names_cannot_mislead`, `auth::api::tests::names_cannot_hide_characters` |
| Registration fills the SD card | 20 registrations per address an hour; at most 100 registered apps, the oldest unused one pushed out first; an unused registration gone after a day; 5 redirect addresses of 200 characters, a name of 60, a body of 4 KB. Nothing named in a registration is ever fetched. | `oauth_server::register`, `oauth_server::purge` | `routes::oauth::tests::registration_cannot_fill_the_box`, `oauth_server::tests::the_registry_is_bounded`, `oauth_server::tests::purge_drops_expired_codes_and_untraded_grants` |
| A token is sent in clear on a public network | OAuth redirect addresses need HTTPS (loopback aside). `skrongle doctor` warns when apps may connect while `SKRONGLE_PUBLIC_URL` is a plain `http://` address that is not a home network's. | `oauth_server::valid_redirect_uri`, `doctor::apps_line` | `oauth_server::tests::redirect_addresses_are_https_or_loopback_only`, `doctor::tests::connected_apps_line_warns_about_plain_http_on_the_internet` |
| A forged `Host` header puts another site's address in what Skrongle hands out | Every address (discovery documents, `WWW-Authenticate`, course links) comes from `SKRONGLE_PUBLIC_URL` and the base path, never from `Host` or `X-Forwarded-*`. Without a public address the OAuth routes answer 404. | `routes::oauth::issuer`, `mcp::tools::link` | `routes::oauth::tests::discovery_addresses_come_from_the_setting_only`, `mcp::tests::links_never_come_from_the_host_header`, `routes::oauth::tests::every_route_is_404_off_or_without_a_public_address` |
| Behind a path, a check misses its route | Discovery, the consent form, the CSRF exemption of the program endpoints and the proxy's sign-in all work under `SKRONGLE_BASE_PATH`, and nothing answers at the root. | `security::for_programs`, `proxy_auth::left_alone` | `routes::oauth::tests::discovery_and_the_whole_flow_under_a_base_path` |
| The switch is off but a route still does something | One gate asks the switch before any extractor reads the request, so every route of the feature answers 404 whatever its method, body or cookie. A batch asks again before each later message. | `routes::api::switch`, `routes::mcp::still_works` | `routes::oauth::tests::every_new_route_is_404_off_and_none_sends_cors`, `mcp::tests::a_batch_stops_when_the_connection_ends` |
| A file name writes outside the course | Paths are taken exactly as sent and checked by the editor's own `check_file`: no `..`, no leading `/` or `./`, no backslash, no other folder, no other case, no NUL, at most 120 characters. Course files live in SQLite and are never written to disk. | `routes::courses::check_file`, `courses::valid_path` | `mcp::tests::bad_file_names_are_refused`, `mcp::tests::odd_file_names_are_refused` |
| One kind of token stands in for another | A refresh token opens nothing; an access or personal token is no refresh token; a personal token cannot be revoked through OAuth; a refresh token only works for its own client. | `auth::api::TOKEN_SQL`, `oauth_server::refresh`, `oauth_server::revoke` | `routes::oauth::tests::token_kinds_never_cross`, `auth::api::tests::token_expiry_and_kind_are_checked`, `oauth_server::tests::revoke_is_for_the_clients_own_tokens` |
| A token meant for another server is accepted | `resource` is required and must be this server or its `/mcp` (one trailing slash aside); the token request repeats it. | `oauth_server::served_resource` | `oauth_server::tests::resource_tricks_do_not_pass`, `routes::oauth::tests::the_token_request_repeats_the_binding` |
| The proxy's sign-in header stands in for a token or a click | The header gives no API access, starts no session on the API or the program endpoints, and the consent POST needs the session cookie itself. | `proxy_auth::left_alone`, `proxy_auth::needs_its_session` | `routes::oauth::tests::the_proxy_header_alone_grants_nothing`, `routes::api::tests::bearer_only_never_the_cookie_or_the_proxy_header` |
| The sign-in round trip becomes an open redirect | The waiting request comes back to the local `/oauth/authorize` path only, whatever the cookie holds. | `routes::oauth::resume` | `routes::oauth::tests::sign_in_comes_back_to_the_request_and_nowhere_else` |
| An error tells an attacker about the server | Any failure that is not the app's own mistake is a fixed `Internal error.`; token errors carry only the OAuth error code; JSON-RPC ids that are not a string or a number are refused, not echoed. | `mcp::answer`, `routes::oauth::token_error`, `mcp::item` | `mcp::tests::other_failures_are_a_fixed_message`, `routes::oauth::tests::code_refresh_and_replay`, `mcp::tests::odd_json_rpc_is_refused_or_harmless` |
| A flood or a stolen token goes unseen | The Connected apps card on Manage Users counts the apps registered and the tokens issued in the last day; `skrongle doctor` prints whether apps may connect and how many grants and registered apps there are. | `routes::manage::apps`, `auth::api::Issued`, `doctor::apps_line` | `routes::oauth::tests::the_owner_sees_registrations_and_tokens_of_the_last_day`, `auth::api::tests::issued_tokens_are_counted_for_a_day`, `doctor::tests::connected_apps_line_warns_about_plain_http_on_the_internet` |

## Accepted risks

These are known and left as they are, each for the reason given.

- **Plain HTTP on a home network.** A token crosses the network in clear, as
  the session cookie already does; someone who can read the network can use
  it until it is revoked. On a network you trust that is the same risk as
  signing in; elsewhere, put HTTPS in front (`skrongle doctor` warns).
- **An app chooses its own name.** Dynamic registration has to let it. The
  name cannot hide characters, and the consent page shows the host the
  answer goes to beside it, even for an app that calls itself "Skrongle".
- **Rate limits and the "tokens issued" count live in memory** and start
  empty after a restart. Keeping them in the database would cost an SD-card
  write per request.
- **A replayed code revokes its grant whoever presents it.** Anyone who
  learns a used code within its ten minutes can disconnect that app. A code
  only ever travels to the app's own redirect address, and the worst case is
  that the person connects the app again; ending a grant whose code was
  stolen matters more.
- **A reused refresh token disconnects the app, even an honest one** (#184).
  An honest client that retries a refresh after losing the answer will be
  disconnected and the person has to connect again. Telling that retry from
  a thief's replay is not possible, and ending a connection whose token was
  stolen matters more.
- **Only the previous refresh token is remembered** (#184). A grant keeps
  the hash of its newest used refresh token, not every one it ever had, so
  the table stays at four rows per grant over a year of hourly refreshes
  instead of about 8,760. A thief who refreshes with a stolen token and then
  refreshes again before the real app next does has pushed the stolen token
  out: the real app's late try is then an unknown token, refused without
  revoking, and the thief keeps the connection until it is revoked on the
  profile or Manage Users, or expires. The real app does stop working, and
  the owner's "tokens issued" count shows the extra refreshes. Any older
  token that comes back is refused too, so a stale one never works.
- **After a refresh, the old access token works until its hour ends.**
  Refreshing keeps only the newest older access token, so a client holds at
  most three working tokens; cutting the old one at once would break a client that
  refreshes while a request is in flight.
- **Allow sent twice makes two grants.** Each Allow is one grant against the
  account's four. One whose code is never traded is purged within about an
  hour after its code expires.
- **A flood of registrations can push out an unused one.** At 100 registered
  apps the oldest without a grant goes first, so an app whose registration
  was pushed out before its first Allow registers again. Apps with a grant
  are never pushed out.
- **A batch runs as one request for its limits.** It may hold up to 10
  messages, counted as one request against the per-grant limit; each write
  in it still counts against the course-write limit, and each later message
  asks the token again.
- **The token, registration and revocation endpoints skip the CSRF check.**
  They never read a cookie and send no `Access-Control-*` header, so a page
  elsewhere has no sign-in to abuse and cannot read their answers.
- **File paths are taken exactly as sent.** The editor's upload guesses a
  folder for a bare name; the API does not, so an agent gets a clear refusal
  instead of a file in a place it did not name.
- **The request body may be the course size limit plus 64 KiB.** A course
  near the limit is made in more than one call.
- **Errors say little.** A token error carries only its OAuth code, with no
  description, so an app gets less help when something goes wrong.
