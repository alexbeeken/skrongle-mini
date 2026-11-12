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
| 401, empty, `WWW-Authenticate: Bearer` | no token, or one that is unknown, expired or revoked, or an account without **Create courses** or waiting behind a temporary password |
| 400 `{"error": "unsupported_protocol_version"}` | an `MCP-Protocol-Version` header this server does not speak |
| 429, a JSON-RPC error (`-32000`) | more than 300 requests from one grant in ten minutes (`security::MCP_CALLS_PER_GRANT`); ten bad tokens from one address give a 429 too |
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
