// Exact helpers for the app: no model in the loop. They hold on to the
// data-testid hooks and state attributes described in docs/testing.md.
import { expect, type Locator, type Page } from "@playwright/test";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { skipWizard } from "./learner";

const REPO = fileURLToPath(new URL("../..", import.meta.url));

/** Log in as the dev user. `query` is passed to /dev/login (default: open Spanish). */
export async function devLogin(page: Page, query = "open=spanish"): Promise<void> {
  await page.goto(`/dev/login?${query}`);
  await expect(page.getByTestId("nav-account-toggle")).toHaveAttribute("data-email", "dev@example.com");
}

/**
 * Log in (`query` goes to /dev/login), "Create a course" on /courses and
 * generate it from the editor's dialog with `wish`. Lands on the job's
 * status page.
 */
export async function generateCourse(page: Page, wish: string, query = "keys=fake"): Promise<void> {
  await devLogin(page, `${query}&next=/courses`);
  await createCourse(page);
  await page.getByTestId("generate-wish").fill(wish);
  await page.getByTestId("generate-submit").click();
}

/**
 * After Generate: the editor follows the job in its progress modal, which
 * closes by itself when the course is ready. A first generation that stops
 * has no editor: the page ends on the course list with the Generate dialog
 * open (Retry or Resume) and the error in it. Waits for either end; returns which.
 */
export async function generationEnds(page: Page, timeout = 60_000): Promise<"done" | "failed"> {
  const stoppedUrl = /\/courses\?(.*&)?generate=/;
  await page.waitForURL((u) => /\/courses\/[^/?]+\?(.*&)?job=/.test(u.href) || stoppedUrl.test(u.href));
  const modal = page.getByTestId("course-ai-modal");
  let end: "done" | "failed" = "done";
  await expect(async () => {
    if (stoppedUrl.test(page.url())) {
      await expect(page.getByTestId("generate")).toBeVisible({ timeout: 1000 });
      end = "failed";
      return;
    }
    await expect(page.getByTestId("course-edit")).toBeVisible({ timeout: 1000 });
    expect(await modal.count()).toBe(0);
    // The row too, not just the modal: a page that says "being generated" with no modal never refreshes (#183).
    await expect(page.getByTestId("course-edit")).toHaveAttribute("data-course-status", /^(ready|failed)$/, { timeout: 1000 });
    end = "done";
  }).toPass({ timeout });
  return end;
}

/** Use AI's mode radio (#1241): "add" (Add), "edit" (Edit) or "change" (Add and edit). */
export function aiMode(page: Page, mode: "add" | "edit" | "change"): Locator {
  return page.locator(`[data-testid=course-ai-mode][value=${mode}]`);
}

/** "Create a course" on /courses: a fresh draft, its editor with the Generate dialog open. Returns the course id. */
export async function createCourse(page: Page): Promise<string> {
  if (!/\/courses$/.test(new URL(page.url()).pathname)) await page.goto("/courses");
  await page.getByTestId("courses-create").click();
  await expect(page.getByTestId("generate")).toBeVisible();
  return (await page.getByTestId("course-edit").getAttribute("data-course-id")) ?? "";
}

/**
 * The course editor saves as it is edited: a lesson's JSON or form 3 s
 * after the last keystroke, a move, rename or delete after 1 s. This makes
 * both waits `ms` on every page `page` opens from now on (the
 * window.AUTOSAVE_TEST_MS hook in static/courses.js).
 */
export async function fastAutosave(page: Page, ms = 150): Promise<void> {
  await page.addInitScript((wait) => {
    (window as unknown as { AUTOSAVE_TEST_MS: number }).AUTOSAVE_TEST_MS = wait;
  }, ms);
}

/**
 * The editor's open icon (`course-edit-open`, #229) opens the course in a new
 * tab and makes it the active one. Clicks it and returns that tab once it has
 * loaded, past the course's first-visit settings wizard (#574); the editor
 * stays open in `page`.
 */
export async function openFromEditor(page: Page): Promise<Page> {
  const [tab] = await Promise.all([page.context().waitForEvent("page"), page.getByTestId("course-edit-open").click()]);
  await tab.waitForURL((u) => u.protocol.startsWith("http") && !u.pathname.includes("/courses/open/"));
  await tab.waitForLoadState();
  await skipWizard(tab);
  return tab;
}

/**
 * Copy a course (#629) and land in the copy's editor: the copy form posted
 * from the page, as the copy icon in the course's settings dialog does.
 * Returns the new course's id.
 */
export async function copyCourse(page: Page, courseId: string): Promise<string> {
  if (!/\/courses$/.test(new URL(page.url()).pathname)) await page.goto("/courses");
  await page.evaluate((id) => {
    const f = document.createElement("form");
    f.method = "post";
    f.action = `/courses/${id}/copy`;
    document.body.appendChild(f);
    f.submit();
  }, courseId);
  await page.waitForURL(/\/courses\/[^/?]+$/);
  await expect(page.getByTestId("course-edit")).toBeVisible();
  return (await page.getByTestId("course-edit").getAttribute("data-course-id")) ?? "";
}

/** Wait until the editor's circle says everything is saved (green). */
export async function editorSaved(page: Page, timeout = 15_000): Promise<void> {
  await expect(page.getByTestId("course-edit-save-status")).toHaveAttribute("data-state", "saved", { timeout });
}

/** Wipe the dev user's progress, projects, own courses and saved app language (keeps the account and keys). */
export async function devReset(page: Page): Promise<void> {
  await page.goto("/dev/reset");
}

/**
 * The test course: every feature a course file can switch on, one lesson per
 * feature (map in data/test-courses/test-course/AUTHORING.md). scripts/e2e.sh
 * serves it (TEST_COURSE=1); open it with `devLogin(page, "open=" + TEST_COURSE)`.
 */
export const TEST_COURSE = "test-course";

/** Where a built-in course's files are: data/courses/<id>, or data/test-courses/<id> for a test course. */
export function courseDir(courseId: string): string {
  const real = join(REPO, "data", "courses", courseId);
  try {
    statSync(real);
    return real;
  } catch {
    return join(REPO, "data", "test-courses", courseId);
  }
}

/** question_id -> correct answer, from a built-in (or test) course's lesson files. */
export function correctAnswers(courseId = "spanish"): Map<string, string> {
  const answers = new Map<string, string>();
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) walk(path);
      else if (name.endsWith(".json")) {
        const lesson = JSON.parse(readFileSync(path, "utf8"));
        for (const q of lesson.quiz ?? []) answers.set(q.question_id, q.correct_answer);
      }
    }
  };
  walk(join(courseDir(courseId), "lessons"));
  return answers;
}

/**
 * Play the Sprint mode session open in `page` to the end, answering whatever is
 * asked with whatever is on offer: it types "x", or taps the options in
 * turn until one is taken (sorting into buckets only moves on when the
 * bucket is right). It checks that a session can be finished, not that the
 * answers are right. `afterCheck` runs after each Check, with the result shown.
 */
export async function playSprintSession(page: Page, maxSteps = 300, afterCheck?: () => Promise<void>): Promise<number> {
  const stage = page.getByTestId("sprint-play-stage");
  const check = page.getByTestId("sprint-play-check");
  const next = page.getByTestId("sprint-play-next");
  const done = page.getByTestId("sprint-play-done");
  await expect(stage).not.toBeEmpty();
  let seen = "", taps = 0;
  const unmatched: Record<string, number> = {}; // match-board terms no meaning took, by label
  const log: string[] = []; // the last few actions, for the error
  const did = (what: string) => log.push(what) > 12 && log.shift();
  for (let step = 0; step < maxSteps; step++) {
    if (await done.isVisible()) return step;
    if (await next.isVisible()) {
      did("continue");
      await next.click({ timeout: 1_000 }).catch(() => did("missed"));
      continue;
    }
    if (await check.isEnabled()) {
      did("check");
      await check.click({ timeout: 1_000 }).catch(() => did("missed"));
      await afterCheck?.();
      continue;
    }
    // Answer something: type into a visible box, or tap the first free tile/option.
    const input = stage.locator("input:visible, textarea:visible").first();
    if ((await input.count()) && (await input.isEditable())) {
      did("type x");
      await input.fill("x");
      continue;
    }
    // A match board (#284): pair the first open term with each open meaning
    // until one takes. Blind tapping can cycle for ever on some tile orders.
    // The board matches by pair, not by text, so near-synonym meanings ("nice
    // to meet you" / "pleased to meet you") are no trap: one of them is the
    // term's own. A term no open meaning takes twice fails then, not in 300 steps.
    const board = stage.locator(".sprint-match");
    if (await board.count()) {
      const term = await board.locator(".col").first().locator(".sprint-tile:not(.done)").first().elementHandle({ timeout: 500 }).catch(() => null);
      if (term) {
        const label = (await term.innerText().catch(() => "?")).trim().slice(0, 30);
        did(`match "${label}"`);
        // Gone (the stage moved on) counts as done.
        const has = (cls: string) => term.evaluate((t, c) => !t.isConnected || t.classList.contains(c), cls).catch(() => true);
        const meanings = await board.locator(".col").nth(1).locator(".sprint-tile:not(.done)").elementHandles();
        let matched = false;
        for (const meaning of meanings) {
          // The term must be the selected tile before its meaning is tapped: a
          // miss clears both picks, and a click can miss while a tile shakes.
          if (!(await has("sel"))) await term.click({ timeout: 1_000 }).catch(() => did("missed"));
          if ((matched = await has("done"))) break; // it paired with a meaning picked earlier
          await meaning.click({ timeout: 1_000 }).catch(() => did("missed"));
          if ((matched = await has("done"))) break;
        }
        if (!matched && (unmatched[label] = (unmatched[label] ?? 0) + 1) >= 2) {
          throw new Error(`Match board: no open meaning took "${label}" twice (${meanings.length} tried).\nStage: ${(await stage.innerText()).slice(0, 300)}`);
        }
        continue;
      }
    }
    // Place the note (#1226): a tap on the staff places one, then Check. The
    // flat and sharp buttons alone would never make an answer.
    const staffNote = stage.locator('[data-testid="staff-note"][data-answer=""]');
    if (await staffNote.count()) {
      did("staff");
      await staffNote.getByTestId("staff-note-staff").click({ timeout: 1_000 }).catch(() => did("missed"));
      continue;
    }
    // A ladder (#640): fill the rungs from the bank, then Check. A blind tap
    // could take a form back off a filled rung and go round for ever.
    const ladderTile = stage.locator('[data-testid="sprint-ladder-tile"]:not(:disabled)').first();
    if (await ladderTile.count()) {
      did(`ladder "${(await ladderTile.innerText({ timeout: 500 }).catch(() => "?")).trim().slice(0, 30)}"`);
      await ladderTile.click({ timeout: 1_000 }).catch(() => did("missed"));
      continue;
    }
    // Spell the chord (#1228): fill the slots from the tiles, then Check. A blind
    // tap could take a note back out of a filled slot and go round for ever.
    const chordTile = stage.locator('[data-testid="chord-spell-tile"]:not(:disabled)').first();
    if (await chordTile.count()) {
      did(`chord "${(await chordTile.innerText({ timeout: 500 }).catch(() => "?")).trim()}"`);
      await chordTile.click({ timeout: 1_000 }).catch(() => did("missed"));
      continue;
    }
    const text = await stage.innerText();
    taps = text === seen ? taps + 1 : 0;
    seen = text;
    const options = stage.locator("button:visible:not([disabled]):not(.sel):not(.used):not(.done)");
    const n = await options.count();
    if (n) {
      const option = options.nth(taps % n);
      // Only for the log: the stage may already have re-rendered and taken the tile away.
      did(`tap "${(await option.innerText({ timeout: 500 }).catch(() => "?")).trim().slice(0, 30)}"`);
      // The stage may change under us (graded, disabled); then just look again.
      await option.click({ timeout: 1_000 }).catch(() => did("missed"));
      continue;
    }
    did("wait");
    await page.waitForTimeout(200); // an animation or a request is in flight
  }
  throw new Error(
    `Sprint mode session did not finish in ${maxSteps} steps.\nLast actions: ${log.join(", ")}\nStage: ${(await stage.innerText()).slice(0, 300)}`,
  );
}

/** After the Share form was sent: the link the dialog shows (shown this
 * once, stored only hashed; nothing is mailed, the owner passes it on). */
export async function shareLink(page: Page): Promise<string> {
  const input = page.locator('[data-testid="course-share-link-row"] [data-testid="course-share-link"]').first();
  await expect(input).toHaveValue(/\/courses\/shared\/[0-9a-f]{64}$/);
  return input.inputValue();
}

/** With the course's Share dialog open: make an invitation link named
 * `label` (or unnamed); the link it shows. */
export async function makeShareLink(page: Page, label = ""): Promise<string> {
  const dialog = page.getByTestId("course-share");
  await expect(dialog).toBeVisible();
  await dialog.getByTestId("course-share-label").fill(label);
  await dialog.getByTestId("course-share-send").click();
  await expect(page.getByTestId("course-share-notice")).toContainText("Invitation link made");
  return shareLink(page);
}

/** The Share dialog's row for the link named `label`. */
export function shareRow(page: Page, label: string) {
  return page.locator(`[data-testid="course-share-row"][data-label="${label}"]`);
}
