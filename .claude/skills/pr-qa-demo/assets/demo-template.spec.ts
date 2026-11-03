// Demo recorder: a Playwright spec that walks through a PR's change with
// captions and a visible cursor, recording a video. Copy it to
// e2e/specs/_demo-pr<N>.spec.ts (untracked; never commit it), fill in the
// `walkthrough` function, and run from the repo root with a server up:
//
//   DEMO_OUT=/abs/path/demo-pr<N>.webm E2E_BASE_URL=<url> \
//     npx --prefix e2e playwright test --config e2e/playwright.config.ts specs/_demo-pr<N>.spec.ts
//
// Then make the .mp4 with music in the PR's demo folder (references/manifest.md):
//   .claude/skills/pr-qa-demo/scripts/finish-demo.sh /abs/path/demo-pr<N>.webm --out <staging dir>/demo.mp4
import { expect, test, type Locator, type Page } from "@playwright/test";
import { devLogin } from "../lib/app.ts";

const OUT = process.env.DEMO_OUT;
if (!OUT) throw new Error("DEMO_OUT=<absolute .webm path> is required");
const TITLE = process.env.DEMO_TITLE ?? "Demo";
const SUBTITLE = process.env.DEMO_SUBTITLE ?? "";
const W = 1280;
const H = 720;

test.use({
  viewport: { width: W, height: H },
  video: { mode: "on", size: { width: W, height: H } },
  launchOptions: { slowMo: 120 },
});
test.setTimeout(10 * 60_000);

// Runs in every document before its scripts: a caption bar and a cursor dot
// that survive navigations (the caption text lives in sessionStorage).
const OVERLAY = () => {
  const render = () => {
    if (document.getElementById("__demo_cap")) return;
    const css = document.createElement("style");
    css.textContent = `
      #__demo_cap{position:fixed;left:0;right:0;bottom:0;z-index:2147483646;padding:14px 28px;
        font:600 22px/1.3 -apple-system,Segoe UI,Helvetica,Arial,sans-serif;color:#fff;
        background:rgba(20,20,24,.86);text-align:center;letter-spacing:.2px;pointer-events:none;
        transition:opacity .25s}
      #__demo_cap:empty{opacity:0}
      #__demo_cur{position:fixed;z-index:2147483647;width:22px;height:22px;margin:-11px 0 0 -11px;border-radius:50%;
        background:rgba(230,60,60,.55);border:2px solid #fff;box-shadow:0 0 0 2px rgba(230,60,60,.6);
        pointer-events:none;transition:left .4s ease,top .4s ease,transform .15s;left:50%;top:50%}
      #__demo_cur.__down{transform:scale(.6);background:rgba(230,60,60,.95)}
      .__demo_spot{outline:3px solid #e63c3c !important;outline-offset:3px;border-radius:4px;
        box-shadow:0 0 0 6px rgba(230,60,60,.25) !important;transition:box-shadow .2s}`;
    document.documentElement.appendChild(css);
    const cap = document.createElement("div");
    cap.id = "__demo_cap";
    cap.textContent = sessionStorage.getItem("__demo_caption") ?? "";
    const cur = document.createElement("div");
    cur.id = "__demo_cur";
    const pos = sessionStorage.getItem("__demo_cursor");
    if (pos) {
      const [x, y] = pos.split(",");
      cur.style.left = `${x}px`;
      cur.style.top = `${y}px`;
    }
    document.documentElement.append(cap, cur);
  };
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", render);
  else render();
};

/** Show a caption at the bottom of the frame and hold for `ms`. */
export async function say(page: Page, text: string, ms = 2000): Promise<void> {
  await page.evaluate((t) => {
    sessionStorage.setItem("__demo_caption", t);
    const el = document.getElementById("__demo_cap");
    if (!el) return;
    el.textContent = t;
    // An open modal <dialog> sits in the top layer and would hide the bar: keep the bar inside the topmost one.
    const dialogs = Array.from(document.querySelectorAll("dialog[open]"));
    const host = dialogs.length ? dialogs[dialogs.length - 1] : document.documentElement;
    if (el.parentElement !== host) host.appendChild(el);
  }, text);
  await page.waitForTimeout(ms);
}

/** Move the demo cursor to the element and outline it. */
export async function spot(loc: Locator, ms = 700): Promise<void> {
  await loc.first().evaluate((el) => el.scrollIntoView({ block: "center", inline: "nearest" })).catch(() => {});
  await loc.page().waitForTimeout(150);
  const box = await loc.first().boundingBox();
  const page = loc.page();
  if (box) {
    const x = box.x + box.width / 2;
    const y = box.y + box.height / 2;
    await page.evaluate(([x, y]) => {
      sessionStorage.setItem("__demo_cursor", `${x},${y}`);
      const c = document.getElementById("__demo_cur");
      if (c) {
        c.style.left = `${x}px`;
        c.style.top = `${y}px`;
      }
    }, [x, y]);
    await page.mouse.move(x, y, { steps: 8 });
  }
  await loc.first().evaluate((el) => el.classList.add("__demo_spot")).catch(() => {});
  await page.waitForTimeout(ms);
  await loc.first().evaluate((el) => el.classList.remove("__demo_spot")).catch(() => {});
}

/** Spot, press the cursor, click. */
export async function click(loc: Locator, ms = 700): Promise<void> {
  await spot(loc, ms);
  const page = loc.page();
  await page.evaluate(() => document.getElementById("__demo_cur")?.classList.add("__down"));
  await loc.first().click();
  await page.waitForTimeout(150);
  await page.evaluate(() => document.getElementById("__demo_cur")?.classList.remove("__down")).catch(() => {});
}

/** Spot a box and type into it slowly. */
export async function type(loc: Locator, text: string): Promise<void> {
  await spot(loc, 400);
  await loc.first().click();
  await loc.first().pressSequentially(text, { delay: 60 });
}

/** A full-frame title card. */
async function titleCard(page: Page, title: string, subtitle: string, ms = 2600): Promise<void> {
  await page.setContent(`<!doctype html><html><body style="margin:0;height:100vh;display:grid;place-items:center;background:#15161a;color:#fff;font-family:-apple-system,Segoe UI,Helvetica,Arial,sans-serif">
    <div style="text-align:center;max-width:1000px;padding:0 40px">
      <div style="font-size:44px;font-weight:700;line-height:1.2">${title.replace(/</g, "&lt;")}</div>
      <div style="font-size:24px;opacity:.75;margin-top:18px;line-height:1.4">${subtitle.replace(/</g, "&lt;")}</div>
    </div></body></html>`);
  await page.waitForTimeout(ms);
}

/** A plain text page for real command output (a log line, a curl answer), with the overlay re-created. */
export async function textPage(page: Page, heading: string, body: string, ms = 2500): Promise<void> {
  const esc = (t: string) => t.replace(/&/g, "&amp;").replace(/</g, "&lt;");
  await page.setContent(`<!doctype html><html><body style="margin:0;min-height:100vh;background:#15161a;color:#eee;font-family:-apple-system,Segoe UI,Helvetica,Arial,sans-serif;padding:48px 60px;box-sizing:border-box">
    <div style="font-size:26px;font-weight:700;margin-bottom:20px">${esc(heading)}</div>
    <pre style="font:17px/1.5 ui-monospace,Menlo,monospace;white-space:pre-wrap;background:#0d0e11;padding:20px 24px;border-radius:8px;border:1px solid #333">${esc(body)}</pre></body></html>`);
  await page.evaluate(OVERLAY);
  await page.waitForTimeout(ms);
}

// ---------------------------------------------------------------------------
// The walkthrough. Replace the body with the PR's own story: one `say` per
// step of the PR's "How to QA it" list that a viewer can see, `click` for
// each action, an `expect` where the result matters (a failed expect ends
// the recording, so the video never shows a broken claim as working).
async function walkthrough(page: Page): Promise<void> {
  await devLogin(page, "open=spanish");
  await say(page, "Signed in as the dev learner, Spanish course open.");
  await click(page.getByRole("link", { name: "Open the learning map" }));
  await say(page, "The learning map.");
}
// ---------------------------------------------------------------------------

test("demo", async ({ page }) => {
  await page.addInitScript(OVERLAY);
  await page.goto("/healthz"); // give the tab the site's origin first: sessionStorage on about:blank throws
  await titleCard(page, TITLE, SUBTITLE);
  await walkthrough(page);
  await say(page, "That is the change. Thanks for watching.", 2200);
  const video = page.video();
  await page.context().close();
  if (!video) throw new Error("no video recorded");
  await video.saveAs(OUT);
  console.log(`demo saved: ${OUT}`);
});
