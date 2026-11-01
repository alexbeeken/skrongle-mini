# The app's text in other languages

Every piece of text the app itself shows lives here, one file per language:
`en.yaml` is the original, every other `<code>.yaml` (`es.yaml`, …) translates
it key for key. `cargo test` fails while a file lacks a key `en.yaml` has, has
one it does not, or uses other `{placeholders}` than the English text.

Courses are not translated here: a course is written in its own language by
its author, and a learner gets it in another as a translated copy. A built-in
theme's own text is: the `themes.<id>` group (`docs/i18n.md`, #463).

## Adding a language

1. Copy `en.yaml` to `<code>.yaml`, where `<code>` is the two-letter language
   code (`de`, `fr`, `pt`, …; it is also the page's `lang` attribute).
2. Set `meta.name` to the language's own name for itself (`Deutsch`).
3. Translate every value. Keep the keys, the `{placeholders}` and any HTML in a
   `_html` key exactly as they are.
4. `cargo build` (the files are compiled into the binary) and `cargo test`.

The language appears in the pickers at once, by its `meta.name`.

## The format

A small subset of YAML, read by `src/i18n.rs`:

```yaml
# a comment
nav:                      # a group (two-space indentation)
  login: Log in           # a plain text, to the end of the line
  hello: "Hi, {name}!"    # "double quotes": JSON escapes (\n, \", é)
  sure: 'It''s fine'      # 'single quotes': '' is one quote
  long: |                 # a block keeps its line breaks (|- drops the last one)
    First line.
    Second line.
  folded: >-              # a folded block joins its lines with spaces
    One sentence
    over two lines.
  lessons:                # a count: `one` for exactly 1, `other` otherwise
    one: "{n} lesson"
    other: "{n} lessons"
```

Keys are letters, digits, `_` and `-`, joined with dots in code: `nav.login`.
Quote a text that starts with a YAML special character (`[ { & * ! % @ ` - ? #`)
or contains `: ` or ` #`. Lists, anchors and multi-document files are not read.

## Conventions

- One group per area of the app (`auth`, `profile`, `study`, `play`,
  `sprint`, `courses`, …); the text a page's script draws goes in that area's
  `js` sub-group (`sprint.js.correct`), so the page can embed just that.
- `{name}` placeholders are filled by code: `t1("nav.hello", "name", name)`.
  Counts use `tn("study.lessons", n)` with `one` / `other` forms.
- A key ending in `_html` may hold markup (`<a>`, `<strong>`, …); the template
  renders it with `|safe`, so its placeholders only ever get trusted values
  (URLs, numbers), never anything a user typed.
- The product's name is a placeholder (`{app}`), never written into a text.
- Translate meaning, not words: buttons short, messages in the register the
  English uses, no machine-translation leftovers.

## Using it in code

```rust
crate::t("nav.login")                       // -> String, escaped by the template
crate::t1("nav.hello", "name", name)        // one placeholder
crate::t2("x.y", "a", 1, "b", 2)            // two
crate::tn("study.lessons", n)               // a count
```

Templates call the same functions: `{{ crate::t("nav.login") }}`. The
request's language is decided once per request (`i18n::middleware`): the
account's `users.lang`, else the `lang` cookie (the picker), else
`Accept-Language`, else English.

A script gets its area's texts from the page:

```html
{% block scripts %}
<script>Object.assign(I18N, {{ crate::i18n::js_json("sprint.js")|safe }});</script>
<script src="{{ crate::routes::asset_url("sprint.js") }}" defer></script>
{% endblock %}
```

and reads them with `t('sprint.js.correct')`, `t('sprint.js.left', {n: 3})`
or `tn('sprint.js.lessons', n)`; `common.js.*` is on every page.
