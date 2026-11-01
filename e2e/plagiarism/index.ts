// Provenance check: are the built-in courses original? Run by
// scripts/plagiarism-check.sh; see docs/testing.md, "Plagiarism check".
// Three layers, cheapest first: a lint of the briefs (no key), word-shingle
// overlap against reference texts and between lessons (no key), and Jev
// judging every passage (cached, cents). Output is a similarity-risk report,
// not a legal finding; no tool can prove an absence of copying.
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { costUSD, jevEnabled, MODEL, SKIP_REASON, usage } from "../lib/jev.ts";
import { Cache, hash } from "../fact-check/run.ts";
import { lint } from "./lint.ts";
import { loadCourse, type Course, type Passage } from "./passages.ts";
import { overlaps, selfOverlaps, type Hit, type Ref } from "./prefilter.ts";
import { PUBLIC_DOMAIN } from "./public-domain.ts";
import { decide, questions, stateOf, T, PROMPT_VERSION, type Finding } from "./probe.ts";

const REPO = fileURLToPath(new URL("../..", import.meta.url));
const COURSES = process.env.PLAGIARISM_COURSES || join(REPO, "data", "courses");
const TEST_COURSES = join(REPO, "data", "test-courses");
const CACHE_FILE = join(REPO, "e2e", "results", "plagiarism", "cache.json");
const OK_FILE = "plagiarism-ok.txt";

const USAGE = `usage: scripts/plagiarism-check.sh [course-id…] [options]

  scripts/plagiarism-check.sh                     lint + pre-filter + Jev, every built-in course
  scripts/plagiarism-check.sh spanish --lesson a2/ lessons whose file path contains the text
  scripts/plagiarism-check.sh --test-course       also the test course
  scripts/plagiarism-check.sh --prefilter-only    no key: lint + shingle overlap only
  scripts/plagiarism-check.sh --sources refs.txt  reference texts: a file or folder of .txt / .json
                                                  ([{name,text}]); keep non-public ones gitignored
  scripts/plagiarism-check.sh --errors            only ERRORs (hard evidence)
  scripts/plagiarism-check.sh --why               also each finding's line for ${OK_FILE}
  scripts/plagiarism-check.sh --json out.json     also write the findings as JSON
  scripts/plagiarism-check.sh --calibrate         measure the judge on public-domain text, and the pre-filter on edits
  scripts/plagiarism-check.sh --no-cache          ask Jev again about everything
  scripts/plagiarism-check.sh --fail              exit 1 on an ERROR (or a lint hit)

CHECK: Jev recognises it but cannot prove it; a person looks. ERROR: a verbatim run
of words shared with a reference, and Jev finds the wording distinctive.
A finding reviewed and found fine goes in data/courses/<id>/${OK_FILE}
(--why prints the line: file at hash # who, when, why).`;

const opts = { courses: [] as string[], lesson: "", testCourse: false, prefilterOnly: false, sources: "", errors: false, why: false, json: "", cache: true, concurrency: 16, fail: false, calibrate: false };
{
  const a = process.argv.slice(2);
  const val = (i: number, name: string) => (a[i + 1] === undefined ? die(`${name} needs a value`) : a[i + 1]);
  for (let i = 0; i < a.length; i++) {
    switch (a[i]) {
      case "-h": case "--help": console.log(USAGE); process.exit(0);
      case "--lesson": opts.lesson = val(i++, "--lesson"); break;
      case "--test-course": opts.testCourse = true; break;
      case "--prefilter-only": opts.prefilterOnly = true; break;
      case "--sources": opts.sources = val(i++, "--sources"); break;
      case "--errors": opts.errors = true; break;
      case "--why": opts.why = true; break;
      case "--json": opts.json = val(i++, "--json"); break;
      case "--no-cache": opts.cache = false; break;
      case "--concurrency": opts.concurrency = Math.max(1, Number(val(i++, "--concurrency")) || 1); break;
      case "--fail": opts.fail = true; break;
      case "--calibrate": opts.calibrate = true; break;
      default:
        if (a[i].startsWith("-")) die(`unknown option ${a[i]}\n\n${USAGE}`);
        opts.courses.push(a[i]);
    }
  }
}
function die(msg: string): never {
  console.error(msg);
  process.exit(2);
}

const tty = process.stdout.isTTY && !process.env.NO_COLOR;
const c = {
  red: (s: string) => (tty ? `\x1b[1;31m${s}\x1b[0m` : s),
  yellow: (s: string) => (tty ? `\x1b[33m${s}\x1b[0m` : s),
  dim: (s: string) => (tty ? `\x1b[2m${s}\x1b[0m` : s),
  bold: (s: string) => (tty ? `\x1b[1m${s}\x1b[0m` : s),
};

// ---------------------------------------------------------------- courses

const dirs = new Map<string, string>();
for (const root of [COURSES, ...(opts.testCourse ? [TEST_COURSES] : [])])
  if (existsSync(root)) for (const d of readdirSync(root).sort()) if (existsSync(join(root, d, "course.json"))) dirs.set(d, join(root, d));
const ids = [...dirs.keys()].filter((d) => opts.testCourse || dirs.get(d)!.startsWith(COURSES));
for (const id of opts.courses) if (!dirs.has(id)) die(`no course "${id}"; try: ${[...dirs.keys()].join(" ")} (--test-course for the test course)`);
const courses: Course[] = [];
for (const id of opts.courses.length ? opts.courses : ids) {
  try {
    courses.push(loadCourse(REPO, dirs.get(id)!, opts.lesson));
  } catch (e) {
    die((e as Error).message);
  }
}

function readSources(path: string): Ref[] {
  if (!existsSync(path)) die(`--sources: no such file or folder ${path}`);
  const one = (f: string): Ref[] =>
    f.endsWith(".json") ? (JSON.parse(readFileSync(f, "utf8")) as Ref[]).map((r) => ({ name: `${relative(REPO, f)}:${r.name}`, text: r.text }))
    : f.endsWith(".txt") || f.endsWith(".md") ? [{ name: relative(REPO, f), text: readFileSync(f, "utf8") }] : [];
  return existsSync(path) && readdirSync(path, { withFileTypes: true }).length && !path.match(/\.\w+$/)
    ? readdirSync(path).sort().flatMap((n) => one(join(path, n)))
    : one(path);
}
const refs: Ref[] = opts.sources ? readSources(opts.sources) : [];

const okLine = (p: Passage) => `${relative(join(dirs.get(p.course)!), join(REPO, p.file))} ${p.at} ${hash(p.text).slice(0, 10)}`;
const okList = (id: string) => {
  const f = join(dirs.get(id)!, OK_FILE);
  return new Set(existsSync(f) ? readFileSync(f, "utf8").split("\n").map((s) => s.replace(/\s+#.*$/, "").trim()).filter(Boolean) : []);
};

const cache = new Cache(CACHE_FILE, opts.cache);
const t0 = Date.now();
if (opts.calibrate) await calibrate();
else await check();

// ---------------------------------------------------------------- check

interface Row { p: Passage; finding: Finding | null; hits: Hit[] }

async function check() {
  let failed = false;
  const bad = lint(REPO);
  console.log(c.bold("Briefs") + c.dim(` (no document may tell an agent to imitate a product's text): ${bad.length ? bad.length + " to fix" : "clean"}`));
  for (const b of bad) console.log("  " + c.red("LINT ") + " " + b);
  if (bad.length) failed = true;

  const oks = new Map(courses.map((x) => [x.id, okList(x.id)]));
  const all = courses.flatMap((x) => x.passages);
  const passages = all.filter((p) => !oks.get(p.course)!.has(okLine(p)));
  const generic = courses.reduce((n, x) => n + x.generic, 0);
  console.log(c.dim(`\nPassages: ${passages.length} in ${new Set(all.map((p) => p.file)).size} files (${generic} generic ones under 12 words skipped${all.length > passages.length ? `, ${all.length - passages.length} on the ok-list` : ""})`));

  // Free pre-filters.
  const external = refs.length ? overlaps(passages, refs, 2) : new Map<Passage, Hit[]>();
  const internal = new Map<Passage, Hit[]>();
  // Only long shared runs count: a six-word shingle is a phrase, and lessons that teach the same thing share phrases.
  for (const x of courses.filter((x) => !dirs.get(x.id)!.startsWith(TEST_COURSES)))
    for (const [p, h] of selfOverlaps(x.passages.filter((p) => passages.includes(p)))) {
      const long = h.filter((y) => y.quote.split(/\s+/).length >= 10);
      if (long.length) internal.set(p, long);
    }
  console.log(c.dim(`Pre-filter: ${refs.length} reference texts, ${external.size} passages overlap them; ${internal.size} passages repeat text of another lesson (self-overlap, not provenance).`));
  for (const [p, h] of internal) console.log(`  ${c.yellow("REPEAT")} ${p.file} ${p.at}  ${c.dim(`«${h[0].quote}» also in ${h[0].other}`)}`);

  const rows: Row[] = passages.map((p) => ({ p, finding: null, hits: external.get(p) ?? [] }));
  const errors: string[] = [];
  if (!opts.prefilterOnly) {
    if (!jevEnabled) die(`${SKIP_REASON}\n(or --prefilter-only for the free checks)`);
    let next = 0, done = 0;
    const worker = async () => {
      while (next < rows.length) {
        const r = rows[next++];
        try {
          r.finding = decide(r.p, await cache.ask(stateOf(r.p), questions()), r.hits);
        } catch (e) {
          errors.push(`${r.p.file} ${r.p.at}: ${(e as Error).message}`);
        }
        if (++done % 25 === 0 && process.stderr.isTTY) process.stderr.write(`\r  ${done}/${rows.length} passages, ${usage.calls} Jev calls `);
      }
    };
    await Promise.all(Array.from({ length: Math.min(opts.concurrency, rows.length) }, worker));
    if (process.stderr.isTTY) process.stderr.write("\r\x1b[K");
    cache.save();
  } else {
    // Without Jev only a verbatim run is a finding, always a CHECK.
    for (const r of rows) {
      const h = r.hits.find((x) => x.quote.split(/\s+/).length >= T.run);
      if (h) r.finding = { verdict: "check", source: "none", p: 1, show: r.p.text.split("\n").slice(0, 2), evidence: [`shares «${h.quote}» with ${h.other}`] };
    }
  }

  const found = rows.filter((r) => r.finding && (!opts.errors || r.finding.verdict === "error"));
  const byFile = new Map<string, Row[]>();
  for (const r of found) byFile.set(r.p.file, [...(byFile.get(r.p.file) ?? []), r]);
  for (const file of [...byFile.keys()].sort()) {
    console.log("\n" + c.bold(file));
    for (const r of byFile.get(file)!.sort((a, b) => (b.finding!.verdict === "error" ? 2 : 1) + b.finding!.p - ((a.finding!.verdict === "error" ? 2 : 1) + a.finding!.p))) {
      const f = r.finding!;
      console.log(`  ${f.verdict === "error" ? c.red("ERROR") : c.yellow("CHECK")}  ${r.p.at}   ${c.bold("resembles " + f.source)}`);
      for (const s of f.show) console.log(`         ${s}`);
      for (const e of f.evidence) console.log(c.dim(`         ${e}`));
      if (opts.why) console.log(c.dim(`         reviewed and fine? add to ${OK_FILE}: ${okLine(r.p)}  # who, when, why`));
    }
  }
  const nErr = found.filter((r) => r.finding!.verdict === "error").length;
  console.log(`\n${found.length ? c.bold(`${nErr} errors, ${found.length - nErr} to check`) : "Nothing found"} in ${rows.length} passages (${courses.length} course${courses.length > 1 ? "s" : ""}).`);
  if (!opts.prefilterOnly) console.log(c.dim(`  Jev ${MODEL}: ${usage.calls} calls, ≈ $${costUSD().toFixed(3)}; ${cache.hits} answers from cache; ${((Date.now() - t0) / 1000).toFixed(0)} s.`));
  console.log(c.dim("  A clean run is weak evidence: in-app and paywalled content is not indexed anywhere, and Jev's memory of it is unmeasured. This is similarity risk, not infringement."));
  if (opts.json)
    writeFileSync(opts.json, JSON.stringify({ model: MODEL, promptVersion: PROMPT_VERSION, lint: bad, passages: rows.length, findings: found.map((r) => ({ course: r.p.course, file: r.p.file, at: r.p.at, ...r.finding, ok_line: okLine(r.p) })) }, null, 2));
  if (errors.length) {
    console.log(c.red(`\n${errors.length} passages could not be checked:`));
    for (const e of errors.slice(0, 20)) console.log("  " + e);
  }
  process.exit(errors.length ? 2 : opts.fail && (nErr || failed) ? 1 : 0);
}

// ---------------------------------------------------------------- calibrate

async function calibrate() {
  const pct = (a: number, b: number) => (b ? `${Math.round((100 * a) / b)} %` : "–");
  // 1. The pre-filter: our own passages, verbatim and lightly edited, against themselves as references.
  const own = courses.flatMap((x) => x.passages).filter((_, i) => i % 5 === 0).slice(0, 60);
  const refTexts: Ref[] = own.map((p) => ({ name: `${p.file} ${p.at}`, text: p.text }));
  const edits: [string, (t: string) => string][] = [
    ["verbatim", (t) => t],
    ["every 9th word swapped", (t) => t.split(/(\s+)/).map((w, i) => (i % 18 === 0 && i > 0 && /\p{L}/u.test(w) ? "xx" : w)).join("")],
    ["every 5th word swapped", (t) => t.split(/(\s+)/).map((w, i) => (i % 10 === 0 && i > 0 && /\p{L}/u.test(w) ? "xx" : w)).join("")],
  ];
  console.log(c.bold("Pre-filter") + c.dim(` (${own.length} passages of the course, edited, checked against the originals as references; flagged = a shared run of ${T.run}+ words)`));
  for (const [name, f] of edits) {
    const m = overlaps(own.map((p) => ({ ...p, text: f(p.text) })), refTexts, 1);
    const caught = [...m.values()].filter((h) => h.some((x) => x.quote.split(/\s+/).length >= T.run)).length;
    console.log(`  ${name.padEnd(26)} caught ${pct(caught, own.length).padStart(5)}`);
  }
  console.log(c.dim("  Shingles catch verbatim and near-verbatim reuse, not a paraphrase."));

  if (opts.prefilterOnly) return;
  if (!jevEnabled) die(`${SKIP_REASON}\n(or --prefilter-only for the free checks)`);

  // 2. Jev: public-domain text (positives) and the course's own passages (presumed original).
  const ctx = { course: "Spanish for English speakers", lesson: "Reading" };
  const pos: Passage[] = PUBLIC_DOMAIN.map((x) => ({ course: "pd", file: "public-domain", at: x.name, text: x.text, ctx }));
  const neg = courses.flatMap((x) => x.passages);
  const run = async (ps: Passage[]) => {
    const out: (Finding | null)[] = [];
    let i = 0;
    await Promise.all(Array.from({ length: Math.min(opts.concurrency, ps.length) }, async () => {
      while (i < ps.length) {
        const k = i++;
        out[k] = decide(ps[k], await cache.ask(stateOf(ps[k]), questions()));
      }
    }));
    return out;
  };
  console.log(c.bold("\nJev") + c.dim(` (${MODEL}): ${pos.length} public-domain passages and ${neg.length} course passages`));
  const rp = await run(pos), rn = await run(neg);
  cache.save();
  const flaggedPos = rp.filter(Boolean).length, flaggedNeg = rn.filter(Boolean).length;
  console.log(`  recall on published text      ${pct(flaggedPos, pos.length).padStart(5)}  (${flaggedPos}/${pos.length} flagged)`);
  console.log(`  false flags on the course     ${pct(flaggedNeg, neg.length).padStart(5)}  (${flaggedNeg}/${neg.length}; these are false alarms unless the course really copies)`);
  console.log(`  precision                     ${pct(flaggedPos, flaggedPos + flaggedNeg).padStart(5)}  (if every course flag is a false one)`);
  for (let k = 0; k < pos.length; k++) if (!rp[k]) console.log(c.dim(`  missed: ${pos[k].at}`));
  console.log(c.yellow("  Caveat: famous public-domain text is far more memorised than a modern app's exercises. Recall here overstates recall on the real target."));
  console.log(c.dim(`  Jev: ${usage.calls} calls, ≈ $${costUSD().toFixed(3)}; ${cache.hits} answers from cache; ${((Date.now() - t0) / 1000).toFixed(0)} s.`));
}
