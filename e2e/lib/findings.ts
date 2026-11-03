// Exact signals that the platform is broken, no model in the loop. Each
// finding says where it happened (mode, lesson, phase, URL) and keeps a
// screenshot of the first time.
import type { Page } from "@playwright/test";
import { join } from "node:path";

export type Kind =
  | "bug" // HTTP errors, failed requests, console and page errors, overflow
  | "invariant" // progress that disagrees between pages, a fudge the app refused
  | "stuck"; // a session that could not finish (a dead button)

export interface Finding {
  kind: Kind;
  signal: string;
  what: string;
  mode: string;
  lesson: string;
  phase: string;
  url: string;
  count: number;
  screenshot?: string;
}

export class Findings {
  list: Finding[] = [];
  where = { lesson: "", phase: "" };
  readonly mode: string;
  readonly dir: string;
  constructor(mode: string, dir: string) {
    this.mode = mode;
    this.dir = dir;
  }

  async flag(page: Page | null, kind: Kind, signal: string, what: string, url = page?.url() ?? "") {
    const path = url.replace(/^https?:\/\/[^/]+/, "");
    const old = this.list.find((f) => f.kind === kind && f.signal === signal && f.what === what);
    if (old) return void old.count++;
    const f: Finding = { kind, signal, what, mode: this.mode, ...this.where, url: path, count: 1 };
    this.list.push(f);
    console.log(`   !! ${kind}: ${what} (${path})`);
    if (page) {
      f.screenshot = `shots/${String(this.list.length).padStart(3, "0")}-${signal}.png`;
      await page.screenshot({ path: join(this.dir, f.screenshot), fullPage: true }).catch(() => (f.screenshot = undefined));
    }
  }

  /** Listen for exact bug signals on `page` for the rest of the run. */
  watch(page: Page) {
    const base = new URL(page.url() === "about:blank" ? "http://x" : page.url()).origin;
    page.on("response", (r) => {
      if (r.status() < 400) return;
      const req = r.request();
      const url = r.url().replace(base, "");
      if (req.isNavigationRequest() && req.frame() === page.mainFrame()) void this.flag(page, "bug", "http", `HTTP ${r.status()} opening ${url}`);
      else void this.flag(page, "bug", "request", `${req.method()} ${url} returned HTTP ${r.status()}`);
    });
    page.on("requestfailed", (req) => {
      const err = req.failure()?.errorText ?? "";
      if (/ERR_ABORTED|NS_BINDING_ABORTED/.test(err)) return; // a navigation that moved on
      void this.flag(page, "bug", "request", `${req.method()} ${req.url().replace(base, "")} failed: ${err}`);
    });
    page.on("console", (m) => {
      if (m.type() !== "error" || /Failed to load resource/.test(m.text())) return; // the response already said which
      void this.flag(page, "bug", "console", `console error: ${m.text().slice(0, 200)}`);
    });
    page.on("pageerror", (e) => void this.flag(page, "bug", "pageerror", `uncaught error: ${e.message.split("\n")[0].slice(0, 200)}`));
  }

  /** Page checks for a page seen for the first time: wider than the screen. */
  async checkPage(page: Page) {
    const width = () => page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth).catch(() => 0);
    // Only what is still too wide once the page has settled: a card sliding in overflows for a moment.
    if ((await width()) <= 2) return;
    await page.waitForTimeout(800);
    const over = await width();
    if (over > 2) await this.flag(page, "bug", "overflow", `${new URL(page.url()).pathname} is ${over}px wider than the screen`);
  }
}
