// The exploration report (issue #40): findings worst first, the route map,
// and proposed e2e tests (journey entries with a suggested exact check,
// regression spec outlines, judged spec outlines, markup fixes), as
// report.json for tools and report.md for people. Also the diff against the
// profile's previous run and how many other profiles hit the same finding.
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { FLAG, stepLine, type Evidence, type Finding, type GoalRun, type Kind, type explorer } from "./explore";
import { PASS } from "./jev";

const REPO = fileURLToPath(new URL("../..", import.meta.url));
export const EXPLORE_DIR = join(REPO, "e2e", "results", "explore");

export type Severity = "high" | "medium" | "low";

export interface RankedFinding extends Finding {
  severity: Severity;
  /** How bad, 0..1: 1 for an exact bug; for a judgement, how far p is on the wrong side. */
  badness: number;
  /** Other profiles whose latest run has the same finding. */
  also_hit_by: string[];
  /** Compared with this profile's previous run. */
  status: "new" | "seen before" | "first run";
  /** One line: what the user hit. */
  hit: string;
}

export interface Proposals {
  journeys: { goal: string; id: string; steps: number; compare: { profile: string; steps: number }[]; check_from: string; code: string }[];
  regressions: { finding: string; what: string; start: string; viewport: string; actions: string[]; assertion: string; code: string }[];
  judged: { finding: string; statement: string; kind: Kind; page: string; p: number; threshold: string; needs_person?: string; code?: string }[];
  markup: { finding: string; testids: string[]; where: string[]; change: string }[];
}

export interface Report {
  version: 1;
  mode: "explore" | "replay";
  profile: string;
  who: string;
  persona_file?: string;
  /** For a replay: the report it replayed, and which of its exact findings came back. */
  replay_of?: string;
  replay?: { reproduced: string[]; gone: string[]; new: string[]; missing_steps: number };
  model: string;
  commit: string;
  started: string;
  seconds: number;
  options: Record<string, unknown>;
  summary: {
    goals_reached: number;
    goals_missed: number;
    steps: number;
    jev: { calls: number; input_tokens: number; ms: number };
    cost_usd: number;
    findings: Record<Kind, number>;
  };
  goals: GoalRun[];
  route: { visited: string[]; never_reached: { path: string; label: string }[] };
  findings: RankedFinding[];
  /** Gone: seen last time on a page this run visited too, not seen now. Not revisited: its pages weren't visited this time. */
  diff?: { previous: string; new: string[]; gone: { id: string; kind: Kind; what: string }[]; not_revisited: string[] };
  proposals: Proposals;
  visits: ReturnType<ReturnType<typeof explorer>["result"]>["visits"];
  judgements: ReturnType<ReturnType<typeof explorer>["result"]>["judgements"];
}

export interface ReportInput {
  mode: Report["mode"];
  profile: string;
  who: string;
  personaFile?: string;
  replayOf?: Report;
  replayOfPath?: string;
  model: string;
  commit: string;
  started: Date;
  seconds: number;
  options: Record<string, unknown>;
  jev: { calls: number; inputTokens: number; ms: number };
  costUSD: number;
  result: ReturnType<ReturnType<typeof explorer>["result"]>;
  /** This run's folder, so it isn't its own "previous run". */
  dir: string;
}

const KIND_ORDER: Kind[] = ["bug", "content", "unclear", "frustrating", "testability"];
const pathOf = (url: string) => url.split("?")[0];

/** The latest report of each profile in e2e/results/explore/ (explore runs only), not counting `skip`. */
export function latestReports(skip?: string): Map<string, { dir: string; report: Report }> {
  const out = new Map<string, { dir: string; report: Report }>();
  if (!existsSync(EXPLORE_DIR)) return out;
  const dirs = readdirSync(EXPLORE_DIR)
    .map((d) => join(EXPLORE_DIR, d))
    .filter((d) => d !== skip && existsSync(join(d, "report.json")))
    .sort((a, b) => statSync(a).mtimeMs - statSync(b).mtimeMs);
  for (const dir of dirs) {
    try {
      const report = JSON.parse(readFileSync(join(dir, "report.json"), "utf8")) as Report;
      if (report.mode === "explore") out.set(report.profile, { dir, report });
    } catch {
      // a half-written report from an interrupted run: ignore it
    }
  }
  return out;
}

export function buildReport(input: ReportInput): Report {
  const { result } = input;
  const others = latestReports(input.dir);
  const previous = input.mode === "explore" ? others.get(input.profile) : undefined;
  others.delete(input.profile);
  const prevIds = new Set(previous?.report.findings.map((f) => f.id) ?? []);

  const findings: RankedFinding[] = result.findings.map((f) => {
    const ps = f.pages.map((p) => p.p).filter((p): p is number => p !== undefined);
    const badness =
      f.kind === "bug" ? 1 : f.kind === "testability" ? 0 : f.signal === "frustration" ? Math.max(...ps) : 1 - Math.min(...ps);
    const also = [...others].filter(([, o]) => o.report.findings.some((x) => x.id === f.id)).map(([id]) => id);
    const severity: Severity =
      f.kind === "bug" ? "high" : f.kind === "testability" ? "low" : badness >= 0.8 || also.length > 0 ? "medium" : "low";
    return {
      ...f,
      severity,
      badness: Math.round(badness * 100) / 100,
      also_hit_by: also,
      status: previous ? (prevIds.has(f.id) ? "seen before" : "new") : "first run",
      hit: hitLine(f),
    };
  });
  // Exact bugs first, then judged findings by how many profiles hit them and how bad, then markup.
  const tier = (k: Kind) => (k === "bug" ? 0 : k === "testability" ? 2 : 1);
  findings.sort(
    (a, b) =>
      tier(a.kind) - tier(b.kind) ||
      b.also_hit_by.length - a.also_hit_by.length ||
      b.badness - a.badness ||
      b.pages.length - a.pages.length ||
      KIND_ORDER.indexOf(a.kind) - KIND_ORDER.indexOf(b.kind),
  );

  const visited = result.route.map(pathOf);
  const seen = new Set(visited);
  const never_reached = Object.entries(result.linked)
    .filter(([p]) => !seen.has(p))
    .map(([path, label]) => ({ path, label }));

  const count = Object.fromEntries(KIND_ORDER.map((k) => [k, findings.filter((f) => f.kind === k).length])) as Record<Kind, number>;
  const report: Report = {
    version: 1,
    mode: input.mode,
    profile: input.profile,
    who: input.who,
    ...(input.personaFile && { persona_file: input.personaFile }),
    model: input.model,
    commit: input.commit,
    started: input.started.toISOString(),
    seconds: Math.round(input.seconds),
    options: input.options,
    summary: {
      goals_reached: result.goals.filter((g) => g.reached).length,
      goals_missed: result.goals.filter((g) => !g.reached).length,
      steps: result.goals.reduce((n, g) => n + g.steps.length, 0),
      jev: { calls: input.jev.calls, input_tokens: input.jev.inputTokens, ms: input.jev.ms },
      cost_usd: Number(input.costUSD.toFixed(5)),
      findings: count,
    },
    goals: result.goals,
    route: { visited: result.route, never_reached },
    findings,
    proposals: { journeys: [], regressions: [], judged: [], markup: [] },
    visits: result.visits,
    judgements: result.judgements,
  };
  if (previous) {
    const now = new Set(findings.map((f) => f.id));
    const missing = previous.report.findings.filter((f) => !now.has(f.id));
    const revisited = (f: Finding) => f.pages.some((p) => seen.has(pathOf(p.page)));
    report.diff = {
      previous: relative(REPO, previous.dir),
      new: findings.filter((f) => !prevIds.has(f.id)).map((f) => f.id),
      gone: missing.filter(revisited).map((f) => ({ id: f.id, kind: f.kind, what: f.what })),
      not_revisited: missing.filter((f) => !revisited(f)).map((f) => f.id),
    };
  }
  if (input.replayOf) {
    const was = input.replayOf.findings.filter((f) => f.kind === "bug" || f.kind === "testability");
    const now = new Set(findings.map((f) => f.id));
    const before = new Set(was.map((f) => f.id));
    report.replay_of = input.replayOfPath;
    report.replay = {
      reproduced: was.filter((f) => now.has(f.id)).map((f) => f.id),
      gone: was.filter((f) => !now.has(f.id)).map((f) => f.id),
      new: findings.filter((f) => !before.has(f.id)).map((f) => f.id),
      missing_steps: result.goals.reduce((n, g) => n + g.steps.filter((s) => s.replay && s.replay !== "done").length, 0),
    };
  }
  report.proposals = propose(report, others);
  return report;
}

function hitLine(f: Finding): string {
  const p = f.pages[0];
  const where = pathOf(p.page);
  switch (f.kind) {
    case "bug":
      return f.signal === "no-effect" ? `On ${where}, ${f.what}.` : f.signal === "lost" || f.signal === "stuck" ? `Gave up on ${where}: ${f.what}.` : `On ${where}: ${f.what}.`;
    case "unclear":
      return `On ${where}, Jev doubts that "${f.what}" (p=${p.p}).`;
    case "frustrating":
      return `On ${where}, "${f.what}" (p=${p.p}).`;
    case "content":
      return `On ${where}, ${JSON.stringify(p.item)} fails "${f.what}" (p=${p.p}).`;
    case "testability":
      return `On ${where}: ${f.what}.`;
  }
}

// ---- proposals ----------------------------------------------------------

const q = (s: string) => JSON.stringify(s);
const slug = (s: string) =>
  s
    .toLowerCase()
    .replace(/`[^`]*`/g, "")
    .replace(/[^a-z0-9 ]+/g, " ")
    .split(/\s+/)
    .filter((w) => w && !["the", "a", "an", "and", "to", "of", "on", "from", "you", "your", "that", "it", "in", "for", "with"].includes(w))
    .slice(0, 5)
    .join("-");

/** A step-log action ("Click the link "Lessons" [nav-lessons]") as Playwright code. */
export function actionCode(label: string): string {
  if (label === "Go back") return "await page.goBack();";
  const tidOf = (rest: string) => / \[([\w-]+)\]$/.exec(rest)?.[1];
  const clean = (t: string) => t.replace(/ \[(selected|marked right|marked wrong)(, [^\]]*)?\]$/, "").replace(/ \([^)]*\)$/, "").slice(0, 40).trim();
  let m = /^Click the (link|button) "(.*)"( \[[\w-]+\])?$/.exec(label);
  if (m) {
    const [, role, text] = m;
    const tid = tidOf(label);
    const t = clean(text);
    const loc = tid ? `page.getByTestId(${q(tid)})${t ? `.filter({ hasText: ${q(t)} })` : ""}.first()` : `page.getByRole(${q(role)}, { name: ${q(t)} }).first()`;
    return `await ${loc}.click();`;
  }
  m = /^Type "(.*)" into the "(.*)" box( \[[\w-]+\])?$/.exec(label);
  if (m) {
    const tid = tidOf(label);
    return `await ${tid ? `page.getByTestId(${q(tid)}).first()` : `page.getByLabel(${q(clean(m[2]))})`}.fill(${q(m[1])});`;
  }
  m = /^Choose "(.*)" in "(.*)"( \[[\w-]+\])?$/.exec(label);
  if (m) {
    const tid = tidOf(label);
    return `await ${tid ? `page.getByTestId(${q(tid)}).first()` : `page.getByLabel(${q(clean(m[2]))})`}.selectOption(${q(m[1])});`;
  }
  m = /^Select the option "(.*)"( \[[\w-]+\])?$/.exec(label);
  if (m) {
    const tid = tidOf(label);
    const t = clean(m[1]);
    return `await ${tid ? `page.getByTestId(${q(tid)}).filter({ hasText: ${q(t)} }).first()` : `page.getByLabel(${q(t)})`}.check();`;
  }
  return `// ${label}`;
}

const viewportCode = (v: string) => (v === "phone" ? `  await page.setViewportSize({ width: 390, height: 844 });\n` : "");

function propose(report: Report, others: Map<string, { report: Report }>): Proposals {
  const out: Proposals = { journeys: [], regressions: [], judged: [], markup: [] };
  const visits = report.visits;

  // Goals reached: a journeys.ts entry with a suggested exact check from the final page's markup.
  for (const g of report.goals) {
    if (!g.reached || report.mode === "replay") continue;
    const mine = g.visits.map((i) => visits[i]);
    const final = mine.find((v) => v.why === "goal reached") ?? mine.at(-1);
    if (!final) continue;
    // The final page is observed while walking and again at "goal reached": compare with the other pages.
    const earlier = mine.filter((v) => v.n < final.n && v.url !== final.url);
    const before = new Set(earlier.flatMap((v) => v.testids));
    const fresh = final.testids.filter((t) => !before.has(t) && !t.startsWith("nav-"));
    fresh.sort((a, b) => a.length - b.length); // the shortest new testid is usually the page's container
    // The strongest check is a data-* value new on the final page; best of all one the URL names (a lesson's slug).
    const seenState = new Set(earlier.flatMap((v) => Object.entries(v.state).flatMap(([t, a]) => Object.entries(a).map(([k, x]) => `${t} ${k}=${x}`))));
    const states = Object.entries(final.state)
      .filter(([t]) => !t.startsWith("nav-"))
      .flatMap(([t, a]) => Object.entries(a).map(([k, v]) => ({ t, k, v })))
      .filter(({ t, k, v }) => v && !seenState.has(`${t} ${k}=${v}`))
      .sort((a, b) => Number(final.url.includes(b.v)) - Number(final.url.includes(a.v)) || Number(fresh.includes(b.t)) - Number(fresh.includes(a.t)));
    let check: string, from: string;
    if (g.check) {
      check = g.check;
      from = "the profile's own exact check";
    } else if (states.length && (final.url.includes(states[0].v) || fresh.includes(states[0].t))) {
      const { t, k, v } = states[0];
      // A count answers at once; getAttribute would wait for the element on every step that lacks it.
      check = `async (page) => (await page.locator(${q(`[data-testid="${t}"][data-${k}="${v}"]`)}).count()) > 0`;
      from = `data-${k}="${v}" on [${t}], new on the final page${final.url.includes(v) ? " and named in its URL" : ""}`;
    } else if (fresh.length) {
      check = `(page) => page.getByTestId(${q(fresh[0])}).first().isVisible()`;
      from = `[${fresh[0]}], which first appeared on the final page` + (fresh.length > 1 ? ` (also new there: ${fresh.slice(1, 5).join(", ")})` : "");
    } else {
      check = `async (page) => new URL(page.url()).pathname === ${q(pathOf(final.url))}`;
      from = "the final page's path: no testid was new there, so consider adding one";
    }
    // A journey has no `topic`: write the topic in.
    const fill = (s: string) => (g.topic === undefined ? s : s.replace(/ \(the `topic`\)/g, "").replace(/`topic`/g, q(g.topic)));
    const id = `${report.profile}-${slug(fill(g.goal))}`;
    const compare = [...others]
      .flatMap(([profile, o]) => o.report.goals.filter((x) => x.goal === g.goal && x.reached).map((x) => ({ profile, steps: x.steps.length })));
    const code = [
      "{",
      `  id: ${q(id)},`,
      `  persona: ${q(report.profile)},`,
      ...(g.viewport === "phone" ? ["  phone: true,"] : []),
      `  start: ${q(g.start)},`,
      `  goal: ${q(fill(g.goal))},`,
      `  done: ${q(fill(g.done))},`,
      `  check: ${check.replace(/\n\s*/g, " ")},`,
      `  maxSteps: ${Math.max(4, g.steps.length + 3)},`,
      "},",
    ].join("\n");
    out.journeys.push({ goal: g.goal, id, steps: g.steps.length, compare, check_from: from, code });
  }

  for (const f of report.findings) {
    if (f.kind === "bug") out.regressions.push(regression(f, report));
    else if (f.kind === "testability") out.markup.push(markup(f));
    else out.judged.push(judgedSpec(f, report));
  }
  return out;
}

function regression(f: RankedFinding, report: Report): Proposals["regressions"][number] {
  const g = report.goals[f.evidence.goal];
  const route = (g?.steps ?? []).slice(0, f.evidence.step).filter((s) => s.action);
  // The shortest repro worth trying first: open the page before the bug and take the last action.
  // The whole route stays in a comment, for when the state built up on the way matters.
  const lastStep = route.at(-1);
  const start = lastStep && !lastStep.url.startsWith("/dev") ? lastStep.url : g?.start ?? "/";
  const actions = lastStep ? [lastStep.action!] : [];
  const listen: Record<string, string> = {
    http: `page.on("response", (r) => r.request().isNavigationRequest() && r.status() >= 400 && errors.push(\`\${r.status()} \${r.url()}\`));`,
    request: `page.on("response", (r) => r.status() >= 400 && errors.push(\`\${r.status()} \${r.url()}\`));\n  page.on("requestfailed", (r) => errors.push(\`\${r.failure()?.errorText} \${r.url()}\`));`,
    console: `page.on("console", (m) => m.type() === "error" && errors.push(m.text()));`,
    pageerror: `page.on("pageerror", (e) => errors.push(e.message));`,
  };
  let assertion: string;
  const lines: string[] = [];
  if (listen[f.signal]) {
    assertion = "expect(errors).toEqual([])";
    lines.push(`  const errors: string[] = [];`, `  ${listen[f.signal]}`);
  } else if (f.signal === "overflow") {
    assertion = "the page is no wider than the screen";
  } else if (f.signal === "no-effect") {
    assertion = "the last action changes the page";
  } else {
    assertion = "the goal is reachable: promote it as a journey with an exact check instead";
  }
  const steps = actions.map(actionCode);
  if (f.signal === "no-effect" && steps.length) {
    const last = steps.pop()!;
    steps.push(`const before = await page.locator("main").innerText();`, last, `// TODO: assert the state this action should set; at least:`, `await expect(page.locator("main")).not.toHaveText(before);`);
  }
  const body = [
    `test(${q(`regression: ${f.what.slice(0, 80)}`)}, async ({ page }) => {`,
    ...lines,
    viewportCode(g?.viewport ?? "desktop").trimEnd(),
    `  await devLogin(page);`,
    ...(route.length > 1 ? [`  // Jev's whole route from ${g?.start ?? "/"}: ${route.map((s) => s.action).join(" → ").replace(/\n/g, " ")}`] : []),
    `  await page.goto(${q(start)});`,
    ...steps.map((s) => `  ${s}`),
    ...(listen[f.signal] ? [`  expect(errors).toEqual([]);`] : []),
    ...(f.signal === "overflow"
      ? [`  expect(await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)).toBeLessThanOrEqual(1);`]
      : []),
    ...(f.signal === "stuck" || f.signal === "lost" ? [`  // ${f.what}: see the step log; the route above is where the walk went wrong`] : []),
    `});`,
  ].filter(Boolean);
  return { finding: f.id, what: f.what, start, viewport: g?.viewport ?? "desktop", actions, assertion, code: body.join("\n") };
}

function judgedSpec(f: RankedFinding, report: Report): Proposals["judged"][number] {
  const worst = f.signal === "frustration" ? f.pages.reduce((a, b) => ((b.p ?? 0) > (a.p ?? 0) ? b : a)) : f.pages.reduce((a, b) => ((b.p ?? 1) < (a.p ?? 1) ? b : a));
  const p = worst.p ?? 0;
  const g = report.goals[f.evidence.goal];
  const threshold = f.signal === "frustration" ? `p < ${(1 - PASS).toFixed(2)} (1 − JEV_PASS)` : `p ≥ ${PASS} (JEV_PASS)`;
  const base = { finding: f.id, statement: f.what, kind: f.kind, page: worst.page, p, threshold };
  if (f.kind === "content") {
    return { ...base, needs_person: `A fact check: is ${JSON.stringify(worst.item)} wrong? If so, fix the lesson JSON; Jev is a lead, not a reviewer.` };
  }
  if (Math.abs(p - FLAG) < 0.15) {
    return { ...base, needs_person: `Borderline (p=${p}, flagged at ${FLAG}): look at the screenshot and decide whether this is a real problem before writing a spec.` };
  }
  if (f.what.includes("`actions`")) {
    return { ...base, needs_person: "Judged on the page's action labels: the exact `labels` check (testing-bot) is the better test." };
  }
  const topic = g?.topic !== undefined ? `, topic: ${q(g.topic)}` : "";
  const expectLine = f.signal === "frustration" ? `expect(p).toBeLessThan(1 - PASS);` : `expect(p).toBeGreaterThanOrEqual(PASS);`;
  const code = [
    `test(${q(`${pathOf(worst.page)}: ${f.what}`)}, async ({ page }, info) => {`,
    viewportCode(g?.viewport ?? "desktop").trimEnd(),
    `  await devLogin(page);`,
    `  await page.goto(${q(worst.page)});`,
    `  const state = { who: ${q(report.who)}${topic}, url: ${q(worst.page)}, page_text: words(await page.locator("main").innerText()) };`,
    `  const p = await judge(state, ${q(f.what)});`,
    "  info.annotations.push({ type: \"jev\", description: `p=${p.toFixed(2)}` });",
    `  ${expectLine}`,
    `});`,
  ].filter(Boolean);
  return { ...base, code: code.join("\n") };
}

let markupIndex: { file: string; line: number; text: string }[] | undefined;
/** Lines in templates/ and static/*.js that mention a testid. */
function whereIs(testid: string): string[] {
  if (!markupIndex) {
    markupIndex = [];
    for (const dir of ["templates", "static"]) {
      const root = join(REPO, dir);
      if (!existsSync(root)) continue;
      for (const name of readdirSync(root)) {
        if (!/\.(html|js)$/.test(name)) continue;
        readFileSync(join(root, name), "utf8")
          .split("\n")
          .forEach((text, i) => text.includes("testid") && markupIndex!.push({ file: `${dir}/${name}`, line: i + 1, text }));
      }
    }
  }
  const re = new RegExp(`["'\`]${testid.replace(/[-]/g, "\\-")}["'\`]`);
  return markupIndex.filter((l) => re.test(l.text)).map((l) => `${l.file}:${l.line}`).slice(0, 4);
}

function markup(f: RankedFinding): Proposals["markup"][number] {
  const testids = [...new Set([...f.what.matchAll(/\[([\w-]+)\]/g)].map((m) => m[1]))];
  const where = testids.flatMap(whereIs);
  const on = testids.length ? testids.map((t) => `[${t}]`).join(", ") : "the element";
  const change: Record<string, string> = {
    labels: `Give the control in ${on} an aria-label (or title) that says what it does, so a test can find it by name.`,
    "class-state": `Mirror the class state in an attribute on ${on}: data-state="…" (or aria-pressed / aria-selected / aria-current), so a test can assert it exactly.`,
    "hidden-menus": `These links are only reachable by opening the menu ${on}: keep a testid on its <summary> so tests open it first, or show the links outside the closed menu.`,
    "error-role": `Add role="alert" to ${on}, so the error is announced and a test can assert getByRole("alert").`,
  };
  return { finding: f.id, testids, where, change: change[f.signal] ?? f.what };
}

// ---- output ---------------------------------------------------------------

const fmtCost = (r: Report) => `≈ $${r.summary.cost_usd.toFixed(5)}`;

export function summaryLine(r: Report): string {
  const s = r.summary;
  return (
    `${r.mode === "replay" ? "replay" : "explore"} ${r.profile}: ${s.goals_reached}/${s.goals_reached + s.goals_missed} goals reached, ${s.steps} steps, ` +
    `${s.jev.calls} Jev calls, ${s.jev.input_tokens} tokens, ${fmtCost(r)}, ${r.seconds}s, ${r.model}, commit ${r.commit}`
  );
}

/** What the terminal shows at the end of a run. */
export function terminalSummary(r: Report, dir: string): string {
  const out = [summaryLine(r)];
  const counts = Object.entries(r.summary.findings).filter(([, n]) => n).map(([k, n]) => `${n} ${k}`);
  out.push(`findings: ${counts.length ? counts.join(", ") : "none"}` + (r.diff ? ` (vs previous run: ${r.diff.new.length} new, ${r.diff.gone.length} gone, ${r.diff.not_revisited.length} not revisited)` : ""));
  for (const f of r.findings.slice(0, 15)) out.push(`  ${f.severity.padEnd(6)} ${f.kind.padEnd(11)} ${f.hit}${f.status === "new" ? "  [new]" : ""}`);
  if (r.findings.length > 15) out.push(`  … ${r.findings.length - 15} more in the report`);
  if (r.replay) out.push(`replay: ${r.replay.reproduced.length} reproduced, ${r.replay.gone.length} gone, ${r.replay.new.length} new; ${r.replay.missing_steps} steps could not be redone`);
  const p = r.proposals;
  out.push(`proposed tests: ${p.journeys.length} journeys, ${p.regressions.length} regression specs, ${p.judged.length} judged specs, ${p.markup.length} markup fixes`);
  out.push(`report: ${relative(REPO, join(dir, "report.md"))}`);
  return out.join("\n");
}

function evidenceMd(e: Evidence, r: Report): string[] {
  const g = r.goals[e.goal];
  const out = [`- Goal: ${g ? `"${g.goal}" (${g.viewport})` : "(between goals)"}, step ${e.step}, at \`${e.url}\``];
  if (e.jev) out.push(`- Jev: "${e.jev.statement}" → p=${e.jev.p}`);
  if (e.screenshot) out.push(`- Screenshot: [${e.screenshot}](${e.screenshot})`);
  if (e.steps.length) out.push("- Steps up to here:", "", "  ```", ...e.steps.map((s) => `  ${s}`), "  ```", "");
  const state = Object.entries(e.state).map(([t, a]) => `${t}{${Object.entries(a).map(([k, v]) => `${k}=${v}`).join(" ")}}`);
  if (e.testids.length) out.push(`- Testids: ${e.testids.slice(0, 30).join(", ")}${e.testids.length > 30 ? ", …" : ""}`);
  if (state.length) out.push(`- State: ${state.slice(0, 12).join(", ")}`);
  return out;
}

export function renderMarkdown(r: Report): string {
  const md: string[] = [];
  const title = r.mode === "replay" ? `Replay of ${r.profile}` : `Exploring as ${r.profile}`;
  md.push(`# ${title}`, "", `> ${r.who}`, "", summaryLine(r), "");
  md.push(`Started ${r.started}. Options: \`${JSON.stringify(r.options)}\`.${r.persona_file ? ` Profile file: \`${r.persona_file}\`.` : ""}`, "");
  if (r.replay) {
    md.push(
      `Replayed \`${r.replay_of}\` with no Jev calls: ${r.replay.reproduced.length} exact findings reproduced, ${r.replay.gone.length} gone, ${r.replay.new.length} new; ${r.replay.missing_steps} recorded steps could not be redone.`,
      "",
    );
    for (const id of r.replay.gone) md.push(`- gone: \`${id}\``);
    if (r.replay.gone.length) md.push("");
  }

  md.push("## Goals", "", "| Goal | Viewport | Reached | Steps | Why |", "|---|---|---|---|---|");
  for (const g of r.goals) md.push(`| ${g.goal.replace(/\|/g, "\\|")}${g.wander ? " *(wander)*" : ""} | ${g.viewport} | ${g.reached ? "yes" : "no"} | ${g.steps.length} | ${g.reason} |`);
  md.push("");

  md.push(`## Findings (${r.findings.length})`, "");
  if (r.diff) {
    md.push(`Compared with \`${r.diff.previous}\`: ${r.diff.new.length} new, ${r.diff.gone.length} gone, ${r.diff.not_revisited.length} not checked (their pages weren't visited this time).`, "");
    for (const g of r.diff.gone) md.push(`- gone: ${g.kind} — ${g.what}`);
    if (r.diff.gone.length) md.push("");
  }
  if (!r.findings.length) md.push("None.", "");
  r.findings.forEach((f, i) => {
    md.push(`### ${i + 1}. [${f.severity}] ${f.kind}: ${f.what.length > 110 ? `${f.what.slice(0, 110)}…` : f.what}`, "");
    md.push(`**What the user hit:** ${f.hit}`, "");
    const meta = [`signal \`${f.signal}\``, `${f.pages.length} page${f.pages.length === 1 ? "" : "s"}`, f.status];
    if (f.also_hit_by.length) meta.push(`also hit by ${f.also_hit_by.join(", ")}`);
    md.push(meta.join(" · "), "");
    if (f.pages.length > 1 || f.pages[0].item)
      md.push(...f.pages.slice(0, 6).map((p) => `- \`${p.page}\`${p.p !== undefined ? ` p=${p.p}` : ""}${p.item ? ` ${JSON.stringify(p.item)}` : ""}`), ...(f.pages.length > 6 ? [`- … ${f.pages.length - 6} more`] : []), "");
    md.push(...evidenceMd(f.evidence, r), "");
  });

  md.push("## Route map", "");
  md.push(r.route.visited.map((u) => `\`${u}\``).join(" → ") || "(nothing visited)", "");
  if (r.route.never_reached.length) {
    md.push("Linked from the header or the home page but never reached:", "");
    for (const n of r.route.never_reached) md.push(`- \`${n.path}\`${n.label ? ` (${n.label})` : ""}`);
    md.push("");
  }
  for (const g of r.goals) {
    md.push(`<details><summary>Step log: ${g.goal}</summary>`, "", "```", g.reason, ...g.steps.map((s) => stepLine(s) + (s.replay ? `  [replay: ${s.replay}]` : "")), "```", "", "</details>", "");
  }

  const p = r.proposals;
  md.push("## Proposed e2e tests", "");
  md.push("### Journeys (goals reached) → `e2e/journeys.ts`", "");
  if (!p.journeys.length) md.push("None: no goal was reached" + (r.mode === "replay" ? " (replays don't propose journeys)." : "."), "");
  for (const j of p.journeys) {
    const cmp = j.compare.length ? ` Other profiles: ${j.compare.map((c) => `${c.profile} in ${c.steps}`).join(", ")}.` : "";
    md.push(`**${j.goal}**: reached in ${j.steps} steps.${cmp} Check from ${j.check_from}.`, "", "```ts", j.code, "```", "");
  }
  md.push("### Regression specs (bugs) → `e2e/specs/exact.spec.ts`", "");
  if (!p.regressions.length) md.push("None.", "");
  for (const x of p.regressions) md.push(`**${x.what}**: asserts ${x.assertion}.`, "", "```ts", x.code, "```", "");
  md.push("### Judged specs (unclear, frustrating, content) → `e2e/specs/judged.spec.ts`", "");
  if (!p.judged.length) md.push("None.", "");
  for (const x of p.judged) {
    md.push(`**${x.statement}** (${x.kind}, p=${x.p} on \`${x.page}\`; would pass at ${x.threshold})`, "");
    if (x.needs_person) md.push(`Needs a person: ${x.needs_person}`, "");
    if (x.code) md.push("```ts", x.code, "```", "");
  }
  md.push("### Markup fixes (testability)", "");
  if (!p.markup.length) md.push("None.", "");
  for (const x of p.markup) md.push(`- ${x.change}${x.where.length ? ` (${x.where.map((w) => `\`${w}\``).join(", ")})` : ""}`);
  md.push("");
  return md.join("\n");
}
