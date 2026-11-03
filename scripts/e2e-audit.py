#!/usr/bin/env python3
"""E2E coverage numbers for docs/e2e-audit.md (#235).

Greps the production routes (src/routes/mod.rs, before the dev-only block)
and the data-testid hooks (templates/, static/), greps everything under e2e/
that drives a browser (specs, lib, journeys, personas, playthrough, record),
and prints Markdown: spec files and whether CI runs them, routes named or
not, testids referenced or not, per template. A route reached only by
clicking a link is not "named"; read the misses with that in mind.

    python3 scripts/e2e-audit.py            # Markdown to stdout
    python3 scripts/e2e-audit.py --json     # the same numbers as JSON
    python3 scripts/e2e-audit.py --write    # refresh the generated half of docs/e2e-audit.md
"""
import json
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
E2E = ROOT / "e2e"


def read(p):
    return p.read_text(encoding="utf-8", errors="replace")


# --- e2e sources -----------------------------------------------------------
E2E_DIRS = ["specs", "lib", "personas", "playthrough", "record"]
e2e_files = [E2E / "journeys.ts"]
for d in E2E_DIRS:
    e2e_files += sorted((E2E / d).rglob("*.ts"))
e2e_text = "\n".join(read(f) for f in e2e_files if f.exists())
# `${anything}` in a template literal stands for one path segment / id part.
e2e_norm = re.sub(r"\$\{[^}]*\}", "\x00", e2e_text)

# --- spec files and CI -----------------------------------------------------
smoke_yml = read(ROOT / ".github/workflows/smoke.yml")
ci_specs = set()
for line in re.findall(r"^\s*specs:\s*(.+)$", smoke_yml, re.M):
    ci_specs.update(line.split())


def spec_in_ci(rel):  # rel: "specs/x.spec.ts"
    return rel in ci_specs or any(rel.startswith(s.rstrip("/") + "/") for s in ci_specs)


def count_tests(src):
    # Playwright test() calls, plus one per journey for the journeys spec.
    return len(re.findall(r"^\s*test(?:\.(?:skip|fixme|only))?\(", src, re.M))


specs = []
for f in sorted((E2E / "specs").rglob("*.spec.ts")):
    rel = f.relative_to(E2E).as_posix()
    src = read(f)
    jev = bool(re.search(r"@jev|jevEnabled|TYPESAFE_API_KEY", src))
    titles = re.findall(r"^\s*test(?:\.(?:skip|fixme|only))?\(\s*[\"'`](.+?)[\"'`]\s*,", src, re.M)
    specs.append({"file": rel, "tests": count_tests(src), "ci": spec_in_ci(rel), "jev": jev, "titles": titles})


def playwright_list():
    """Every test as Playwright lists it (loops expanded), or None without node_modules."""
    try:
        out = subprocess.run(
            ["npx", "playwright", "test", "--list", "--reporter=json"],
            cwd=E2E, capture_output=True, text=True, timeout=120,
            env={**__import__("os").environ, "E2E_BASE_URL": "http://127.0.0.1:1"},
        ).stdout
        suites = json.loads(out)["suites"]
    except Exception:
        return None
    got = {}

    def walk(s, file):
        file = s.get("file", file)
        for sp in s.get("specs", []):
            got.setdefault("specs/" + file, []).append(sp["title"])
        for c in s.get("suites", []):
            walk(c, file)

    for s in suites:
        walk(s, None)
    return got


listed = playwright_list()
if listed:
    for s in specs:
        if s["file"] in listed:
            s["titles"] = listed[s["file"]]
            s["tests"] = len(s["titles"])

# --- routes ----------------------------------------------------------------
mod = read(ROOT / "src/routes/mod.rs")
prod = mod[mod.index("let mut app = Router::new()") : mod.index("if state.dev {")]
routes = []
for path, handlers in re.findall(r'\.route\("([^"]+)",\s*(.+?)\)\s*$', prod, re.M):
    methods = sorted(set(re.findall(r"\b(get|post|put|delete|patch)\(", handlers)))
    rx = re.escape(path)
    rx = re.sub(r"\\\{\\\*[a-z_]+\\\}", r"[^\\s\"'`]+", rx)
    rx = re.sub(r"\\\{[a-z_]+\\\}", r"[^/\\s\"'`?#]+", rx)
    # bounded on the right so /courses does not count for /courses/new
    hit = re.search(r"[\"'`(]" + rx + r"(?=[\"'`?#)\s]|$)", e2e_norm, re.M) is not None
    if not hit and path.endswith(".md"):
        hit = path in e2e_text
    # Looser, for markup and scripts: a parameter is `{{ … }}`, `' + x + '`, `${x}`…
    loose = re.sub(r"\\\{(?:\\\*)?[a-z_]+\\\}",lambda _: r"(?:\{\{.*?\}\}|\$\{.*?\}|[^\"'\s]*?)", re.escape(path))
    routes.append({"path": path, "methods": methods, "named": hit, "loose": loose})

# --- testids ---------------------------------------------------------------
surface = {}  # testid (or prefix pattern) -> source file
for f in sorted(list((ROOT / "templates").glob("*.html")) + list((ROOT / "static").glob("*.js"))):
    src = read(f)
    found = re.findall(r'data-testid=\\?"([^"\\]+)', src)
    found += re.findall(r"data-testid=([a-z][a-z0-9-]+)", src)
    found += re.findall(r'dataset\.testid\s*=\s*["\']([^"\']+)', src)
    found += re.findall(r'setAttribute\(\s*["\']data-testid["\']\s*,\s*["\']([^"\']+)', src)
    for t in found:
        if re.fullmatch(r"[a-z0-9{][a-z0-9{}()_. -]*", t):  # not a JS concatenation
            surface.setdefault(t, f.relative_to(ROOT).as_posix())

# strings in the e2e sources that hold a testid, literal or templated
e2e_strings = set(re.findall(r'["\'`]([a-z][a-z0-9\x00-]*[a-z0-9\x00])["\'`]', e2e_norm))
e2e_strings |= set(re.findall(r'data-testid[\^\$\*]?=\\?["\']?([a-z][a-z0-9\x00-]+)', e2e_norm))
literal = {s for s in e2e_strings if "\x00" not in s}
templated = [re.compile("^" + re.escape(s).replace("\x00", "[a-z0-9-]+") + "$") for s in e2e_strings if "\x00" in s]
prefixes = set(re.findall(r'data-testid\^=\\?["\']?([a-z][a-z0-9-]+)', e2e_text))


def referenced(tid):
    if "{{" in tid or "{" in tid:
        # a templated hook: referenced when its fixed part is
        fixed = re.split(r"\{\{.*?\}\}|\{.*?\}", tid)
        pat = re.compile("^" + "[a-z0-9-]*".join(re.escape(p) for p in fixed) + "$")
        return any(pat.match(s) for s in literal) or any(p.startswith(fixed[0]) for p in prefixes if fixed[0])
    if tid in literal or any(r.match(tid) for r in templated):
        return True
    return any(tid.startswith(p) for p in prefixes)


ids = [{"testid": t, "file": f, "referenced": referenced(t)} for t, f in sorted(surface.items())]
visited = {i["file"] for i in ids if i["referenced"]}
ref_set = {i["testid"] for i in ids if i["referenced"]}
# A script counts as visited when a visited template loads it.
for f in (ROOT / "static").glob("*.js"):
    js = f.relative_to(ROOT).as_posix()
    if any(f'"{f.name}"' in read(ROOT / t) or js in read(ROOT / t) for t in list(visited) if t.startswith("templates/")):
        visited.add(js)

# A route no e2e file names may still be reached: a form or script on a page
# the specs drive calls it. "via" lists the templates and scripts naming it.
ui_text = {}
for f in sorted(list((ROOT / "templates").glob("*.html")) + list((ROOT / "static").glob("*.js"))):
    ui_text[f.relative_to(ROOT).as_posix()] = read(f)
smoke_sh = read(ROOT / "scripts/smoke.sh")
for r in routes:
    loose = r.pop("loose")
    pat = re.compile(r"(?:[\"'`]|\}\})" + loose + r"(?=[\"'`?#\s]|$|\{\{)", re.M)
    r["via"] = [f for f, t in ui_text.items() if pat.search(t)]
    r["smoke_sh"] = re.search(r"[\s\"'$]" + loose + r"(?=[\"'?\s]|$)", smoke_sh.replace("$URL", " "), re.M) is not None
    # In a template, the form or link that calls the route must itself be
    # driven: a referenced testid on the same line or the next two. A script
    # counts when a page the specs drive loads it.
    r["by"] = []
    for f in r["via"]:
        if f.endswith(".js"):
            if f in visited:
                r["by"].append(f"`{f}`")
            continue
        lines = ui_text[f].splitlines()
        for n, line in enumerate(lines):
            if pat.search(line):
                near = " ".join(lines[n : n + 3])
                hooks = [t for t in re.findall(r'data-testid="([^"]+)"', near) if t in ref_set]
                if hooks:
                    r["by"].append(f"`{hooks[0]}`")
                    break
    r["reached"] = r["named"] or bool(r["by"])

# --- journeys and personas -------------------------------------------------
journeys = re.findall(r'^\s*\{?\s*id:\s*"([^"]+)"', read(E2E / "journeys.ts"), re.M)
personas = sorted(p.stem for p in (E2E / "personas").glob("*.ts"))

data = {"specs": specs, "routes": routes, "testids": ids, "journeys": journeys, "personas": personas}
if "--json" in sys.argv:
    json.dump(data, sys.stdout, indent=1)
    sys.exit(0)

# --- Markdown --------------------------------------------------------------
# --write replaces everything after MARKER in docs/e2e-audit.md.
MARKER = "<!-- e2e-audit:generated -->"
real_stdout = sys.stdout
if "--write" in sys.argv:
    import io

    sys.stdout = io.StringIO()
commit = subprocess.run(["git", "-C", str(ROOT), "rev-parse", "--short", "HEAD"], capture_output=True, text=True).stdout.strip()
n_tests = sum(s["tests"] for s in specs)
n_ci = sum(s["tests"] for s in specs if s["ci"] and not s["jev"])
n_named = sum(r["named"] for r in routes)
n_ref = sum(i["referenced"] for i in ids)
print(f"<!-- generated by scripts/e2e-audit.py at {commit} -->\n")
print("| What | Count |\n|---|---|")
print(f"| Spec files | {len(specs)} ({sum(s['ci'] for s in specs)} run in CI) |")
n_local = sum(s["tests"] for s in specs if not s["ci"] and not s["jev"])
n_key = sum(s["tests"] for s in specs if s["jev"])
print(f"| Tests | {n_tests}: {n_ci} run in CI, {n_local} keyless but local only, {n_key} need a Jev key |")
n_reached = sum(r["reached"] for r in routes)
print(f"| Production routes (`src/routes/mod.rs`, no `/dev`) | {len(routes)}: {n_named} named by an e2e file, {n_reached - n_named} more reached through a page, {len(routes) - n_reached} not reached |")
print(f"| `data-testid` hooks (templates, static JS) | {len(ids)}, {n_ref} referenced ({100 * n_ref // max(len(ids), 1)} %) |")
print(f"| Model-driven journeys / personas | {len(journeys)} / {len(personas)} |")

print("\n### Spec files\n\n| File | Tests | CI | Needs a key |\n|---|---|---|---|")
for s in specs:
    print(f"| `{s['file']}` | {s['tests']} | {'yes' if s['ci'] else '**no**'} | {'yes' if s['jev'] else ''} |")

def route(r):
    return f"`{' '.join(m.upper() for m in r['methods'])} {r['path']}`"


print("\n### Routes reached only through a page\n")
print("No e2e file names them; a spec drives the form, link (its testid) or script that calls them.\n")
for r in routes:
    if not r["named"] and r["reached"]:
        print(f"- {route(r)}: " + ", ".join(dict.fromkeys(r["by"])))
print("\n### Routes no e2e flow reaches\n")
print("Not named by an e2e file, and no driven form, link or script calls them.\n")
for r in routes:
    if not r["reached"]:
        extra = []
        if r["via"]:
            extra.append("called from " + ", ".join(f"`{f}`" for f in r["via"]))
        if r["smoke_sh"]:
            extra.append("curled by `scripts/smoke.sh`")
        print(f"- {route(r)}" + (": " + "; ".join(extra) if extra else ""))

print("\n### Testids per template\n\n| File | Hooks | Referenced | Not referenced |\n|---|---|---|---|")
by_file = {}
for i in ids:
    by_file.setdefault(i["file"], []).append(i)
for f, xs in sorted(by_file.items(), key=lambda kv: -sum(not x["referenced"] for x in kv[1])):
    miss = [x["testid"] for x in xs if not x["referenced"]]
    shown = ", ".join(f"`{m}`" for m in miss[:12]) + (f" (+{len(miss) - 12})" if len(miss) > 12 else "")
    print(f"| `{f}` | {len(xs)} | {len(xs) - len(miss)} | {shown} |")

print("\n### Every test, by file\n")
print("Titles as Playwright lists them" + ("" if listed else " (from the source: run `npm ci` in e2e/ for loops expanded)") + ".\n")
for s in specs:
    where = "needs a key" if s["jev"] else ("CI" if s["ci"] else "local only")
    print(f"- **`{s['file']}`** ({s['tests']}, {where}): " + " · ".join(t.strip() for t in s["titles"]))

if "--write" in sys.argv:
    body, sys.stdout = sys.stdout.getvalue(), real_stdout
    doc = ROOT / "docs/e2e-audit.md"
    head = read(doc).split(MARKER)[0]
    # A line that names themes is fenced, so the docs read true with themes off (#1184).
    body = "\n".join(f"<!-- themes -->\n{l}\n<!-- /themes -->" if "theme" in l.lower() else l for l in body.split("\n"))
    doc.write_text(head + MARKER + "\n" + body, encoding="utf-8")
    print(f"wrote {doc.relative_to(ROOT)}")
