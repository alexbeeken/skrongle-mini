//! The documentation under `docs/`, served inside the app so a link can be
//! handed to a person or an LLM. `/docs/<page>` renders the Markdown;
//! `/docs/<page>.md` returns it raw; `/llms.txt` lists the pages an LLM needs
//! to write a course. Pages are grouped into sections ([`SECTIONS`]): user
//! guides and reference first, developer pages last (and any page not listed,
//! so a new file shows up without a code change).
//!
//! While themes are off (#1138) the docs say nothing about them: the theme
//! pages are gone and theme-only text is cut out ([`for_themes`]), so a
//! learner can't tell they exist until they ship.

use askama::Template;
use axum::extract::{Path, State};
use axum::http::{header, HeaderValue};
use axum::response::{IntoResponse, Response};
use std::sync::Arc;

use super::{Docs, Page};
use crate::auth::{CurrentUser, MaybeUser};
use crate::error::AppError;
use crate::AppState;

pub struct DocLink {
    pub slug: String,
    pub title: String,
}

/// One heading of the docs sidebar and its pages.
pub struct DocSection {
    pub id: &'static str,
    /// The heading's i18n key (`docs.section_*`).
    pub title: &'static str,
    pub pages: Vec<DocLink>,
}

/// Pages that are only about themes: not listed or served while themes
/// are off.
pub const THEME_PAGES: &[&str] = &["using-themes", "themes"];

/// Docs text for a site with themes on or off. Text only for a site with
/// themes on sits between `<!-- themes -->` and `<!-- /themes -->`; text
/// only for a site with them off, between `<!-- no-themes -->` and
/// `<!-- /no-themes -->`. Either may be inline or on lines of their own (a
/// marker line goes with the span). The other mode's spans are cut, and
/// the markers dropped.
pub fn for_themes(md: &str, on: bool) -> String {
    let (cut, keep) = if on { ("no-themes", "themes") } else { ("themes", "no-themes") };
    let cut_open = format!("<!-- {cut} -->");
    let cut_close = format!("<!-- /{cut} -->");
    let mut out = String::with_capacity(md.len());
    let mut rest = md;
    while let Some(i) = rest.find(&cut_open) {
        out.push_str(rest.get(..i).unwrap_or_default());
        let after = rest.get(i..).unwrap_or_default();
        let Some(j) = after.find(&cut_close) else {
            rest = "";
            break;
        };
        rest = after.get(j + cut_close.len()..).unwrap_or_default();
        // A span that starts a line takes the end of its last line with it.
        if (out.is_empty() || out.ends_with('\n')) && rest.starts_with('\n') {
            rest = rest.get(1..).unwrap_or_default();
        }
    }
    out.push_str(rest);
    let marks = [format!("<!-- {keep} -->"), format!("<!-- /{keep} -->")];
    let mut kept: String = out.split_inclusive('\n').filter(|l| !marks.iter().any(|m| l.trim() == m)).collect();
    for m in &marks {
        kept = kept.replace(m.as_str(), "");
    }
    kept
}

/// `docs/course.schema.json` without anything about themes: the theme file,
/// the `themes` blocks of a course and a lesson, and the line naming them.
pub fn schema_without_themes(raw: &[u8]) -> Vec<u8> {
    let Ok(mut v) = serde_json::from_slice::<serde_json::Value>(raw) else { return raw.to_vec() };
    fn walk(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Object(m) => {
                if let Some(serde_json::Value::Object(props)) = m.get_mut("properties") {
                    props.remove("themes");
                }
                for x in m.values_mut() {
                    walk(x);
                }
            }
            serde_json::Value::Array(a) => {
                a.retain(|x| x.get("$ref").and_then(|r| r.as_str()) != Some("#/$defs/theme"));
                a.iter_mut().for_each(walk);
            }
            _ => {}
        }
    }
    if let Some(serde_json::Value::Object(defs)) = v.get_mut("$defs") {
        for d in ["theme", "theme_colors", "lesson_theme"] {
            defs.remove(d);
        }
    }
    walk(&mut v);
    if let Some(serde_json::Value::String(d)) = v.get_mut("description") {
        *d = d.replace("for one lesson file and for a theme file (data/themes/<id>/theme.json)", "and for one lesson file");
    }
    serde_json::to_vec_pretty(&v).unwrap_or_else(|_| raw.to_vec())
}

/// The self-hosting pages, hidden on the hosted site until Skrongle mini
/// ships (#1435): not served, not in the sidebar, not picked up by the
/// catch-all section. A mini still serves them. Bringing them back is a
/// revert of that issue's PR.
pub const HIDDEN_UNTIL_MINI_SHIPS: &[&str] = &["self-hosting", "self-hosting-faq"];

/// Whether a page is hidden on this server.
pub fn hidden(slug: &str) -> bool {
    !crate::is_mini() && HIDDEN_UNTIL_MINI_SHIPS.contains(&slug)
}

fn md_of(slug: &str, on: bool) -> Option<String> {
    if (!on && THEME_PAGES.contains(&slug)) || hidden(slug) {
        return None;
    }
    let f = Docs::get(&format!("{slug}.md"))?;
    Some(for_themes(&String::from_utf8_lossy(&f.data), on))
}

/// The sidebar, in order: (id, title key in `i18n/<code>.yaml`, page slugs).
/// User pages are listed by name; the last section also takes every page not
/// named anywhere, except the hidden ones ([`HIDDEN_UNTIL_MINI_SHIPS`]).
pub const SECTIONS: &[(&str, &str, &[&str])] = &[
    ("guides", "docs.section_guides", &["index", "getting-started", "course-structure", "making-courses", "using-themes", "faq"]),
    (
        "reference",
        "docs.section_reference",
        &["course-format", "features", "themes", "progress", "i18n", "pricing", "data-inventory", "privacy", "cookies", "content-check-versions"],
    ),
    (
        "dev",
        "docs.section_dev",
        &["dev", "architecture", "testing", "qa", "e2e-audit", "security-review", "scalability", "scalability-aws", "load-testing", "aws-setup"],
    ),
];

#[derive(Template)]
#[template(path = "docs.html")]
pub struct DocsTemplate {
    pub user: Option<CurrentUser>,
    pub title: String,
    pub slug: String,
    pub html: String,
    pub sections: Vec<DocSection>,
}

fn title_of(md: &str, slug: &str) -> String {
    md.lines()
        .find_map(|l| l.strip_prefix("# ").map(|t| t.trim().to_string()))
        .unwrap_or_else(|| slug.replace('-', " "))
}

/// The release tarball the self-hosting guide installs: the one for the
/// version this server runs (`Cargo.toml`; the release workflow refuses a
/// tag that doesn't match it), for the Pi (`aarch64`). Named
/// `skrongle-<version>-<arch>.tar.gz` by `scripts/package-release.sh`, with
/// `skrongle`, `data/`, `skrongle.service` and `terms.example.md` inside,
/// as `docs/self-hosting.md` lays out. The download only exists once the
/// repo is public and that version is released (#95, #101).
pub const RELEASE_TARBALL_NAME: &str = concat!("skrongle-", env!("CARGO_PKG_VERSION"), "-aarch64.tar.gz");
pub const RELEASE_TARBALL: &str = concat!(
    "https://github.com/alexbeeken/pooki/releases/download/v",
    env!("CARGO_PKG_VERSION"),
    "/skrongle-",
    env!("CARGO_PKG_VERSION"),
    "-aarch64.tar.gz"
);

/// "Host it yourself": the friendly guide for first-time self-hosters. The
/// technical steps are in `docs/self-hosting.md`, which it links to.
#[derive(Template)]
#[template(path = "self_host.html")]
pub struct SelfHostTemplate {
    pub user: Option<CurrentUser>,
}

/// GET /self-host. 404 on the hosted site until Skrongle mini ships (#1435),
/// like the pages in [`HIDDEN_UNTIL_MINI_SHIPS`].
pub async fn self_host(MaybeUser(user): MaybeUser) -> Result<Page<SelfHostTemplate>, AppError> {
    if !crate::is_mini() {
        return Err(AppError::NotFound);
    }
    Ok(Page(SelfHostTemplate { user }))
}

/// GET /privacy: the privacy policy (`docs/privacy.md`), public.
pub async fn privacy(State(state): State<Arc<AppState>>, MaybeUser(user): MaybeUser) -> Result<Response, AppError> {
    render(&state, "privacy", user)
}

/// GET /cookies: the cookie notice (`docs/cookies.md`), public.
pub async fn cookies(State(state): State<Arc<AppState>>, MaybeUser(user): MaybeUser) -> Result<Response, AppError> {
    render(&state, "cookies", user)
}

/// Every embedded `.md` file, as (slug, title); the theme pages only while
/// themes are on.
pub fn pages(themes_on: bool) -> Vec<DocLink> {
    let mut names: Vec<String> = Docs::iter().map(|n| n.to_string()).filter(|n| n.ends_with(".md") && n != "terms.md").collect();
    names.sort();
    // README-style ordering: overview first, then the rest alphabetically.
    names.sort_by_key(|n| if n == "index.md" { 0 } else { 1 });
    names
        .into_iter()
        .filter_map(|n| {
            let slug = n.trim_end_matches(".md").to_string();
            let body = md_of(&slug, themes_on)?;
            Some(DocLink { title: title_of(&body, &slug), slug })
        })
        .collect()
}

/// The sidebar: every embedded page, in its section.
pub fn sections(themes_on: bool) -> Vec<DocSection> {
    let mut all = pages(themes_on);
    let mut out: Vec<DocSection> = SECTIONS
        .iter()
        .map(|(id, title, slugs)| DocSection {
            id,
            title,
            pages: slugs.iter().filter_map(|s| all.iter().position(|p| p.slug == *s).map(|i| all.remove(i))).collect(),
        })
        .collect();
    if let Some(last) = out.last_mut() {
        last.pages.extend(all);
    }
    out
}

/// GET /llms.txt — the index an LLM is given (llmstxt.org format). Links are
/// absolute when `SKRONGLE_PUBLIC_URL` is set, else app-absolute paths.
pub async fn llms_txt(State(state): State<Arc<AppState>>) -> Response {
    let base = crate::env("PUBLIC_URL")
        .map(|u| u.trim().trim_end_matches('/').to_string())
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| crate::base_path().to_string());
    (
        [(header::CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"))],
        llms_body(!state.themes.is_off()).replace("{base}", &base),
    )
        .into_response()
}

fn llms_body(themes_on: bool) -> String {
    for_themes(&Docs::get("llms.txt").map(|f| String::from_utf8_lossy(&f.data).into_owned()).unwrap_or_default(), themes_on)
}

/// GET /docs
pub async fn index(State(state): State<Arc<AppState>>, MaybeUser(user): MaybeUser) -> Result<Response, AppError> {
    render(&state, "index", user)
}

/// GET /docs/{page}  (or {page}.md for the raw Markdown)
pub async fn page(State(state): State<Arc<AppState>>, MaybeUser(user): MaybeUser, Path(page): Path<String>) -> Result<Response, AppError> {
    let on = !state.themes.is_off();
    if let Some(slug) = page.strip_suffix(".md") {
        let md = md_of(slug, on).ok_or(AppError::NotFound)?;
        return Ok((
            [(header::CONTENT_TYPE, HeaderValue::from_static("text/markdown; charset=utf-8"))],
            md,
        )
            .into_response());
    }
    if let Some(name) = page.strip_suffix(".json") {
        let f = Docs::get(&format!("{name}.json")).ok_or(AppError::NotFound)?;
        let body = if on || name != "course.schema" { f.data.into_owned() } else { schema_without_themes(&f.data) };
        return Ok((
            [(header::CONTENT_TYPE, HeaderValue::from_static("application/json; charset=utf-8"))],
            body,
        )
            .into_response());
    }
    render(&state, &page, user)
}

fn render(state: &AppState, slug: &str, user: Option<CurrentUser>) -> Result<Response, AppError> {
    if !slug.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(AppError::NotFound);
    }
    // The terms have their own page (contact filled in, sections per config).
    if slug == "terms" {
        return Ok(axum::response::Redirect::to(&crate::url("/terms")).into_response());
    }
    let on = !state.themes.is_off();
    let md = md_of(slug, on).ok_or(AppError::NotFound)?;
    let title = title_of(&md, slug);
    let html = render_doc(&md);
    Ok(Page(DocsTemplate { user, title, slug: slug.to_string(), html, sections: sections(on) }).into_response())
}

/// Markdown to HTML for a docs page. The files link to each other relatively
/// (`[Theming](themes)`), which reads right on GitHub but resolves against the
/// wrong folder from `/docs` (no trailing slash), so relative links become
/// `/docs/<page>`. Every link also gets a testid, and links to another page
/// its slug, so a test can tell them apart.
pub(crate) fn render_doc(md: &str) -> String {
    use pulldown_cmark::{html, CowStr, Event, Options, Parser, Tag, TagEnd};
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_SMART_PUNCTUATION);
    let mut events: Vec<Event> = Parser::new_ext(md, opts).collect();
    add_heading_ids(&mut events);
    let events = events.into_iter().map(|ev| match ev {
        Event::Start(Tag::Link { dest_url, title, .. }) => {
            let (href, slug) = doc_href(&dest_url);
            let mut a = format!("<a href=\"{}\" data-testid=\"docs-link\"", attr(&href));
            if let Some(slug) = slug {
                a.push_str(&format!(" data-slug=\"{}\"", attr(&slug)));
            }
            if !title.is_empty() {
                a.push_str(&format!(" title=\"{}\"", attr(&title)));
            }
            a.push('>');
            Event::InlineHtml(CowStr::from(a))
        }
        Event::End(TagEnd::Link) => Event::InlineHtml(CowStr::Borrowed("</a>")),
        ev => ev,
    });
    let mut out = String::with_capacity(md.len() * 3 / 2);
    html::push_html(&mut out, events);
    out
}

/// Give every heading a GitHub-style id ("Themed material (optional)" ->
/// `themed-material-optional`), so `page#heading` links land where they do
/// on GitHub. A repeated heading gets `-1`, `-2`….
fn add_heading_ids(events: &mut [pulldown_cmark::Event]) {
    use pulldown_cmark::{CowStr, Event, Tag, TagEnd};
    let mut seen: Vec<String> = Vec::new();
    for i in 0..events.len() {
        let Some(Event::Start(Tag::Heading { id: None, .. })) = events.get(i) else { continue };
        let mut text = String::new();
        for ev in events.iter().skip(i + 1) {
            match ev {
                Event::End(TagEnd::Heading(_)) => break,
                Event::Text(t) | Event::Code(t) => text.push_str(t),
                _ => {}
            }
        }
        let base = heading_slug(&text);
        let n = seen.iter().filter(|s| **s == base).count();
        seen.push(base.clone());
        let slug = if n == 0 { base } else { format!("{base}-{n}") };
        if let Some(Event::Start(Tag::Heading { id, .. })) = events.get_mut(i) {
            *id = Some(CowStr::from(slug));
        }
    }
}

fn heading_slug(text: &str) -> String {
    text.trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .collect()
}

/// Where a link in a docs page goes, and the docs page it names, if any.
fn doc_href(dest: &str) -> (String, Option<String>) {
    let relative = !dest.is_empty()
        && !dest.starts_with(['/', '#', '?'])
        && !dest.split(['/', '#', '?']).next().is_some_and(|first| first.contains(':'));
    if !relative {
        return (dest.to_string(), None);
    }
    let page = dest.split(['#', '?']).next().unwrap_or_default();
    let slug = (!page.contains('.') && !page.contains('/')).then(|| page.to_string()).filter(|s| !s.is_empty());
    (format!("{}/docs/{dest}", crate::base_path()), slug)
}

fn attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The guide downloads one tarball and unpacks the same name (#641).
    #[test]
    fn release_tarball_is_this_version() {
        assert!(RELEASE_TARBALL.ends_with(&format!("/{RELEASE_TARBALL_NAME}")));
        assert_eq!(RELEASE_TARBALL_NAME, format!("skrongle-{}-aarch64.tar.gz", env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn relative_links_point_into_docs() {
        let base = crate::base_path();
        assert_eq!(doc_href("themes"), (format!("{base}/docs/themes"), Some("themes".into())));
        assert_eq!(doc_href("course-format#lessons"), (format!("{base}/docs/course-format#lessons"), Some("course-format".into())));
        assert_eq!(doc_href("course-format.md"), (format!("{base}/docs/course-format.md"), None));
        assert_eq!(doc_href("https://example.com/x"), ("https://example.com/x".into(), None));
        assert_eq!(doc_href("mailto:a@b.c"), ("mailto:a@b.c".into(), None));
        assert_eq!(doc_href("#heading"), ("#heading".into(), None));
        assert_eq!(doc_href("/login"), ("/login".into(), None));
    }

    #[test]
    fn headings_get_github_ids() {
        let html = render_doc("# Course format\n\n## Themed material (optional)\n\n## `quiz`\n\n## Notes\n\n## Notes\n");
        assert!(html.contains("<h2 id=\"themed-material-optional\">"), "{html}");
        assert!(html.contains("<h2 id=\"quiz\">"), "{html}");
        assert!(html.contains("<h2 id=\"notes-1\">"), "{html}");
    }

    /// Every page is in exactly one section and the named ones exist.
    #[test]
    fn every_page_has_one_section() {
        for on in [true, false] {
            let secs = sections(on);
            let listed: Vec<&str> = secs.iter().flat_map(|s| s.pages.iter().map(|p| p.slug.as_str())).collect();
            let all = pages(on);
            assert_eq!(listed.len(), all.len());
            for p in &all {
                assert_eq!(listed.iter().filter(|s| **s == p.slug).count(), 1, "{} listed once", p.slug);
            }
            assert_eq!(secs[0].pages[0].slug, "index");
        }
        for (_, _, slugs) in SECTIONS {
            for s in *slugs {
                assert!(Docs::get(&format!("{s}.md")).is_some(), "SECTIONS names docs/{s}.md, which does not exist");
            }
        }
    }

    /// Until Skrongle mini ships (#1435) the hosted site (the shape tests
    /// run as) serves no self-hosting page and lists none, and no public
    /// page links to one.
    #[test]
    fn self_hosting_pages_are_hidden_until_mini_ships() {
        assert!(!crate::is_mini());
        for on in [true, false] {
            for slug in HIDDEN_UNTIL_MINI_SHIPS {
                assert!(md_of(slug, on).is_none(), "docs/{slug}.md is served");
                assert!(sections(on).iter().all(|s| s.pages.iter().all(|p| p.slug != *slug)), "{slug} is in the sidebar");
            }
            for slug in ["index", "faq", "getting-started", "course-structure", "making-courses", "using-themes"] {
                let Some(md) = md_of(slug, on) else { continue };
                let html = render_doc(&md);
                assert!(!html.contains("/self-host") && !html.contains("self-hosting"), "docs/{slug}.md links to self-hosting");
            }
            let llms = llms_body(on);
            assert!(!llms.contains("self-host"), "llms.txt mentions self-hosting");
        }
    }

    /// Each section title is a key that exists, so the sidebar never shows
    /// the bare key ("docs.using").
    #[test]
    fn section_titles_are_translated() {
        for (_, key, _) in SECTIONS {
            assert_ne!(crate::t(key), *key, "i18n/en.yaml has no {key}");
        }
    }

    /// llms.txt names the course format and the schema, and every file it
    /// links to is served.
    #[test]
    fn llms_txt_links_resolve() {
        for on in [true, false] {
            let body = llms_body(on);
            assert!(body.contains("{base}/docs/course-format.md") && body.contains("{base}/docs/course.schema.json"));
            for link in body.split("]({base}/docs/").skip(1) {
                let file = link.split(')').next().unwrap();
                let served = match file.strip_suffix(".md") {
                    Some(slug) => md_of(slug, on).is_some(),
                    None => Docs::get(file).is_some(),
                };
                assert!(served, "llms.txt (themes on: {on}) links to docs/{file}, which is not served");
            }
        }
    }

    /// Spans cut and markers dropped, inline and on lines of their own.
    #[test]
    fn theme_spans_follow_the_switch() {
        let md = "a<!-- themes -->, b<!-- /themes --> c<!-- no-themes --> d<!-- /no-themes -->\n\n<!-- themes -->\n## T\n\nx\n<!-- /themes -->\n- one\n";
        assert_eq!(for_themes(md, true), "a, b c\n\n## T\n\nx\n- one\n");
        assert_eq!(for_themes(md, false), "a c d\n\n- one\n");
    }

    /// While themes are off no served page, `llms.txt` or the schema says
    /// a word about them (#1184), and every marker in the files is closed.
    #[test]
    fn themes_off_docs_never_say_theme() {
        let mut bad = Vec::new();
        for name in Docs::iter().map(|n| n.to_string()).filter(|n| n.ends_with(".md") || n.ends_with(".txt")) {
            let raw = String::from_utf8_lossy(&Docs::get(&name).unwrap().data).into_owned();
            for m in ["themes", "no-themes"] {
                assert_eq!(raw.matches(&format!("<!-- {m} -->")).count(), raw.matches(&format!("<!-- /{m} -->")).count(), "docs/{name}: unclosed <!-- {m} -->");
            }
            let off = match name.strip_suffix(".md") {
                Some(slug) => match md_of(slug, false) { Some(md) => md, None => continue },
                None => for_themes(&raw, false),
            };
            for (i, line) in off.lines().enumerate() {
                if line.to_lowercase().contains("theme") {
                    bad.push(format!("docs/{name} (themes off) line {}: {line}", i + 1));
                }
            }
        }
        let schema = String::from_utf8(schema_without_themes(&Docs::get("course.schema.json").unwrap().data)).unwrap();
        for line in schema.lines().filter(|l| l.to_lowercase().contains("theme")) {
            bad.push(format!("course.schema.json (themes off): {}", line.trim()));
        }
        assert!(bad.is_empty(), "{} lines name themes while they are off; fence them with <!-- themes --> … <!-- /themes -->:\n{}", bad.len(), bad.join("\n"));
    }

    #[test]
    fn headings_get_ids() {
        assert_eq!(heading_slug("Diagnostics: `skrongle doctor`"), "diagnostics-skrongle-doctor");
        assert_eq!(heading_slug("Backup and restore"), "backup-and-restore");
        let html = render_doc("## Updating\n\ntext\n\n## Updating\n");
        assert!(html.contains("<h2 id=\"updating\">") && html.contains("<h2 id=\"updating-1\">"), "{html}");
    }

    /// Every `page#section` link in the docs names a heading on that page.
    #[test]
    fn every_docs_anchor_resolves() {
      for on in [true, false] {
        let ids_of = |slug: &str| -> Vec<String> {
            let md = md_of(slug, on).unwrap_or_else(|| panic!("a page links to {slug}, which is not served (themes on: {on})"));
            render_doc(&md).split(" id=\"").skip(1).map(|h| h.split('"').next().unwrap().to_string()).collect()
        };
        for p in pages(on) {
            let md = md_of(&p.slug, on).unwrap();
            let html = render_doc(&md);
            for href in html.split("href=\"").skip(1).map(|h| h.split('"').next().unwrap()) {
                let (page, anchor) = if let Some(a) = href.strip_prefix('#') {
                    (p.slug.clone(), a)
                } else if let Some(rest) = href.strip_prefix(&format!("{}/docs/", crate::base_path())) {
                    match rest.split_once('#') {
                        Some((page, a)) if !page.contains('.') => (page.to_string(), a),
                        _ => continue,
                    }
                } else {
                    continue;
                };
                assert!(ids_of(&page).iter().any(|id| id == anchor), "docs/{}.md links to {href}, but {page} has no heading #{anchor} (themes on: {on})", p.slug);
            }
        }
      }
    }

    /// A raw `<title>` (or another tag whose content the browser reads as
    /// text) in a doc's prose swallows the rest of the page: "<title> (copy)"
    /// cut /docs/making-courses off mid-list. Write `&lt;title&gt;` or use
    /// backticks.
    #[test]
    fn no_doc_swallows_the_rest_of_its_page() {
        for p in pages(true) {
            let md = String::from_utf8_lossy(&Docs::get(&format!("{}.md", p.slug)).unwrap().data).into_owned();
            let html = render_doc(&md).to_ascii_lowercase();
            for tag in ["title", "textarea", "style", "script", "xmp", "plaintext", "iframe", "noembed", "noframes", "noscript"] {
                for open in [format!("<{tag}>"), format!("<{tag} ")] {
                    assert!(!html.contains(&open), "docs/{}.md has a raw {open}: the browser hides everything after it", p.slug);
                }
            }
        }
    }

    /// The cookie notice names every cookie the app sets: each
    /// `const …COOKIE…: &str = "name"` under `src/` and each
    /// `document.cookie = "name=` under `static/` (#977). A new cookie fails
    /// here until `docs/cookies.md` documents it.
    #[test]
    fn cookie_notice_names_every_cookie() {
        fn walk(dir: &std::path::Path, ext: &str, out: &mut Vec<String>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, ext, out);
                } else if p.extension().is_some_and(|x| x == ext) {
                    out.push(std::fs::read_to_string(&p).unwrap_or_default());
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let (mut rs, mut js) = (Vec::new(), Vec::new());
        walk(&root.join("src"), "rs", &mut rs);
        walk(&root.join("static"), "js", &mut js);
        let mut names: Vec<String> = Vec::new();
        for line in rs.iter().flat_map(|f| f.lines()) {
            let Some((lhs, rhs)) = line.split_once(": &str = \"") else { continue };
            if lhs.trim_start().starts_with("pub const ") || lhs.trim_start().starts_with("const ") {
                if lhs.rsplit(' ').next().is_some_and(|id| id.contains("COOKIE")) {
                    names.extend(rhs.split('"').next().map(str::to_string));
                }
            }
        }
        for part in js.iter().flat_map(|f| f.split("document.cookie = \"").skip(1)) {
            names.extend(part.split('=').next().map(str::to_string));
        }
        for must in ["sid", "oauth", "signup", "twofa", "lang", "tz"] {
            assert!(names.iter().any(|n| n == must), "the scan missed the {must} cookie: {names:?}");
        }
        let md = String::from_utf8_lossy(&Docs::get("cookies.md").unwrap().data).into_owned();
        for n in &names {
            assert!(md.contains(&format!("| `{n}` |")), "docs/cookies.md has no table row for the `{n}` cookie");
        }
    }

    /// Every relative link in every docs page names a file that is served.
    #[test]
    fn every_docs_link_resolves() {
        for on in [true, false] {
            for p in pages(on) {
                let html = render_doc(&md_of(&p.slug, on).unwrap());
                for href in html.split("href=\"").skip(1).map(|h| h.split('"').next().unwrap()) {
                    let Some(rest) = href.strip_prefix(&format!("{}/docs/", crate::base_path())) else { continue };
                    let file = rest.split(['#', '?']).next().unwrap();
                    let served = if file.contains('.') { Docs::get(file).is_some() } else { md_of(file, on).is_some() };
                    assert!(served, "docs/{}.md links to {href}, which is not served (themes on: {on})", p.slug);
                }
            }
        }
    }

    /// The paths the "Where things live" map at the top of
    /// docs/architecture.md names: words made of path characters, with
    /// a trailing `/` dropped.
    fn architecture_map_paths() -> std::collections::HashSet<String> {
        let doc = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/docs/architecture.md")).expect("docs/architecture.md");
        let start = doc.find("\n## Where things live\n").expect("docs/architecture.md has a \"## Where things live\" section");
        let rest = &doc[start + 1..];
        let section = rest.find("\n## ").map_or(rest, |end| &rest[..end]);
        section
            .split(|c: char| !(c.is_ascii_alphanumeric() || "_./-".contains(c)))
            .map(|w| w.trim_end_matches(['/', '.']).to_string())
            .filter(|w| !w.is_empty())
            .collect()
    }

    /// Every `.rs` file under `dir`, relative to `src/`. `tests.rs` files
    /// hold a module's tests and need no line of their own.
    fn rust_files(dir: &std::path::Path, src: &std::path::Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("read src") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                rust_files(&path, src, out);
            } else if path.extension().is_some_and(|e| e == "rs") && path.file_name().is_some_and(|n| n != "tests.rs") {
                let rel = path.strip_prefix(src).expect("under src").to_string_lossy().replace('\\', "/");
                out.push(rel);
            }
        }
    }

    /// The map names every file in `src/` (as `ai.rs`, `routes/sprint.rs`,
    /// with or without `src/`) and every folder directly under `static/`,
    /// `templates/` and `data/` (#912). A new module fails here until it
    /// has its line.
    #[test]
    fn architecture_map_names_every_file() {
        let named = architecture_map_paths();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let src = root.join("src");
        let mut files = Vec::new();
        rust_files(&src, &src, &mut files);
        let mut missing: Vec<String> = files
            .into_iter()
            .filter(|f| !named.contains(f) && !named.contains(&format!("src/{f}")))
            .map(|f| format!("src/{f}"))
            .collect();
        for top in ["static", "templates", "data"] {
            for entry in std::fs::read_dir(root.join(top)).expect("read folder") {
                let path = entry.expect("dir entry").path();
                if path.is_dir() {
                    let rel = format!("{top}/{}", path.file_name().unwrap_or_default().to_string_lossy());
                    if !named.contains(&rel) {
                        missing.push(format!("{rel}/"));
                    }
                }
            }
        }
        missing.sort();
        assert!(
            missing.is_empty(),
            "docs/architecture.md \"Where things live\" does not name: {}. Add each to the line for its area.",
            missing.join(", ")
        );
    }
}
