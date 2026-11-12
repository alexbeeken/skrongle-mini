# The MCP server

A person's AI assistant (Claude Code, Claude, ChatGPT or another client
that speaks MCP) can make and edit that person's courses through
Skrongle's MCP server. The assistant writes the course JSON itself;
Skrongle makes no AI call and spends no AI allowance. It checks the
files the way the course editor does, stores them, and hands back a link
the assistant gives the person.

It is off until the owner turns on *Let apps connect* on Manage Users, and
it takes a token the person made on their profile (Connected apps, see
[Features](features), "Connected apps"). The code is `src/mcp.rs` (the
protocol and the tool list), `src/mcp/tools.rs` (the course tools),
`src/mcp/format.rs` (`get_course_format`) and `src/routes/mcp.rs` (the
HTTP gate).

## The endpoint

`POST <server>/mcp`, with the base path when the server has one
(`https://example.org/skrongle/mcp`). It speaks MCP's streamable HTTP
transport (protocol `2025-06-18`; `2025-03-26` is accepted too): each POST
carries one JSON-RPC message, or a batch of at most 10, and gets one JSON
answer. There is no session and no event stream, so a connected assistant
holds no connection open. A notification gets 202 with no body.

Methods: `initialize` (its `instructions` say to read `get_course_format`
first and that courses are named by id), `ping`, `tools/list` and
`tools/call`.

## Signing in

Only `Authorization: Bearer <token>` is read. A session cookie does
nothing here, and neither does a proxy's sign-in header. Every request is
checked in this order:

| Answer | When |
|---|---|
| 404, empty | *Let apps connect* is off: the route does not exist |
| 403 `{"error": "origin_not_allowed"}` | the request has an `Origin` header, whatever its value. A browser always sends one, so no web page can reach the server, not even one that found the box on a home network. No `Access-Control-*` header is ever sent. |
| 405 `{"error": "method_not_allowed"}`, `Allow: POST` | any method but POST (`GET` too: there is no stream to open) |
| 415 `{"error": "unsupported_media_type"}` | a body that is not `Content-Type: application/json` |
| 400 `{"error": "invalid_request"}` | a token in the address (`?access_token=`, `?token=`), even beside a good header |
| 401, empty, `WWW-Authenticate: Bearer` | no token, or one that is unknown, expired or revoked, or an account without **Create courses** or waiting behind a temporary password. With `SKRONGLE_PUBLIC_URL` set the header is `Bearer resource_metadata="<public address><base>/.well-known/oauth-protected-resource/mcp"`, where an app learns how to sign in (see "OAuth" below); the 403 below carries it too. |
| 400 `{"error": "unsupported_protocol_version"}` | an `MCP-Protocol-Version` header this server does not speak |
| 429, a JSON-RPC error (`-32000`) | more than 300 requests from one grant in ten minutes (`security::MCP_CALLS_PER_GRANT`); a bad token from an address that sent ten bad tokens in ten minutes gets a 429 too; a good token is still served, and a request with no `Authorization` header is not counted |
| 413, a JSON-RPC error | a body larger than a course may be (`SKRONGLE_COURSE_BYTES`) plus 64 KiB, stopped while it is read |
| 400, a JSON-RPC error | not JSON (`-32700`), nested more than 32 deep (`-32700`), an empty batch or one of more than 10 (`-32600`) |
| 403 `{"error": "insufficient_scope", "scope": "…"}` | a tool the grant's scopes do not allow; `WWW-Authenticate` names the scope. In a batch, nothing runs. |

Rate limits are off on the dev server, as everywhere.

## Scopes

Two, chosen when the token is made:

- `courses:read`: `get_course_format`, `list_courses`, `get_course_files`, `validate_course`.
- `courses:write`: `create_course`, `put_course_files`.

A token can also be limited to some of the person's courses. Then every
tool sees only those, and `create_course` is refused.

## Tools

| Tool | Scope | Does |
|---|---|---|
| `get_course_format` | `courses:read` | The [course format](course-format), one part at a time. No arguments: the overview and the list of section ids. `section`: that section and its subsections (`lesson` finds `lesson-files`). `section: "example"`: the files of a worked course that uses every feature; `file`: one of them. |
| `list_courses` | `courses:read` | The person's own courses: `id`, `title`, `language`, `language_label`, `lessons` (the count), `state`, `loads`, `error`, `link`. |
| `get_course_files` | `courses:read` | `course_id`, and optionally `paths`: the course's files as `{"path", "content"}`, with `missing` naming any path asked for that is not there. |
| `validate_course` | `courses:read` | Checks and stores nothing. `files` alone: a whole new course. `course_id` and `files`: the files laid over that course. `course_id` alone: the course as stored. |
| `create_course` | `courses:write` | A new course of the person's own from a whole set of files. It must load. It gets a new id (the manifest's `id` is rewritten, its `title` kept). Returns `course_id`, `title` and `link`. |
| `put_course_files` | `courses:write` | `course_id` and `files`: replaces or adds those files. The whole course with the new files is checked first; if any file is refused or the course would not load, nothing changes. Returns `saved` and `link`. |

`files` is a list of `{"path", "content"}`, each at its full path in the
course: `course.json`, `placement.json`, `lessons/<name>.json` (folders
allowed) or `pictures/<name>.svg`. Anything else, `..`, a leading slash
or a backslash among them, is refused by the editor's own check before
anything is written. `content` is the file's text; a JSON object is taken
too.

**What the tools never do:** read or change another account's course, read
a built-in course or one shared with the person (its text was written by
someone else), share, delete, read progress or accounts, or start an AI
job.

**Own courses only.** A built-in course, a course shared with the person,
another account's course, one outside the token's course list and an id
nobody has all get the same answer: *No course of yours has that id*.

**Answers.** A refusal the assistant can fix (a file that is not JSON, a
course that would not load, an unknown id, a limit) is a tool result with
`isError: true` and `{"error": "…", "errors": […]}`, worded as the editor
words it: a validation error names the file and the field. Any other
failure is the JSON-RPC error `-32603` *Internal error.*, with no detail;
the detail goes to the server log. Course text comes back only as values
inside a tool result, never in the instructions or a tool's description.

**Links.** With `SKRONGLE_PUBLIC_URL` set, a link is that address, the
base path and `/courses/<id>`. Without it, a link is the path alone
(`/skrongle/courses/<id>`): the person opens it on the address they use
for the server. A link is never built from the request's `Host`.

**Limits** are the editor's: a file up to 1 MiB, 400 files a course,
`SKRONGLE_COURSE_BYTES` a course, `SKRONGLE_COURSE_BYTES_PER_USER` an
account and `SKRONGLE_COURSES_PER_USER` courses, and the account's course
writes (`security::COURSE_WRITES_PER_USER`, which the editor's saves count
against too). A course near the size limit is made with `create_course`
and then filled with `put_course_files`, since one request holds at most
the course limit plus 64 KiB.

**Log.** Each write logs one line: the account id, the grant id, the tool
and the course id. Never the token, never the course text.

## Try it

With a token from your profile (Connected apps, **Make a token**):

```sh
curl -s -X POST http://<server>/mcp \
  -H "Authorization: Bearer skr_…" -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_courses"}}'
```

Or run `npx @modelcontextprotocol/inspector`, choose *Streamable HTTP*, give
the URL and the `Authorization` header, and click **Connect**.

## OAuth

An app that connects from a vendor's cloud (claude.ai, ChatGPT) cannot be
handed a token: it signs in with OAuth 2.1, and Skrongle is its own small
authorization server (#169). This only helps a server with a public HTTPS
address; on a home network a token from the profile is the way in. The
code is `src/oauth_server.rs` (the rules) and `src/routes/oauth.rs` (the
HTTP side).

Every route below answers 404 while *Let apps connect* is off, and while
`SKRONGLE_PUBLIC_URL` is unset: an issuer must be a fixed address. The
issuer is `SKRONGLE_PUBLIC_URL` plus the base path
(`https://example.org/skrongle`), and every address in the documents is
built from it, never from the request's `Host` or `X-Forwarded-*`. The
routes answer on plain HTTP too, but a redirect address that is not on
the person's own computer must be HTTPS.

| Route | What |
|---|---|
| `GET <base>/.well-known/oauth-protected-resource/mcp` | RFC 9728 for `/mcp`: `resource` `<issuer>/mcp`, `authorization_servers` `[<issuer>]`, the two scopes. `/mcp`'s 401 names it. |
| `GET <base>/.well-known/oauth-protected-resource` | the same for the site: `resource` `<issuer>` |
| `GET <base>/.well-known/oauth-authorization-server` | RFC 8414: the four endpoints below, `code` only, `authorization_code` and `refresh_token`, `S256` only, `token_endpoint_auth_methods_supported: ["none"]`, the scopes, `authorization_response_iss_parameter_supported: true` |
| `GET <base>/.well-known/openid-configuration` | the same document: the one place on the MCP spec's discovery list a server behind a path can answer itself ([Self-hosting](self-hosting), "Connected apps behind a path") |
| `GET <base>/oauth/authorize` | the request; the consent page |
| `POST <base>/oauth/authorize` | the consent page's answer (a form from the page, with the session cookie) |
| `POST <base>/oauth/token` | a code or a refresh token for tokens |
| `POST <base>/oauth/register` | RFC 7591 registration |
| `POST <base>/oauth/revoke` | RFC 7009: the app disconnects itself |

**Register** with JSON (at most 4 KB): `redirect_uris` (1 to 5, each at
most 200 characters, `https://…` or `http://localhost`, `http://127.0.0.1`,
`http://[::1]`; no fragment, no other scheme), `client_name` (at most 60
characters, no control characters or markup; without one the app is
named by its first address's host), and if given
`token_endpoint_auth_method: "none"`, `grant_types` within
`authorization_code` and `refresh_token`, `response_types: ["code"]`.
Anything else is ignored and nothing named in it is fetched. 201 with
`client_id`; 400 `invalid_redirect_uri` or `invalid_client_metadata`; 413
over 4 KB; 429 past 20 from one address in an hour. The server keeps at
most 100 registrations (`caps::MAX_OAUTH_CLIENTS`; past it the oldest with
no grant goes) and drops one nobody finished a grant for within a day.

**Authorize** takes `response_type=code`, `client_id`, `redirect_uri`,
`code_challenge` (S256, 43 characters), `code_challenge_method=S256`,
`state` (required, at most 500 characters), `resource` (required: the
issuer or `<issuer>/mcp`) and optionally `scope` (both scopes without
it). Each field at most once. The redirect address must be one the app
registered, the same string; for `http://` on a loopback host only the
port may differ. A request that fails any check gets an error page here
(`oauth-error`, the OAuth code and what is wrong) and is never sent back
to the app. Signed out, the person goes through the normal sign-in
(password, then two-factor) and comes back: the request waits in an
`oauth_next` cookie (HttpOnly, SameSite=Lax, ten minutes) and the sign-in
returns to `<base>/oauth/authorize?…` only, never to an address from the
request. An account without **Create courses** gets a page saying to ask
the owner. Otherwise the consent page: the app's name, the host the answer
goes to (from the registered address), the scopes in plain words, the
account's own courses to limit it to (a grant limited to some courses
cannot create one), how long it works (30 days, 90, a year), **Allow** and
**Deny**. It is asked every time; nothing is granted on GET.

**The answer** (POST, the same fields plus `decision=allow|deny`,
`course` per course and `lifetime`) needs the session cookie (a proxy's
sign-in header alone signs nobody in here) and `Sec-Fetch-Site:
same-origin`, or for a browser without it an `Origin` that is the public
address; anything else is a 403. The request is checked again. Deny sends
`error=access_denied&state=…&iss=…` to the redirect address; Allow makes
one grant (counted against the account's 4) and sends `code=…&state=…&iss=…`.

**Token** (`application/x-www-form-urlencoded`, each field once):

- `grant_type=authorization_code` with `code`, `client_id`,
  `redirect_uri`, `code_verifier` and `resource`, all required and equal to
  the request's. A code works once, for ten minutes; a second use deletes
  the grant it made and every token under it.
- `grant_type=refresh_token` with `refresh_token` and `client_id` (and
  `resource` if the app sends it). Only the client the grant was made for
  may use it; another client gets `invalid_grant` and changes nothing. The
  refresh token works once (rotation with reuse detection, RFC 9700
  §4.14.2): a second use by its client answers `invalid_grant` and revokes
  the grant and every token under it, the newest pair included, since one
  of the two holders stole it. An honest client that retries a refresh
  after losing the answer will be disconnected and the person has to
  connect again. Only the previous refresh token is remembered as used; an
  older one is simply `invalid_grant` ([Security: connected
  apps](security-apps), "Accepted risks").

200 is `{"access_token", "token_type": "Bearer", "expires_in": 3600,
"refresh_token", "scope"}`: the access token works an hour (never past the
grant), the refresh token as long as the grant. Errors are RFC 6749 §5.2,
the code only (`{"error": "invalid_grant"}`): `invalid_request`,
`invalid_client` (401), `invalid_grant`, `unsupported_grant_type`,
`invalid_target`. Every answer carries `Cache-Control: no-store`. 30
refused requests from one address in ten minutes, then a refused request
is 429 until the window ends (`security::OAUTH_TOKEN_FAILURES_PER_IP`); a
good code or refresh token from that address is still served.

**Revoke** (`token`, `client_id`): the grant behind the app's own access or
refresh token goes, with every token under it. 200 for an unknown token
too; `invalid_client` for an unknown client.

**Try it** on the dev server with `/dev/oauth-client`, a public client that
plays the whole flow ([Testing](testing), "The dev OAuth client").
