//! The documentation under `docs/`, served inside the app so a link can be
//! handed to a person or an LLM. `/docs/<page>` renders the Markdown;
//! `/docs/<page>.md` returns it raw; `/llms.txt` lists the pages an LLM needs
//! to write a course. Pages are grouped into sections ([`SECTIONS`]): user
//! guides and reference first, developer pages last (and any page not listed,
//! so a new file shows up without a code change).

use askama::Template;
use axum::extract::Path;
use axum::http::{header, HeaderValue};
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;
use std::sync::Mutex;

use super::{Docs, Page};
use crate::auth::{CurrentUser, MaybeUser};
use crate::error::AppError;

#[derive(Clone)]
pub struct DocLink {
    pub slug: String,
    pub title: String,
}

/// One heading of the docs sidebar and its pages.
#[derive(Clone)]
pub struct DocSection {
    pub id: &'static str,
    /// The heading's i18n key (`docs.section_*`).
    pub title: &'static str,
    pub pages: Vec<DocLink>,
}

fn md_of(slug: &str) -> Option<String> {
    Docs::get(&format!("{slug}.md")).map(|f| String::from_utf8_lossy(&f.data).into_owned())
}

/// The sidebar, in order: (id, title key in `i18n/<code>.yaml`, page slugs).
/// User pages are listed by name; the last section also takes every page not
/// named anywhere.
pub const SECTIONS: &[(&str, &str, &[&str])] = &[
    ("guides", "docs.section_guides", &["index", "getting-started", "course-structure", "making-courses", "faq"]),
    (
        "self-hosting",
        "docs.section_self_hosting",
        &["setup-home-wifi", "setup-tailscale", "self-hosting", "self-hosting-faq"],
    ),
    (
        "reference",
        "docs.section_reference",
        &["course-format", "features", "progress", "i18n", "data-inventory"],
    ),
    (
        "dev",
        "docs.section_dev",
        &["architecture", "testing"],
    ),
];

#[derive(Template)]
#[template(path = "docs.html")]
pub struct DocsTemplate {
    pub user: Option<CurrentUser>,
    pub title: String,
    pub slug: String,
    pub html: std::sync::Arc<str>,
    pub sections: &'static [DocSection],
}

fn title_of(md: &str, slug: &str) -> String {
    md.lines()
        .find_map(|l| l.strip_prefix("# ").map(|t| t.trim().to_string()))
        .unwrap_or_else(|| slug.replace('-', " "))
}

/// Every embedded `.md` file, as (slug, title).
pub fn pages() -> Vec<DocLink> {
    let mut names: Vec<String> = Docs::iter().map(|n| n.to_string()).filter(|n| n.ends_with(".md")).collect();
    names.sort();
    // README-style ordering: overview first, then the rest alphabetically.
    names.sort_by_key(|n| if n == "index.md" { 0 } else { 1 });
    names
        .into_iter()
        .filter_map(|n| {
            let slug = n.trim_end_matches(".md").to_string();
            let body = md_of(&slug)?;
            Some(DocLink { title: title_of(&body, &slug), slug })
        })
        .collect()
}

/// The sidebar: every embedded page, in its section.
pub fn sections() -> Vec<DocSection> {
    let mut all = pages();
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
pub async fn llms_txt() -> Response {
    let base = crate::env("PUBLIC_URL")
        .map(|u| u.trim().trim_end_matches('/').to_string())
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| crate::base_path().to_string());
    (
        [(header::CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"))],
        llms_body().replace("{base}", &base),
    )
        .into_response()
}

fn llms_body() -> String {
    Docs::get("llms.txt").map(|f| String::from_utf8_lossy(&f.data).into_owned()).unwrap_or_default()
}

/// GET /docs
pub async fn index(MaybeUser(user): MaybeUser) -> Result<Response, AppError> {
    render("index", user)
}

/// GET /docs/{page}  (or {page}.md for the raw Markdown)
pub async fn page(MaybeUser(user): MaybeUser, Path(page): Path<String>) -> Result<Response, AppError> {
    if let Some(slug) = page.strip_suffix(".md") {
        let md = md_of(slug).ok_or(AppError::NotFound)?;
        return Ok((
            [(header::CONTENT_TYPE, HeaderValue::from_static("text/markdown; charset=utf-8"))],
            md,
        )
            .into_response());
    }
    if let Some(name) = page.strip_suffix(".json") {
        let f = Docs::get(&format!("{name}.json")).ok_or(AppError::NotFound)?;
        return Ok((
            [(header::CONTENT_TYPE, HeaderValue::from_static("application/json; charset=utf-8"))],
            f.data.into_owned(),
        )
            .into_response());
    }
    render(&page, user)
}

fn render(slug: &str, user: Option<CurrentUser>) -> Result<Response, AppError> {
    if !slug.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(AppError::NotFound);
    }
    let page = rendered(slug).ok_or(AppError::NotFound)?;
    Ok(Page(DocsTemplate { user, title: page.title, slug: slug.to_string(), html: page.html, sections: sections_cached() }).into_response())
}

/// A rendered page: its title and body HTML.
#[derive(Clone, Debug, PartialEq)]
struct Rendered {
    title: String,
    html: std::sync::Arc<str>,
}

/// A page, rendered on its first visit and kept for
/// the life of the process (#39). The body depends only on the page and the
/// base path, both fixed at startup; the signed-in user and the sidebar's
/// current-page marker live in the template. Only pages that exist are
/// stored, so the map is bounded by the docs.
fn rendered(slug: &str) -> Option<Rendered> {
    static CACHE: Mutex<Option<HashMap<String, Rendered>>> = Mutex::new(None);
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    if let Some(hit) = map.get(slug) {
        return Some(hit.clone());
    }
    let md = md_of(slug)?;
    let entry = Rendered { title: title_of(&md, slug), html: std::sync::Arc::<str>::from(render_doc(&md)) };
    map.insert(slug.to_string(), entry.clone());
    Some(entry)
}

/// [`sections`], built once.
fn sections_cached() -> &'static [DocSection] {
    static CACHE: std::sync::OnceLock<Vec<DocSection>> = std::sync::OnceLock::new();
    CACHE.get_or_init(sections)
}

#[cfg(test)]
thread_local! {
    /// Markdown renders on this thread, for the test that a second request
    /// of a page does not parse again.
    pub(crate) static RENDERS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Markdown to HTML for a docs page. The files link to each other relatively
/// (`[Features](features)`), which reads right on GitHub but resolves against the
/// wrong folder from `/docs` (no trailing slash), so relative links become
/// `/docs/<page>`. Every link also gets a testid, and links to another page
/// its slug, so a test can tell them apart.
pub(crate) fn render_doc(md: &str) -> String {
    #[cfg(test)]
    RENDERS.with(|c| c.set(c.get() + 1));
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

/// Give every heading a GitHub-style id ("Listening material (optional)" ->
/// `listening-material-optional`), so `page#heading` links land where they do
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

    /// A page is parsed on its first request only (#39).
    #[test]
    fn page_is_rendered_once() {
        let first = rendered("faq").expect("page exists");
        assert_eq!(RENDERS.with(|c| c.get()), 1);
        assert_eq!(rendered("faq"), Some(first));
        assert_eq!(RENDERS.with(|c| c.get()), 1, "second request parsed again");
        assert!(rendered("no-such-page").is_none());
    }

    #[test]
    fn relative_links_point_into_docs() {
        let base = crate::base_path();
        assert_eq!(doc_href("features"), (format!("{base}/docs/features"), Some("features".into())));
        assert_eq!(doc_href("course-format#lessons"), (format!("{base}/docs/course-format#lessons"), Some("course-format".into())));
        assert_eq!(doc_href("course-format.md"), (format!("{base}/docs/course-format.md"), None));
        assert_eq!(doc_href("https://example.com/x"), ("https://example.com/x".into(), None));
        assert_eq!(doc_href("mailto:a@b.c"), ("mailto:a@b.c".into(), None));
        assert_eq!(doc_href("#heading"), ("#heading".into(), None));
        assert_eq!(doc_href("/login"), ("/login".into(), None));
    }

    #[test]
    fn headings_get_github_ids() {
        let html = render_doc("# Course format\n\n## Listening material (optional)\n\n## `quiz`\n\n## Notes\n\n## Notes\n");
        assert!(html.contains("<h2 id=\"listening-material-optional\">"), "{html}");
        assert!(html.contains("<h2 id=\"quiz\">"), "{html}");
        assert!(html.contains("<h2 id=\"notes-1\">"), "{html}");
    }

    /// Every page is in exactly one section and the named ones exist.
    #[test]
    fn every_page_has_one_section() {
        let secs = sections();
        let listed: Vec<&str> = secs.iter().flat_map(|s| s.pages.iter().map(|p| p.slug.as_str())).collect();
        let all = pages();
        assert_eq!(listed.len(), all.len());
        for p in &all {
            assert_eq!(listed.iter().filter(|s| **s == p.slug).count(), 1, "{} listed once", p.slug);
        }
        assert_eq!(secs[0].pages[0].slug, "index");
        for (_, _, slugs) in SECTIONS {
            for s in *slugs {
                assert!(Docs::get(&format!("{s}.md")).is_some(), "SECTIONS names docs/{s}.md, which does not exist");
            }
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
        let body = llms_body();
        assert!(body.contains("{base}/docs/course-format.md") && body.contains("{base}/docs/course.schema.json"));
        for link in body.split("]({base}/docs/").skip(1) {
            let file = link.split(')').next().unwrap();
            let served = match file.strip_suffix(".md") {
                Some(slug) => md_of(slug).is_some(),
                None => Docs::get(file).is_some(),
            };
            assert!(served, "llms.txt links to docs/{file}, which is not served");
        }
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
        let ids_of = |slug: &str| -> Vec<String> {
            let md = md_of(slug).unwrap_or_else(|| panic!("a page links to {slug}, which is not served"));
            render_doc(&md).split(" id=\"").skip(1).map(|h| h.split('"').next().unwrap().to_string()).collect()
        };
        for p in pages() {
            let md = md_of(&p.slug).unwrap();
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
                assert!(ids_of(&page).iter().any(|id| id == anchor), "docs/{}.md links to {href}, but {page} has no heading #{anchor}", p.slug);
            }
        }
    }

    /// A raw `<title>` (or another tag whose content the browser reads as
    /// text) in a doc's prose swallows the rest of the page: "<title> (copy)"
    /// cut /docs/making-courses off mid-list. Write `&lt;title&gt;` or use
    /// backticks.
    #[test]
    fn no_doc_swallows_the_rest_of_its_page() {
        for p in pages() {
            let md = String::from_utf8_lossy(&Docs::get(&format!("{}.md", p.slug)).unwrap().data).into_owned();
            let html = render_doc(&md).to_ascii_lowercase();
            for tag in ["title", "textarea", "style", "script", "xmp", "plaintext", "iframe", "noembed", "noframes", "noscript"] {
                for open in [format!("<{tag}>"), format!("<{tag} ")] {
                    assert!(!html.contains(&open), "docs/{}.md has a raw {open}: the browser hides everything after it", p.slug);
                }
            }
        }
    }

    /// Every relative link in every docs page names a file that is served.
    #[test]
    fn every_docs_link_resolves() {
        for p in pages() {
            let html = render_doc(&md_of(&p.slug).unwrap());
            for href in html.split("href=\"").skip(1).map(|h| h.split('"').next().unwrap()) {
                let Some(rest) = href.strip_prefix(&format!("{}/docs/", crate::base_path())) else { continue };
                let file = rest.split(['#', '?']).next().unwrap();
                let served = if file.contains('.') { Docs::get(file).is_some() } else { md_of(file).is_some() };
                assert!(served, "docs/{}.md links to {href}, which is not served", p.slug);
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
