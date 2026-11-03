// Fact check: Jev reads every unit of the built-in courses (a sentence and
// its translation, a line of a lesson, a quiz question, a sort item, a spot,
// a conjugation table, a vocabulary entry) and the
// report lists what is wrong, where, and the fix when there is one.
// Run by scripts/fact-check.sh; see docs/testing.md, "Fact check". How the
// questions are asked: probe.ts. How well it works: --calibrate.
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { costUSD, jevEnabled, MODEL, SKIP_REASON, usage } from "../lib/jev.ts";
import { trials, type Trial } from "./calibrate.ts";
import { Cache, hash, probeAll, type Result } from "./run.ts";
import { loadCourse, type Course, type Unit } from "./units.ts";

const REPO = fileURLToPath(new URL("../..", import.meta.url));
// FACT_CHECK_COURSES: another folder of courses (a copy with planted mistakes, say).
const COURSES = process.env.FACT_CHECK_COURSES || join(REPO, "data", "courses");
const CACHE_FILE = join(REPO, "e2e", "results", "fact-check", "cache.json");
const OK_FILE = "fact-check-ok.txt";

const USAGE = `usage: scripts/fact-check.sh [course-id…] [options]

  scripts/fact-check.sh                        every course in data/courses/
  scripts/fact-check.sh spanish                one course
  scripts/fact-check.sh spanish --lesson a2/   lessons whose file path contains the text
  scripts/fact-check.sh --errors               only ERROR findings (each has hard evidence)
  scripts/fact-check.sh --why                  also each finding's line for ${OK_FILE}
  scripts/fact-check.sh --json out.json        also write the findings as JSON
  scripts/fact-check.sh --calibrate [n]        measure it: plant mistakes in n units per kind (default 30)
  scripts/fact-check.sh --no-cache             ask Jev again about everything
  scripts/fact-check.sh --concurrency 8        parallel Jev calls (default 16)
  scripts/fact-check.sh --fail                 exit 1 when there is an ERROR

A finding checked and found right goes in data/courses/<id>/${OK_FILE}
(--why prints the line to paste); it is skipped until that item changes.`;

const opts = { courses: [] as string[], lesson: "", errors: false, why: false, json: "", cache: true, concurrency: 16, fail: false, calibrate: 0 };
{
  const a = process.argv.slice(2);
  const val = (i: number, name: string) => (a[i + 1] === undefined ? die(`${name} needs a value`) : a[i + 1]);
  for (let i = 0; i < a.length; i++) {
    switch (a[i]) {
      case "-h": case "--help": console.log(USAGE); process.exit(0);
      case "--lesson": opts.lesson = val(i++, "--lesson"); break;
      case "--errors": opts.errors = true; break;
      case "--why": opts.why = true; break;
      case "--json": opts.json = val(i++, "--json"); break;
      case "--no-cache": opts.cache = false; break;
      case "--concurrency": opts.concurrency = Math.max(1, Number(val(i++, "--concurrency")) || 1); break;
      case "--fail": opts.fail = true; break;
      case "--calibrate": opts.calibrate = /^\d+$/.test(a[i + 1] ?? "") ? Number(a[++i]) : 30; break;
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
  mark: (s: string) => (tty ? s.replace(/«([^»]*)»/g, "\x1b[1;4m$1\x1b[0m") : s),
};
const progress = (total: number) => (done: number, found: number) => {
  if (process.stderr.isTTY) process.stderr.write(`\r  ${done}/${total} units, ${found} findings, ${usage.calls} Jev calls `);
};

// ---------------------------------------------------------------- courses

const ids = readdirSync(COURSES).filter((d) => existsSync(join(COURSES, d, "course.json"))).sort();
for (const id of opts.courses) if (!ids.includes(id)) die(`no course "${id}" in ${relative(REPO, COURSES) || COURSES}; try: ${ids.join(" ")}`);
if (!jevEnabled) die(SKIP_REASON);
const courses = new Map<string, Course>();
for (const id of opts.courses.length ? opts.courses : ids) {
  try {
    courses.set(id, loadCourse(REPO, join(COURSES, id), opts.lesson));
  } catch (e) {
    die((e as Error).message);
  }
}

/** The line in a course's ok-list for a unit: its file, place and a hash of its content. */
const okLine = (u: Unit) => `${relative(join(COURSES, u.course), join(REPO, u.file))} ${u.at} ${hash(u).slice(0, 10)}`;
const okLists = new Map<string, Set<string>>();
for (const id of courses.keys()) {
  const f = join(COURSES, id, OK_FILE);
  okLists.set(id, new Set(existsSync(f) ? readFileSync(f, "utf8").split("\n").map((s) => s.replace(/#.*/, "").trim()).filter(Boolean) : []));
}

const cache = new Cache(CACHE_FILE, opts.cache);
const t0 = Date.now();
if (opts.calibrate) await calibrate();
else await check();

// ---------------------------------------------------------------- check

async function check() {
  const all = [...courses.values()].flatMap((x) => x.units);
  const units = all.filter((u) => !okLists.get(u.course)!.has(okLine(u)));
  const files = new Set(all.map((u) => u.file)).size;
  console.log(c.dim(`Fact check with ${MODEL}: ${units.length} units in ${files} files${all.length > units.length ? ` (${all.length - units.length} on the ok-list)` : ""}`));
  const { results, errors } = await probeAll(units, courses, cache, opts.concurrency, progress(units.length));
  if (process.stderr.isTTY) process.stderr.write("\r\x1b[K");
  cache.save();
  const found = results.filter((r) => r.finding && (!opts.errors || r.finding.verdict === "error"));
  report(found);
  if (opts.json)
    writeFileSync(opts.json, JSON.stringify({ model: MODEL, units: units.length, findings: found.map((r) => ({ course: r.unit.course, file: r.unit.file, at: r.unit.at, ...r.finding, ok_line: okLine(r.unit) })) }, null, 2));
  const nErr = found.filter((r) => r.finding!.verdict === "error").length;
  summary(units.length, files, found, nErr);
  if (errors.length) {
    console.log(c.red(`\n${errors.length} units could not be checked:`));
    for (const e of errors.slice(0, 20)) console.log("  " + e);
  }
  process.exit(errors.length ? 2 : opts.fail && nErr ? 1 : 0);
}

function report(found: Result[]) {
  const byFile = new Map<string, Result[]>();
  for (const r of found) byFile.set(r.unit.file, [...(byFile.get(r.unit.file) ?? []), r]);
  const rank = (r: Result) => (r.finding!.verdict === "error" ? 2 : 1) + r.finding!.p;
  for (const file of [...byFile.keys()].sort()) {
    console.log("\n" + c.bold(file));
    for (const r of byFile.get(file)!.sort((a, b) => rank(b) - rank(a))) {
      const f = r.finding!;
      const tag = f.verdict === "error" ? c.red("ERROR") : c.yellow("CHECK");
      console.log(`  ${tag}  ${r.unit.at}   ${c.bold(f.what)}  ${c.dim(f.p.toFixed(2))}`);
      for (const s of f.show) console.log(`         ${c.mark(s)}`);
      for (const e of f.evidence) console.log(c.dim(`         ${e}`));
      if (opts.why) console.log(c.dim(`         right after all? add to ${OK_FILE}: ${okLine(r.unit)}`));
    }
  }
}

function summary(n: number, files: number, found: Result[], nErr: number) {
  const kinds = new Map<string, number>();
  for (const r of found) kinds.set(r.finding!.what, (kinds.get(r.finding!.what) ?? 0) + 1);
  console.log(
    `\n${found.length ? c.bold(`${nErr} errors, ${found.length - nErr} to check`) : "Nothing found"} in ${n} units (${courses.size} course${courses.size > 1 ? "s" : ""}, ${files} files).`,
  );
  if (kinds.size) console.log(c.dim("  " + [...kinds].sort((a, b) => b[1] - a[1]).map(([k, v]) => `${k} ${v}`).join(" · ")));
  console.log(c.dim(`  Jev: ${usage.calls} calls, ${usage.inputTokens.toLocaleString()} input tokens, ≈ $${costUSD().toFixed(3)}; ${cache.hits} answers from cache; ${((Date.now() - t0) / 1000).toFixed(0)} s.`));
  if (found.length)
    console.log(c.dim(`  ERROR: Jev has specific evidence (a fix it prefers, its own answer). CHECK: Jev doubts it and says where. Verify both; --why for the ok-list line.`));
}

// ---------------------------------------------------------------- calibrate

async function calibrate() {
  for (const course of courses.values()) {
    const ts = trials(course, opts.calibrate);
    console.log(c.dim(`Calibrating ${course.id} with ${MODEL}: ${ts.length} units, ${opts.calibrate} per kind (untouched, and with one planted mistake)`));
    const { results, errors } = await probeAll(ts.map((t) => t.unit), courses, cache, opts.concurrency, progress(ts.length));
    if (process.stderr.isTTY) process.stderr.write("\r\x1b[K");
    cache.save();
    const byUnit = new Map<Unit, Result>(results.map((r) => [r.unit, r]));
    const rows = new Map<string, Trial[]>();
    for (const t of ts) rows.set(`${t.unit.kind}\t${t.mistake ?? ""}`, [...(rows.get(`${t.unit.kind}\t${t.mistake ?? ""}`) ?? []), t]);
    const pct = (a: number, b: number) => (b ? `${Math.round((100 * a) / b)}%` : "–").padStart(6);
    console.log(`\n${"unit".padEnd(9)} ${"mistake".padEnd(24)} ${"n".padStart(4)} ${"flagged".padStart(8)} ${"error".padStart(6)} ${"points".padStart(7)} ${"fixed".padStart(6)}`);
    for (const k of [...rows.keys()].sort()) {
      const [kind, mistake] = k.split("\t");
      const t = rows.get(k)!.filter((x) => byUnit.has(x.unit));
      const f = t.map((x) => ({ x, f: byUnit.get(x.unit)!.finding }));
      const flagged = f.filter((y) => y.f).length, err = f.filter((y) => y.f?.verdict === "error").length;
      const withWord = f.filter((y) => y.x.word);
      const points = withWord.filter((y) => y.f?.show.some((s) => s.toLowerCase().includes(`«${y.x.word!.toLowerCase()}»`))).length;
      const fixed = withWord.filter((y) => y.f?.fix && y.f.fix.to.toLowerCase() === y.x.original!.toLowerCase()).length;
      const line = `${kind.padEnd(9)} ${(mistake || "(untouched)").padEnd(24)} ${String(t.length).padStart(4)} ${pct(flagged, t.length).padStart(8)} ${pct(err, t.length)} ${withWord.length ? pct(points, withWord.length).padStart(7) : "".padStart(7)} ${withWord.length ? pct(fixed, withWord.length) : ""}`;
      console.log(mistake ? line : c.bold(line));
    }
    // The real course is the other half of the measurement: nearly every unit in it is right, so its
    // flags are (mostly) false alarms, counted over thousands of units instead of a sample.
    const real = await probeAll(course.units, courses, cache, opts.concurrency, progress(course.units.length));
    if (process.stderr.isTTY) process.stderr.write("\r\x1b[K");
    cache.save();
    const rf = real.results.filter((r) => r.finding);
    const re = rf.filter((r) => r.finding!.verdict === "error").length;
    console.log(c.bold(`\nthe real course: ${re} errors, ${rf.length - re} checks in ${course.units.length} units (${((100 * rf.length) / course.units.length).toFixed(2)} %)`));
    console.log(c.dim(`  these are false alarms unless the course really has mistakes there; read them with scripts/fact-check.sh ${course.id}`));
    console.log(c.dim(`\n  flagged: findings of either verdict. error: flagged as ERROR.`));
    console.log(c.dim(`  points: the finding marks the planted word. fixed: its fix puts back the original word.`));
    if (errors.length) console.log(c.red(`  ${errors.length} units failed: ${errors[0]}`));
  }
  console.log(c.dim(`  Jev: ${usage.calls} calls, ≈ $${costUSD().toFixed(3)}; ${cache.hits} answers from cache; ${((Date.now() - t0) / 1000).toFixed(0)} s.`));
}
