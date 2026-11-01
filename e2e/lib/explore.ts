// Exploratory runs (issue #40): a persona walks its goals with the navigator
// and every page it reaches is observed in two layers.
//
//  - Exact signals, no model, are bugs: HTTP >= 400 on a navigation or a
//    fetch, failed requests, console and uncaught page errors, a page wider
//    than a phone screen, an action with no effect, a stuck or lost walk.
//  - Jev judgements: the persona's concerns and frustrations, one batched
//    call per new page, are "unclear" and "frustrating" findings; its item
//    probes are "content" findings; its markup checks are "testability".
//
// Each finding keeps evidence from where it was first seen: the step log up
// to then, a screenshot, the URL, the visible testids and their data-* state,
// and Jev's answer. lib/explore-report.ts turns it all into the report and
// the proposed tests. Exploration reports; it never fails a test by itself.
import type { Page } from "@playwright/test";
import { choose, judgeMany, usage } from "./jev";
import { navigate, redo, see, type Outcome, type Seen, type Step } from "./navigate";
import type { Journey } from "../journeys";
import type { CheckId, Persona, PersonaGoal, Statement, Viewport } from "./persona";

/** A concern under this, or a frustration at or above it, is a finding. */
export const FLAG = Number(process.env.JEV_FLAG || 0.5);
/** Pages judged per run at most: keeps a wandering persona's cost bounded. */
const MAX_PAGES = Number(process.env.JEV_EXPLORE_PAGES || 15);
const MAX_TEXT = 4_000;

export type Kind = "bug" | "content" | "unclear" | "frustrating" | "testability";
/** What raised a finding: an exact signal, a judged statement, or a markup check. */
export type Signal =
  | "http" | "request" | "console" | "pageerror" | "overflow" | "no-effect" | "stuck" | "lost"
  | "concern" | "frustration" | "item"
  | Exclude<CheckId, "overflow">;

export interface Evidence {
  /** Index into the run's goals, and the step in that goal's log. */
  goal: number;
  step: number;
  url: string;
  /** Relative to the run folder. */
  screenshot?: string;
  /** The step log up to this point (the last 8 steps). */
  steps: string[];
  /** Visible testids and their data-* state on the page. */
  testids: string[];
  state: Record<string, Record<string, string>>;
  /** For a judged finding: the statement and Jev's probability. */
  jev?: { statement: string; p: number };
}

export interface Finding {
  /** Stable across runs, for diffing: the signal, the page for exact bugs, and what was seen (digits folded). */
  id: string;
  kind: Kind;
  signal: Signal;
  /** The statement, the error, or the check's message. */
  what: string;
  /** Every page it was seen on, with Jev's probability when judged. */
  pages: { page: string; p?: number; item?: Record<string, string> }[];
  /** Where it was first seen. */
  evidence: Evidence;
}

export interface Visit {
  n: number;
  goal: number;
  step: number;
  url: string;
  why: string;
  testids: string[];
  state: Record<string, Record<string, string>>;
  screenshot: string;
}

export interface GoalRun {
  goal: string;
  done: string;
  start: string;
  viewport: Viewport;
  topic?: string;
  /** Picked by --wander rather than written in the profile. */
  wander?: boolean;
  /** The profile's exact check, as source, when it has one. */
  check?: string;
  reached: boolean;
  reason: string;
  steps: (Step & { replay?: "done" | "missing" | "failed" })[];
  /** Indices into `visits` of this goal's pages. */
  visits: number[];
}

export interface ExploreOptions {
  /** The run folder: screenshots go to <dir>/shots/. */
  dir: string;
  /** False for --replay: exact signals only, no Jev. */
  judge: boolean;
  /** Stop calling Jev after this many calls in all (0: no cap). */
  maxCalls: number;
}

/**
 * Page text with runs of one- and two-character lines (accent buttons like
 * "á" "¿", stat tiles like "0" "XP") joined into one line: a row of accents
 * reads as one line of noise instead of nine, and "0 XP" stays readable.
 */
export const words = (text: string) => {
  const out: string[] = [];
  let run: string[] = [];
  const flush = () => (run.length && out.push(run.join(" ")), (run = []));
  for (const line of text.split("\n").map((l) => l.trim()).filter(Boolean)) {
    if (line.length <= 2) run.push(line);
    else {
      flush();
      out.push(line);
    }
  }
  flush();
  return out.join("\n");
};

/** Wait (briefly) for entrance animations, so a screenshot or a measurement shows the settled page. */
const settle = (page: Page) =>
  page
    .waitForFunction(() => document.getAnimations().every((a) => a.playState !== "running" || a.effect?.getComputedTiming().iterations === Infinity), null, { timeout: 3_000 })
    .catch(() => {});

const text = (s: Statement) => (typeof s === "string" ? s : s.text);
const pathOf = (url: string) => url.split("?")[0];
const round = (x: number) => Math.round(x * 100) / 100;
const fold = (s: string) => s.replace(/\d+/g, "#");
const SHOW_STEPS = 8;

/** One line per step, as in the journey logs. */
export const stepLine = (s: Step) =>
  `${s.n}. ${s.url}  done=${s.done}` + (s.action ? `  → ${s.action} (conf ${s.confidence}${s.guess ? ", guess" : ""})` : "") + (s.note ? `  !! ${s.note}` : "");

export function explorer(persona: Persona, opts: ExploreOptions) {
  const judged = new Set<string>(); // page keys judged or checked already
  const findings = new Map<string, Finding>();
  const judgements: { page: string; why: string; kind: string; statement: string; p: number }[] = [];
  const goals: GoalRun[] = [];
  const visits: Visit[] = [];
  const route: string[] = [];
  const linked = new Map<string, string>(); // path -> label, linked from the header or the home page
  let cur = { goal: -1, steps: [] as readonly Step[], viewport: "desktop" as Viewport };
  let needShot: Finding[] = []; // exact signals raised between visits get the next visit's screenshot
  let shots = 0;

  const overCalls = () => opts.maxCalls > 0 && usage.calls >= opts.maxCalls;

  function flag(kind: Kind, signal: Signal, what: string, where: Finding["pages"][number], ev: Partial<Evidence> = {}) {
    const id = [signal, kind === "bug" ? pathOf(where.page) : "", fold(what)].filter(Boolean).join(" | ");
    const f = findings.get(id);
    if (f) {
      if (!f.pages.some((p) => p.page === where.page && JSON.stringify(p.item) === JSON.stringify(where.item))) f.pages.push(where);
      return;
    }
    const last = visits.at(-1);
    const same = last && last.goal === cur.goal && last.url === where.page;
    const evidence: Evidence = {
      goal: cur.goal,
      step: cur.steps.length,
      url: where.page,
      screenshot: same ? last.screenshot : undefined,
      steps: cur.steps.slice(-SHOW_STEPS).map(stepLine),
      testids: same ? last.testids : [],
      state: same ? last.state : {},
      ...ev,
    };
    const nf: Finding = { id, kind, signal, what, pages: [where], evidence };
    findings.set(id, nf);
    if (!evidence.screenshot) needShot.push(nf);
  }

  /** Listen for the exact signals; call once per page object. */
  function attach(page: Page) {
    const here = () => {
      try {
        const u = new URL(page.url());
        return u.pathname + u.search;
      } catch {
        return page.url();
      }
    };
    const short = (url: string) => {
      try {
        const u = new URL(url);
        return u.pathname + u.search;
      } catch {
        return url;
      }
    };
    page.on("response", (r) => {
      if (r.status() < 400) return;
      const req = r.request();
      const url = short(r.url());
      if (req.isNavigationRequest() && req.frame() === page.mainFrame()) flag("bug", "http", `HTTP ${r.status()} opening ${url}`, { page: url });
      else flag("bug", "request", `${req.method()} ${url} (${req.resourceType()}) returned HTTP ${r.status()}`, { page: here() });
    });
    page.on("requestfailed", (req) => {
      const err = req.failure()?.errorText ?? "failed";
      // Leaving a page cancels what it was still loading; that isn't a fault.
      if (err.includes("ERR_ABORTED") || err.includes("NS_BINDING_ABORTED")) return;
      flag("bug", "request", `${req.method()} ${short(req.url())} (${req.resourceType()}) failed: ${err}`, { page: here() });
    });
    page.on("console", (m) => {
      // A failed resource also logs "Failed to load resource"; the response already said which.
      if (m.type() !== "error" || m.text().startsWith("Failed to load resource")) return;
      flag("bug", "console", `console error: ${m.text().slice(0, 200)}`, { page: here() });
    });
    page.on("pageerror", (e) => flag("bug", "pageerror", `uncaught error: ${e.message.split("\n")[0].slice(0, 200)}`, { page: here() }));
  }

  /** Observe the page as it is now: a visit with a screenshot; new pages get the exact checks and Jev's judgements. */
  async function look(page: Page, why: string, goal: PersonaGoal, seen?: Seen): Promise<void> {
    seen ??= await see(page);
    const shown: string[] = await page.evaluate(() =>
      Array.from(new Set(Array.from(document.querySelectorAll("[data-testid]")).filter((el) => el.checkVisibility()).map((el) => el.getAttribute("data-testid")!))).sort(),
    );
    await settle(page);
    const screenshot = `shots/${String(++shots).padStart(3, "0")}.jpg`;
    await page.screenshot({ path: `${opts.dir}/${screenshot}`, type: "jpeg", quality: 60 }).catch(() => {});
    const visit: Visit = { n: visits.length, goal: cur.goal, step: cur.steps.length, url: seen.url, why, testids: shown, state: seen.state, screenshot };
    visits.push(visit);
    goals[cur.goal]?.visits.push(visit.n);
    for (const f of needShot) Object.assign(f.evidence, { screenshot, testids: f.evidence.testids.length ? f.evidence.testids : shown, state: Object.keys(f.evidence.state).length ? f.evidence.state : seen.state });
    needShot = [];
    if (route[route.length - 1] !== seen.url) route.push(seen.url);

    // What the header (and the home page) link to: the sections a route can miss.
    const links: [string, string][] = await page.evaluate(
      (home) =>
        Array.from(document.querySelectorAll(home ? "header a[href], main a[href]" : "header a[href]"))
          .map((a) => ({ u: new URL((a as HTMLAnchorElement).href), label: ((a as HTMLElement).innerText || a.textContent || a.getAttribute("title") || "").replace(/\s+/g, " ").trim() }))
          .filter(({ u }) => u.origin === location.origin)
          .map(({ u, label }) => [u.pathname, label.slice(0, 60)] as [string, string]),
      pathOf(seen.url) === "/",
    ).catch(() => []);
    for (const [p, label] of links) if (!linked.has(p) && !/^\/(logout|dev)\b/.test(p)) linked.set(p, label);

    const key = `${pathOf(seen.url)} ${shown.join(",")}`;
    if (judged.has(key)) return;
    judged.add(key);
    const at = seen.url;
    const ev = { testids: shown, state: seen.state, screenshot, url: at };

    // Exact checks of the page: overflow always at phone width; the profile's others.
    const checks = new Set<CheckId>(persona.checks ?? []);
    if (cur.viewport === "phone") checks.add("overflow");
    for (const id of checks) {
      for (const what of await CHECKS[id](page)) flag(id === "overflow" ? "bug" : "testability", id, what, { page: at }, ev);
    }

    if (!opts.judge || judged.size > MAX_PAGES || overCalls()) return;
    const path = pathOf(seen.url);

    // Concerns and frustrations that apply here, in one call.
    const applies = (s: Statement) =>
      (typeof s === "string" || ((!s.on || s.on.test(path)) && (!s.when || shown.includes(s.when)))) &&
      (goal.topic !== undefined || !text(s).includes("`topic`"));
    const statements: Record<string, { kind: "concern" | "frustration"; text: string }> = {};
    persona.concerns.filter(applies).forEach((s, i) => (statements[`c${i}`] = { kind: "concern", text: text(s) }));
    persona.frustrations.filter(applies).forEach((s, i) => (statements[`f${i}`] = { kind: "frustration", text: text(s) }));
    if (Object.keys(statements).length) {
      const needsActions = Object.values(statements).some((s) => s.text.includes("`actions`"));
      const state = {
        who: persona.who,
        ...(goal.topic !== undefined && { topic: goal.topic }),
        url: seen.url,
        title: seen.title,
        page_text: words(seen.text).slice(0, MAX_TEXT),
        ...(needsActions && { actions: seen.actions.slice(0, 60) }),
      };
      const ps = await judgeMany(state, Object.fromEntries(Object.entries(statements).map(([k, s]) => [k, s.text])));
      for (const [k, s] of Object.entries(statements)) {
        const p = round(ps[k]);
        judgements.push({ page: at, why, kind: s.kind, statement: s.text, p });
        if (s.kind === "concern" ? p < FLAG : p >= FLAG) {
          flag(s.kind === "concern" ? "unclear" : "frustrating", s.kind, s.text, { page: at, p }, { ...ev, jev: { statement: s.text, p } });
        }
      }
    }

    // Item by item: a sample of each table on the page, one question per row, one call per table.
    for (const probe of persona.items ?? []) {
      if (overCalls()) break;
      const rows: string[][] = await page.evaluate(
        ({ testid, max }) =>
          Array.from(document.querySelectorAll(`[data-testid="${testid}"]`))
            .filter((el) => el.checkVisibility())
            .slice(0, max)
            .map((row) => Array.from(row.querySelectorAll("td")).map((td) => (td as HTMLElement).innerText.trim())),
        { testid: probe.testid, max: probe.max ?? 6 },
      );
      if (!rows.length) continue;
      const items = rows.map((cells) => Object.fromEntries(probe.cells.map((c, i) => [c, cells[i] ?? ""]).filter(([, v]) => v)));
      const state = Object.fromEntries(items.map((it, i) => [`item_${i}`, it]));
      const ps = await judgeMany(state, Object.fromEntries(items.map((_, i) => [`i${i}`, probe.statement.replaceAll("`item`", `\`item_${i}\``)])));
      items.forEach((item, i) => {
        const p = round(ps[`i${i}`]);
        judgements.push({ page: at, why, kind: "item", statement: `${probe.statement} ${JSON.stringify(item)}`, p });
        if (p < FLAG) flag("content", "item", probe.statement, { page: at, p, item }, { ...ev, jev: { statement: `${probe.statement} ${JSON.stringify(item)}`, p } });
      });
    }
  }

  /** Bugs from the walk itself: dead actions, a stuck page, a persona who gave up. */
  function walkFindings(outcome: Outcome) {
    outcome.steps.forEach((s, i) => {
      // A link to the page you're on, or the option that is already picked, changes nothing, and that's fine.
      if (!s.note?.includes("no effect") || !s.action || s.note.includes("a link to this page") || s.action.includes(" [selected")) return;
      cur.steps = outcome.steps.slice(0, i + 1);
      flag("bug", "no-effect", `"${s.action}" had no effect`, { page: s.url });
    });
    cur.steps = outcome.steps;
    const last = outcome.steps.at(-1)?.url ?? "";
    if (outcome.reason.startsWith("stuck")) flag("bug", "stuck", outcome.reason, { page: last });
    if (outcome.reason.startsWith("lost")) flag("bug", "lost", outcome.reason, { page: last });
  }

  function begin(goal: PersonaGoal, viewport: Viewport, extra: Partial<GoalRun> = {}): GoalRun {
    const g: GoalRun = {
      goal: goal.goal,
      done: goal.done,
      start: goal.start ?? "/",
      viewport,
      ...(goal.topic !== undefined && { topic: goal.topic }),
      ...(goal.check && { check: goal.check.toString() }),
      reached: false,
      reason: "",
      steps: [],
      visits: [],
      ...extra,
    };
    goals.push(g);
    cur = { goal: goals.length - 1, steps: [], viewport };
    return g;
  }

  /** Walk one goal from wherever `page` is, observing as it goes. `maxSteps` is what's left of the budget. */
  async function run(page: Page, goal: PersonaGoal, viewport: Viewport, maxSteps: number, extra: Partial<GoalRun> = {}): Promise<GoalRun> {
    const g = begin(goal, viewport, extra);
    const journey: Journey = { id: persona.id, start: g.start, goal: goal.goal, done: goal.done, check: goal.check, inputs: goal.inputs, maxSteps };
    const outcome = await navigate(page, journey, {
      persona,
      onPage: (seen, steps) => {
        cur.steps = steps;
        return look(page, "visited", goal, seen);
      },
      stop: () => (overCalls() ? `budget: ${opts.maxCalls} Jev calls spent` : undefined),
    });
    cur.steps = outcome.steps;
    await look(page, outcome.reached ? "goal reached" : "gave up here", goal);
    if (outcome.reached && goal.then) await goal.then(page, (why) => look(page, why, goal));
    walkFindings(outcome);
    Object.assign(g, { reached: outcome.reached, reason: outcome.reason, steps: outcome.steps });
    return g;
  }

  /**
   * Redo a recorded goal with no Jev: the same start and actions, observed
   * the same way (exact signals only). `goal` is the profile's goal with that
   * text, when there is one, for its `check` and `then`.
   */
  async function replay(page: Page, rec: GoalRun, goal: PersonaGoal | undefined): Promise<GoalRun> {
    const pg: PersonaGoal = goal ?? { goal: rec.goal, done: rec.done, start: rec.start, topic: rec.topic };
    const g = begin(pg, rec.viewport, { start: rec.start, wander: rec.wander });
    const steps: GoalRun["steps"] = [];
    const inputs = { ...persona.inputs, ...goal?.inputs };
    for (const s of rec.steps) {
      cur.steps = steps;
      await look(page, "visited", pg);
      const step: GoalRun["steps"][number] = { ...s, url: new URL(page.url()).pathname + new URL(page.url()).search };
      steps.push(step);
      if (!s.action) continue;
      step.replay = await redo(page, s.action, { avoid: persona.avoid, inputs });
      if (step.replay !== "done") step.note = `replay: action ${step.replay}`;
    }
    cur.steps = steps;
    const reached = goal?.check ? await goal.check(page) : rec.reached && steps.every((s) => s.replay !== "missing");
    await look(page, reached ? "goal reached" : "route ended", pg);
    if (reached && goal?.then) await goal.then(page, (why) => look(page, why, pg));
    Object.assign(g, { reached, reason: goal?.check ? `the profile's exact check ${reached ? "passed" : "failed"}` : "replayed the recorded route", steps });
    return g;
  }

  /**
   * --wander: pick a section the persona hasn't visited (a Choice weighted by
   * `prefers`) and try it out. Undefined when nothing is left to pick. (Wrapped:
   * a PersonaGoal has a `then`, so an async function can't return it bare.)
   */
  async function pickSection(page: Page): Promise<{ goal: PersonaGoal } | undefined> {
    await page.goto("/");
    await look(page, "wander: choosing a section", { goal: "", done: "" });
    const visited = new Set(route.map(pathOf));
    const open = [...linked].filter(([p]) => !visited.has(p) && p !== "/").slice(0, 60);
    if (!open.length || overCalls()) return undefined;
    const options = Object.fromEntries(open.map(([p, label], i) => [`s${i}`, `${label || p} (${p})`]));
    const res = await choose(
      { who: persona.who, visited: [...visited] },
      `You are the person in \`who\`. Which of these sections of the app would you look at next? ${persona.prefers}`,
      options,
    );
    const [path, label] = open[Number(res.choice.slice(1))] ?? open[0];
    const goal: PersonaGoal = {
      goal: `Try out the "${label || path}" section: use what it offers the way you would.`,
      done: `The learner has used something in the "${label || path}" section.`,
      start: path,
    };
    return { goal };
  }

  function result() {
    return { visits, route, linked: Object.fromEntries(linked), goals, findings: [...findings.values()], judgements, pages_judged: judged.size };
  }

  return { attach, look, run, replay, pickSection, result };
}

/** Exact page checks, run in the page; each returns a message per problem. */
const CHECKS: Record<CheckId, (page: Page) => Promise<string[]>> = {
  overflow: async (page) => {
    // Slide-ins and confetti overshoot while they run: measure once they have settled.
    await settle(page);
    return page.evaluate(() => {
      const w = document.documentElement.clientWidth;
      const extra = document.documentElement.scrollWidth - w;
      if (extra <= 1) return [];
      const name = (el: Element) => {
        const id = el.closest("[data-testid]")?.getAttribute("data-testid");
        return `<${el.tagName.toLowerCase()}${el.className ? ` class="${el.className}"` : ""}>${id ? ` in [${id}]` : ""}`;
      };
      // The deepest elements past the edge: the ones whose children all fit.
      const over = (el: Element) => el.getBoundingClientRect().right > w + 1 && el.getBoundingClientRect().width > 0;
      const wide = [
        ...new Set(
          Array.from(document.querySelectorAll("body *"))
            .filter((el) => el.checkVisibility() && over(el) && !Array.from(el.children).some(over))
            .map(name),
        ),
      ].slice(0, 3);
      return [`the page is ${extra}px wider than a ${w}px screen${wide.length ? `: ${wide.join(", ")} run off the edge` : ""}`];
    });
  },
  labels: (page) =>
    page.evaluate(() => {
      // Grouped by testid: a row of accent buttons is one finding, not nine.
      const groups = new Map<string, string[]>();
      for (const el of Array.from(document.querySelectorAll("a[href], button, [role=button], summary"))) {
        if (!el.checkVisibility()) continue;
        // An SVG element (an algebra tile, role=button) has no innerText.
        const shown = ((el as HTMLElement).innerText ?? el.textContent ?? "").replace(/\s+/g, " ").trim();
        const named = el.getAttribute("aria-label") || el.getAttribute("title") || el.getAttribute("aria-labelledby");
        if (named || /[\p{L}\p{N}]{2}/u.test(shown)) continue;
        const id = el.closest("[data-testid]")?.getAttribute("data-testid");
        const where = `${el.tagName.toLowerCase()}${id ? ` in [${id}]` : ""}`;
        groups.set(where, [...(groups.get(where) ?? []), `"${shown || "(empty)"}"`]);
      }
      return [...groups].map(([where, labels]) => {
        const shown = [...new Set(labels)];
        return `${labels.length === 1 ? "a" : labels.length} ${where} labelled ${shown.slice(0, 6).join(", ")}${shown.length > 6 ? ", …" : ""}: no aria-label or title says what ${labels.length === 1 ? "it does" : "they do"}`;
      });
    }),
  "class-state": (page) =>
    page.evaluate(() => {
      const STATE = /^(sel|selected|done|right|wrong|correct|active|passed|failed|error|matched|used|current|locked)$/;
      const out = new Set<string>();
      for (const el of Array.from(document.querySelectorAll("[class]"))) {
        const states = Array.from(el.classList).filter((c) => STATE.test(c));
        if (!states.length || !el.checkVisibility()) continue;
        const said = Array.from(el.attributes).some(
          (a) => (a.name.startsWith("data-") && !["data-testid", "data-e2e-ref"].includes(a.name)) || /^aria-(pressed|selected|checked|current|expanded|invalid|disabled)$/.test(a.name),
        );
        if (said) continue;
        const id = el.closest("[data-testid]")?.getAttribute("data-testid");
        out.add(`<${el.tagName.toLowerCase()} class="${el.className}">${id ? ` in [${id}]` : ""} shows "${states.join(", ")}" only in its class`);
      }
      return [...out];
    }),
  "hidden-menus": (page) =>
    page.evaluate(() => {
      // A menu link is only hidden if no visible link on the page goes to the same place.
      const shown = new Set(
        Array.from(document.querySelectorAll<HTMLAnchorElement>("a[href]"))
          .filter((a) => a.checkVisibility())
          .map((a) => a.href),
      );
      return Array.from(document.querySelectorAll("details:not([open])"))
        .filter((d) => d.checkVisibility())
        .map((d) => {
          const links = Array.from(d.querySelectorAll<HTMLAnchorElement>("a[href]"))
            .filter((a) => !shown.has(a.href))
            .map((a) => a.textContent!.replace(/\s+/g, " ").trim());
          const sum = d.querySelector("summary");
          const name = sum?.getAttribute("aria-label") || sum?.getAttribute("title") || sum?.textContent?.replace(/\s+/g, " ").trim() || "(unlabelled)";
          const id = d.closest("[data-testid]")?.getAttribute("data-testid") ?? sum?.closest("[data-testid]")?.getAttribute("data-testid");
          return links.length ? `${links.length} links only show after opening the menu "${name}"${id ? ` [${id}]` : ""}: ${links.slice(0, 6).join(", ")}` : "";
        })
        .filter(Boolean);
    }),
  "error-role": (page) =>
    page.evaluate(() => {
      const out: string[] = [];
      for (const el of Array.from(document.querySelectorAll("[class*=error], [data-testid*=error]"))) {
        const t = (el as HTMLElement).innerText?.trim();
        if (!t || !el.checkVisibility() || el.closest("[role=alert]")) continue;
        const id = el.getAttribute("data-testid");
        out.push(`error "${t.slice(0, 60)}"${id ? ` [${id}]` : ""} is not role=alert`);
      }
      return out;
    }),
};
