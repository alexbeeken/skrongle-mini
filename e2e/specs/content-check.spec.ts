// The AI content check, stage 1 (#1036): Jev scores every part of a made
// course before it is shared. The dev server's Jev stand-in answers from
// data/test-fixtures/jev/content-check.json: "hurting yourself is the
// answer" scores high on self-harm, everything else low. A high score goes
// to the AI review (stage 2, #1037), whose default fixture answer is "fine":
// the course stays clear and its dialog shows the scores, the lesson that
// scored high and the review's word on it. Stage 2's flags are in
// content-review.spec.ts. Caching, failures and the job are checked in
// content_check::tests.
import { expect, test, type Page } from "@playwright/test";
import { createCourse, devLogin } from "../lib/app";
import { login, newAccount, signUp } from "../lib/learner";

test.skip(process.env.E2E_MINI === "1", "the content check is hosted only; Skrongle mini leaves it out");

/** Put `summary` in the course's first lesson, titled `title`. */
async function edit(page: Page, id: string, title: string, summary: string) {
  const listing = (await (await page.request.get(`/api/courses/${id}/files`)).json()) as { files: { path: string }[] };
  const path = listing.files.map((f) => f.path).find((p) => p.startsWith("lessons/"))!;
  const lesson = JSON.parse(((await (await page.request.get(`/api/courses/${id}/file?path=${encodeURIComponent(path)}`)).json()) as { content: string }).content);
  lesson.title = title;
  lesson.summary = summary;
  const r = await page.request.post(`/api/courses/${id}/files`, { data: { files: [{ path, content: JSON.stringify(lesson, null, 2) }] } });
  expect(r.ok()).toBe(true);
  return path;
}

/** Share the course; the scan runs first and the share goes through. */
async function share(page: Page, id: string) {
  const r = await page.request.post(`/courses/${id}/share`, { form: { emails: `friend-${id}@example.org` }, maxRedirects: 0 });
  expect(r.status()).toBe(303);
  expect(r.headers()["location"]).not.toContain("flagged");
}

test("a course is scanned before it is shared; a high score the AI review passes blocks nothing", async ({ page, browser }) => {
  const them = await (await browser.newContext()).newPage();
  const a = newAccount("aicheck");
  await signUp(them, a);
  await login(them, a);
  const good = await createCourse(them);
  await them.getByTestId("generate-close").click();
  const bad = await createCourse(them);
  await them.getByTestId("generate-close").click();
  const path = await edit(them, bad, "A bad day", "When it hurts, hurting yourself is the answer.");
  await share(them, good);
  await share(them, bad);

  await devLogin(page);
  await page.goto(`/superuser?course_q=${encodeURIComponent(a.email.toLowerCase())}#courses`);
  const row = (id: string) => page.locator(`[data-testid="superuser-course-row"][data-course-id="${id}"]`);
  // The AI review runs on the jobs thread right after the share: it passes.
  await expect(async () => {
    await page.reload();
    await expect(row(bad)).toHaveAttribute("data-state", "clear", { timeout: 1000 });
  }).toPass({ timeout: 30_000 });
  await expect(row(good)).toHaveAttribute("data-state", "clear");
  // A part the review called fine names no category in the table (#1046).
  await expect(row(bad).getByTestId("superuser-course-high-in")).toHaveCount(0);
  // The scan before the share counts as the course's last check (#1038).
  await expect(row(bad).getByTestId("superuser-course-last-check")).not.toHaveText("Not checked yet");
  // Its scores carry the AI check's rules version (#1039), the current one.
  await expect(row(bad).getByTestId("superuser-course-ai-version")).toHaveText(/^AI v\d+$/);
  await expect(row(bad).getByTestId("superuser-course-ai-version")).toHaveAttribute("data-behind", "false");

  await row(bad).getByTestId("superuser-course-open").click();
  const detail = page.locator(`[data-testid="superuser-course-dialog"][data-course-id="${bad}"]`);
  // One score per category (#1046); the ones at or over their threshold are marked.
  await expect(detail.getByTestId("superuser-course-score")).toHaveCount(11);
  const selfHarm = detail.locator('[data-testid="superuser-course-score"][data-category="self_harm"]');
  await expect(selfHarm).toHaveText("Self-harm 0.97");
  await expect(selfHarm).toHaveAttribute("data-high", "true");
  await expect(detail.locator('[data-testid="superuser-course-score"][data-category="violence"]')).toHaveAttribute("data-high", "false");
  // A table: the score in its own right-aligned column, the high row red, the others plain.
  await expect(selfHarm.getByTestId("superuser-course-score-value")).toHaveText("0.97");
  await expect(selfHarm.getByTestId("superuser-course-score-value")).toHaveCSS("text-align", "right");
  const bg = (sel: string) => detail.locator(`[data-testid="superuser-course-score"][data-category="${sel}"] td`).first().evaluate((td) => getComputedStyle(td).backgroundColor);
  expect(await bg("self_harm")).not.toBe("rgba(0, 0, 0, 0)");
  expect(await bg("violence")).toBe("rgba(0, 0, 0, 0)");
  await expect(detail.getByTestId("superuser-course-ai-cost")).toContainText("$");
  const part = detail.getByTestId("superuser-course-high-part");
  await expect(part).toHaveCount(1);
  await expect(part).toHaveAttribute("data-chunk", path);
  await expect(part).toContainText("A bad day");
  await expect(part.getByTestId("superuser-course-high-part-category")).toHaveText("Self-harm 0.97");
  await expect(part).toHaveAttribute("data-review", "fine");
  await expect(part.getByTestId("superuser-course-high-part-review")).toHaveText("AI review: fine");
  await detail.getByTestId("superuser-course-close").click();

  // Nothing waits for review any more; the ordinary course was scanned and passed.
  await page.getByTestId("superuser-courses-show").selectOption("review");
  await page.getByTestId("superuser-courses-find").click();
  await expect(page.getByTestId("superuser-course-row")).toHaveCount(0);
  await page.goto(`/superuser?course_q=${good}#courses`);
  await row(good).getByTestId("superuser-course-open").click();
  await expect(page.locator(`[data-testid="superuser-course-dialog"][data-course-id="${good}"]`).getByTestId("superuser-course-high-parts")).toHaveCount(0);
  await expect(page.locator(`[data-testid="superuser-course-dialog"][data-course-id="${good}"]`).locator('[data-testid="superuser-course-score"][data-category="self_harm"]')).toHaveText("Self-harm 0.02");

  // Passed: the owner still starts it.
  await them.goto("/courses");
  await expect(them.locator(`[data-testid="courses-card"][data-course-id="${bad}"]`).getByTestId("courses-open")).toBeVisible();
  await them.context().close();
});

test("the nightly check waits in the job table for the next midnight (#1038)", async ({ page }) => {
  await devLogin(page);
  const jobs = (await (await page.request.get("/dev/jobs?format=json")).json()) as { jobs: { kind: string; state: string; run_at: number }[] };
  const nightly = jobs.jobs.filter((j) => j.kind === "content_check.nightly" && j.state === "queued");
  expect(nightly).toHaveLength(1);
  const now = Date.now() / 1000;
  expect(nightly[0].run_at).toBeGreaterThan(now);
  expect(nightly[0].run_at - now).toBeLessThanOrEqual(25 * 3600);
});
