// The playthrough loop, the same for every mode: a new account opens the
// course and plays the mode's lessons in order. Jev plays each lesson
// (player.ts) up to `attempts` times; a lesson it can't pass is marked
// failed and passed for it through the agents' API (the "fudge"), so the run
// always reaches the end of the course. After every lesson it checks that
// every page agrees on what is passed, and saves a checkpoint of the
// database to start a server from (scripts/jev-playthrough.sh serve).
import { chromium, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join, relative } from "node:path";
import { costUSD, MODEL, usage } from "../lib/jev.ts";
import { apiProgress, lockedNext, progressMismatches, register, type ApiLesson } from "../lib/learner.ts";
import type { Mode } from "./modes.ts";
import { play, readLesson, type Graded, type PhaseResult, type Trace } from "./player.ts";
import { Findings, type Kind } from "./watch.ts";

export interface Options {
  base: string;
  course: string;
  lessons: number; // stop after this many lessons
  attempts: number;
  maxCalls: number;
  headed: boolean;
  out: string; // the run folder
  db: string; // the server's database, for checkpoints ("" = none)
  failOn: Kind[];
  traceAll: boolean; // traces for every lesson, not only the failed ones
  repo: string;
}

interface Lesson {
  slug: string;
  title: string;
  status: "passed" | "failed" | "not played";
  attempts: {
    n: number;
    phases: Omit<PhaseResult, "graded">[];
    right: number;
    wrong: number;
    passed: boolean;
    app: AppStatus; // what the app says about the lesson after the try
    why?: string; // why it didn't pass, in words
    calls: number;
    seconds: number;
  }[];
  wrong: Graded[]; // every wrong answer, all attempts
  fudge?: string; // why and how it was passed for Jev
  checkpoint?: string;
  trace?: string; // every question put to Jev on this lesson
}

/** The app's own record of a lesson (/api/progress). */
interface AppStatus {
  seen: boolean;
  passed: boolean;
  best_pct: number | null;
  status: string;
}

/** Why a lesson isn't passed, from the rule in docs/progress.md: seen, and one run of 70 % or more. */
function whyNot(a: AppStatus, mode: Mode): string | undefined {
  if (a.passed) return undefined;
  const parts = [];
  if (!a.seen) parts.push(`not seen: nothing in ${mode.title} counted as teaching it (in Study mode, the reading check must be passed)`);
  if ((a.best_pct ?? 0) < 70) parts.push(a.best_pct === null ? "no counted run (too few answers?)" : `best run ${a.best_pct} %, under 70 %`);
  return parts.join("; ") || a.status;
}

export async function run(mode: Mode, o: Options): Promise<number> {
  const t0 = Date.now();
  mkdirSync(join(o.out, "shots"), { recursive: true });
  const findings = new Findings(mode.id, o.out);
  const stamp = new Date().toISOString().replace(/\D/g, "").slice(4, 14);
  const account = { email: `jev-${mode.id}-${stamp}@example.com`, password: `pt-${Math.random().toString(36).slice(2, 12)}` };
  const lessons: Lesson[] = [];
  let end = "", total = 0;

  const browser = await chromium.launch({ headless: !o.headed });
  const page = await (await browser.newContext({ viewport: mode.viewport, baseURL: o.base })).newPage();
  findings.watch(page);
  const checked = new Set<string>();
  const onPage = async (url: string) => {
    const path = new URL(url).pathname;
    if (checked.has(path)) return;
    checked.add(path);
    await findings.checkPage(page);
  };

  const api = () => apiProgress(page);

  const save = () => writeReport(mode, o, { account, lessons, findings: findings.list, end, total, seconds: Math.round((Date.now() - t0) / 1000) });

  try {
    // ---- the account ----
    await register(page, account, o.course);
    console.log(`==> ${mode.title}: account ${account.email} / ${account.password}, course ${o.course}`);
    writeFileSync(join(o.out, "account.json"), JSON.stringify({ ...account, base: o.base, course: o.course }, null, 2));

    // ---- the lessons ----
    for (let played = 0; ; ) {
      await page.goto(mode.index);
      const items = await page.getByTestId(mode.item).evaluateAll((els) =>
        els.map((e) => ({ slug: e.getAttribute("data-lesson-id")!, passed: e.getAttribute("data-passed") === "true", locked: e.getAttribute("data-state") === "locked" })),
      );
      total = items.length;
      if (!total) throw new Error(`${mode.index} lists no lessons ([data-testid=${mode.item}])`);
      const next = items.find((i) => !i.passed);
      if (!next) { end = `Every lesson in ${mode.title} is passed.`; break; }
      if (next.locked) {
        await findings.flag(page, "invariant", "locked", `${next.slug} is the first lesson not passed, but it is locked`);
        end = `Stopped: ${next.slug} is locked.`;
        break;
      }
      if (played >= o.lessons) { end = `Stopped after ${o.lessons} lessons (--lessons).`; break; }
      if (usage.calls >= o.maxCalls) { end = `Stopped at ${o.maxCalls} Jev calls (--max-calls).`; break; }
      played++;

      const info = (await api()).find((l) => l.slug === next.slug);
      const lesson: Lesson = { slug: next.slug, title: info?.title ?? next.slug, status: "not played", attempts: [], wrong: [] };
      lessons.push(lesson);
      findings.where.lesson = lesson.slug;
      const read = await readLesson(page, `/lessons/${encodeURIComponent(lesson.slug)}`);
      const traces: (Trace & { attempt: number })[] = [];

      for (let n = 1; n <= o.attempts && lesson.status !== "passed"; n++) {
        const t = Date.now(), calls = usage.calls;
        const phases: PhaseResult[] = [];
        for (const phase of mode.phases(lesson.slug)) {
          findings.where.phase = phase.name;
          const r = await play(page, phase, { avoid: mode.avoid, skip: mode.skip, advice: mode.advice, lesson: read, onPage, trace: (t) => traces.push({ attempt: n, ...t }) });
          phases.push(r);
          if (r.outcome !== "done") await findings.flag(page, "stuck", r.outcome, `${phase.name} not finished: ${r.note ?? r.outcome}; last: ${r.log.slice(-4).join(" → ")}`);
        }
        await page.waitForLoadState("networkidle").catch(() => {}); // the last answers and the finish call
        const now = (await api()).find((l) => l.slug === lesson.slug);
        const app: AppStatus = { seen: !!now?.seen, passed: !!now?.passed, best_pct: now?.best_pct ?? null, status: now?.status ?? "missing from /api/progress" };
        const passed = app.passed;
        const graded = phases.flatMap((p) => p.graded);
        lesson.wrong.push(...graded.filter((g) => !g.right));
        lesson.attempts.push({
          n,
          phases: phases.map(({ graded: _, ...p }) => p),
          right: graded.filter((g) => g.right).length,
          wrong: graded.filter((g) => !g.right).length,
          passed,
          app,
          why: whyNot(app, mode),
          calls: usage.calls - calls,
          seconds: Math.round((Date.now() - t) / 1000),
        });
        if (passed) lesson.status = "passed";
      }
      findings.where.phase = "";

      if (lesson.status !== "passed") {
        lesson.status = "failed";
        lesson.fudge = await fudge(page, lesson, info, findings);
      }
      if (lesson.status === "failed" || o.traceAll) {
        const file = join(o.out, "traces", `${String(lessons.length).padStart(2, "0")}-${lesson.slug}.jsonl`);
        mkdirSync(join(o.out, "traces"), { recursive: true });
        writeFileSync(file, traces.map((t) => JSON.stringify(t)).join("\n") + "\n");
        lesson.trace = relative(o.out, file);
      }
      await checkPages(page, findings, mode, await api());
      lesson.checkpoint = checkpoint(o, lessons.length, lesson.slug);
      const a = lesson.attempts.at(-1)!;
      const done = (await api()).filter((l) => l.passed).length;
      console.log(
        `${String(done).padStart(3)}/${total}  ${lesson.status === "passed" ? "✓" : "✗"} ${lesson.slug.padEnd(34)} ` +
          `${lesson.attempts.length} tr${lesson.attempts.length === 1 ? "y" : "ies"}  last ${a.right}/${a.right + a.wrong} right  ${a.calls} calls  ${a.seconds}s` +
          (lesson.fudge ? `  → ${lesson.fudge} (${a.why})` : ""),
      );
      save();
    }
  } catch (e) {
    end = `Failed: ${(e as Error).message.split("\n")[0]}`;
    await findings.flag(page, "bug", "fatal", end);
  } finally {
    save();
    await browser.close();
  }

  const failing = findings.list.filter((f) => o.failOn.includes(f.kind));
  const failed = lessons.filter((l) => l.status === "failed").length;
  console.log(`==> ${end}`);
  console.log(
    `==> ${mode.title}: ${lessons.length - failed} of ${lessons.length} played lessons passed by Jev, ${failed} failed and fudged; ` +
      `${findings.list.length} findings (${failing.length} failing the run); Jev ${usage.calls} calls ≈ $${costUSD().toFixed(4)}`,
  );
  console.log(`==> report: ${relative(o.repo, join(o.out, "report.md"))}`);
  return failing.length || end.startsWith("Failed") ? 1 : 0;
}

/** Pass a lesson Jev failed, through the agents' API, so the run can go on. */
async function fudge(page: Page, lesson: Lesson, info: ApiLesson | undefined, findings: Findings): Promise<string> {
  const n = Math.max(info?.required_answers ?? 3, 1);
  const r = await page.request.post("/api/progress/pass", {
    data: { lesson: lesson.slug, answered: n, correct: n, evidence: `Playthrough: Jev failed ${lesson.attempts.length} attempts; passed artificially to go on.` },
  });
  const body = await r.text();
  if (!r.ok() || !JSON.parse(body).passed) {
    await findings.flag(null, "invariant", "fudge", `POST /api/progress/pass for ${lesson.slug} did not pass it: HTTP ${r.status()} ${body.slice(0, 200)}`);
    return "fudge refused";
  }
  return "fudged";
}

/** Every page that lists lessons agrees with /api/progress on what is passed. */
async function checkPages(page: Page, findings: Findings, mode: Mode, truth: ApiLesson[]) {
  const last = truth.filter((l) => l.passed).at(-1)?.slug;
  for (const m of await progressMismatches(page, truth, last ? [`/lessons/${last}`] : [])) {
    if (m.status !== 200) continue; // the response watcher has it
    await findings.flag(page, "invariant", "passed", m.what);
  }
  // The mode's own list: the next lesson after a pass is open.
  const locked = await lockedNext(page, mode.index, mode.item, truth);
  if (locked) await findings.flag(page, "invariant", "unlock", locked.replace(` on ${mode.index} `, ` in ${mode.title} `));
}

/** A copy of the database (and its key) after this lesson. */
function checkpoint(o: Options, n: number, slug: string): string | undefined {
  if (!o.db || !existsSync(o.db)) return undefined;
  const dir = join(o.out, "checkpoints");
  mkdirSync(dir, { recursive: true });
  const file = join(dir, `${String(n).padStart(2, "0")}-${slug}.db`);
  try {
    execFileSync("sqlite3", [o.db, `.backup '${file}'`], { stdio: "ignore" });
    if (existsSync(`${o.db}.key`)) copyFileSync(`${o.db}.key`, `${file}.key`);
    return relative(o.repo, file);
  } catch {
    return undefined; // no sqlite3: no checkpoints
  }
}

// ---------- the report ----------

interface Report {
  account: { email: string; password: string };
  lessons: Lesson[];
  findings: Findings["list"];
  end: string;
  total: number;
  seconds: number;
}

function writeReport(mode: Mode, o: Options, r: Report) {
  const failed = r.lessons.filter((l) => l.status === "failed");
  const passedByJev = r.lessons.filter((l) => l.status === "passed");
  const failing = r.findings.filter((f) => o.failOn.includes(f.kind));
  const summary = {
    mode: mode.id,
    title: mode.title,
    ok: failing.length === 0 && !r.end.startsWith("Failed"),
    end: r.end,
    lessons_total: r.total,
    lessons_played: r.lessons.length,
    passed_by_jev: passedByJev.length,
    failed: failed.map((l) => l.slug),
    findings: r.findings.length,
    failing_findings: failing.length,
    jev: { model: MODEL, calls: usage.calls, input_tokens: usage.inputTokens, usd: Number(costUSD().toFixed(4)) },
    minutes: Math.round(r.seconds / 6) / 10,
  };
  writeFileSync(join(o.out, "report.json"), JSON.stringify({ summary, account: r.account, server: o.base, findings: r.findings, lessons: r.lessons }, null, 2));
  const pct = (l: Lesson) => {
    const a = l.attempts.at(-1);
    return a && a.right + a.wrong ? `${Math.round((100 * a.right) / (a.right + a.wrong))} %` : "–";
  };
  const md = [
    `# ${mode.title} playthrough`,
    "",
    `**${summary.ok ? "OK" : "FAILED"}**: ${r.end}`,
    "",
    `- Jev passed **${passedByJev.length}** of ${r.lessons.length} lessons played (${r.total} in the mode); **${failed.length}** failed and were passed artificially.`,
    `- Findings: ${r.findings.length} (${failing.length} of the kinds that fail the run: ${o.failOn.join(", ") || "none"}).`,
    `- Account \`${r.account.email}\` / \`${r.account.password}\` on ${o.base}. Jev (${MODEL}): ${usage.calls} calls, ≈ $${costUSD().toFixed(4)}; ${summary.minutes} min.`,
    "",
    "## Findings",
    "",
    ...(r.findings.length
      ? r.findings.map((f) => `- **${f.kind}** (${f.signal}${f.count > 1 ? `, ×${f.count}` : ""}): ${f.what}. At \`${f.url}\`, lesson \`${f.lesson || "–"}\`${f.phase ? `, ${f.phase}` : ""}.${f.screenshot ? ` [screenshot](${f.screenshot})` : ""}`)
      : ["None."]),
    "",
    "## Lessons",
    "",
    "| # | Lesson | Result | Tries | Last try right | Wrong answers | Checkpoint |",
    "|---|---|---|---|---|---|---|",
    ...r.lessons.map((l, i) => `| ${i + 1} | ${l.title} (\`${l.slug}\`) | ${l.status === "passed" ? "✓ Jev" : `✗ ${l.fudge ?? l.status}`} | ${l.attempts.length} | ${pct(l)} | ${l.wrong.length} | ${l.checkpoint ? `\`${l.checkpoint.split("/").pop()}\`` : "–"} |`),
    "",
    "## Lessons Jev failed",
    "",
    "Too hard, unclear, or broken: the wrong answers with the app's feedback, and how each try ended.",
    "",
    ...(failed.length
      ? failed.flatMap((l) => [
          `### ${l.title} (\`${l.slug}\`)`,
          "",
          ...l.attempts.map(
            (a) =>
              `- **Try ${a.n}**: ${a.right}/${a.right + a.wrong} answers right. The app says: *${a.app.status}*${a.why ? `. Not passed because: **${a.why}**` : ""}. ` +
              `Phases: ${a.phases.map((p) => `${p.phase} ${p.outcome}${p.note ? ` (${p.note})` : ""}`).join(", ")}.`,
          ),
          "",
          "Wrong answers (Jev's confidence in brackets; a low one means it was guessing):",
          "",
          ...l.wrong.map((g) => `- ✗ ${g.phase}: *${g.question.slice(0, 160)}* → ${g.actions.slice(-4).join(" → ") || "–"}. **App:** ${g.feedback}`),
          "",
          ...(l.trace ? [`Every question put to Jev on this lesson: [\`${l.trace}\`](${l.trace}) (one JSON line each: state, options, choice, confidence, runners-up).`, ""] : []),
        ])
      : ["None."]),
  ].join("\n");
  writeFileSync(join(o.out, "report.md"), md + "\n");
}
