// The AI content check, stage 2 (#1037): a part Jev scored high (stage 1,
// #1036) is reviewed by a model in context. The dev server's Jev stand-in
// (data/test-fixtures/jev/content-check.json) scores "hurting yourself is
// the answer" and "people like them are vermin" high; the simulated model
// answers from data/test-fixtures/llm/content-review.json, picked by the
// [fixture:NAME] token in the text: "harmful" flags the course with a
// report and the quote, "harmful-new" flags it again past an override note.
// A false friend the review passes is in content-check.spec.ts; the prompt,
// the read loop, the hash cache and who pays are in
// content_check::review::tests.
import { expect, test, type Page } from "@playwright/test";
import { createCourse, devLogin } from "../lib/app";
import { login, newAccount, signUp } from "../lib/learner";

test.skip(process.env.E2E_MINI === "1", "the content check is hosted only; Skrongle mini leaves it out");

async function file(page: Page, id: string, path: string): Promise<Record<string, unknown>> {
  return JSON.parse(((await (await page.request.get(`/api/courses/${id}/file?path=${encodeURIComponent(path)}`)).json()) as { content: string }).content);
}

async function save(page: Page, id: string, path: string, json: Record<string, unknown>) {
  const r = await page.request.post(`/api/courses/${id}/files`, { data: { files: [{ path, content: JSON.stringify(json, null, 2) }] } });
  expect(r.ok()).toBe(true);
}

let shares = 0;
/** Share the course with a new address: the scan runs first, and the share goes through. */
async function share(page: Page, id: string) {
  shares += 1;
  const r = await page.request.post(`/courses/${id}/share`, { form: { emails: `friend-${shares}-${id}@example.org` }, maxRedirects: 0 });
  expect(r.status()).toBe(303);
  expect(r.headers()["location"]).not.toContain("flagged");
}

/** Today's hosted spend on /superuser, in dollars. */
async function spent(page: Page): Promise<number> {
  await page.goto("/superuser");
  const text = (await page.getByTestId("superuser-ai-today-spent").textContent()) ?? "";
  return Number(/\$([\d.]+)/.exec(text)?.[1] ?? "NaN");
}

function row(page: Page, id: string) {
  return page.locator(`[data-testid="superuser-course-row"][data-course-id="${id}"]`);
}

/** The course table, found by the course id. A goto that differs only by
 * its hash doesn't load the page again, so it reloads too. */
async function table(page: Page, id: string) {
  await page.goto(`/superuser?course_q=${id}#courses`);
  await page.reload();
}

/** Reload the course table until the course's row is in `state`. */
async function waitForState(page: Page, id: string, state: string) {
  await expect(async () => {
    await table(page, id);
    await expect(row(page, id)).toHaveAttribute("data-state", state, { timeout: 1000 });
  }).toPass({ timeout: 30_000 });
}

test("a harmful course is flagged with a short report; an override note is followed until new harm comes", async ({ page, browser }) => {
  const them = await (await browser.newContext()).newPage();
  const a = newAccount("aireview");
  await signUp(them, a);
  await login(them, a);
  const id = await createCourse(them);
  await them.getByTestId("generate-close").click();
  const listing = (await (await them.request.get(`/api/courses/${id}/files`)).json()) as { files: { path: string }[] };
  const path = listing.files.map((f) => f.path).find((p) => p.startsWith("lessons/"))!;
  const lesson = await file(them, id, path);
  lesson.title = "A bad day";
  lesson.summary = "When it hurts, hurting yourself is the answer. [fixture:harmful]";
  await save(them, id, path, lesson);

  await devLogin(page);
  const before = await spent(page);
  await share(them, id);

  // Within a minute: flagged by the AI check, with the report and the quote.
  await waitForState(page, id, "flagged");
  await expect(row(page, id)).toHaveAttribute("data-source", "ai");
  // The table says which category scored high (#1046).
  await expect(row(page, id).getByTestId("superuser-course-high-in")).toHaveText("Self-harm");
  await row(page, id).getByTestId("superuser-course-open").click();
  const detail = page.locator(`[data-testid="superuser-course-dialog"][data-course-id="${id}"]`);
  await expect(detail.getByTestId("superuser-course-reason")).toContainText("That encourages self-harm");
  const quote = detail.getByTestId("superuser-course-match");
  await expect(quote).toContainText("Self-harm");
  await expect(quote).toContainText("A bad day");
  await expect(quote).toContainText(`${path} /summary`);
  // The quote is masked; held down it shows.
  const text = quote.getByTestId("masked-word");
  await expect(text).toHaveText("h****** ******** ** *** ******");
  await expect(text).toHaveAttribute("data-tip", "Click to reveal text");
  await expect(detail).not.toContainText("hurting yourself");
  await text.hover();
  await page.mouse.down();
  await expect(text).toHaveText("hurting yourself is the answer");
  await page.mouse.up();
  await expect(text).toHaveText("h****** ******** ** *** ******");
  await expect(detail.getByTestId("superuser-course-high-part-review")).toHaveText("AI review: harmful");
  await detail.getByTestId("superuser-course-close").click();
  expect(await spent(page), "the review's calls count in today's spend").toBeGreaterThan(before);

  // The owner's card: the red ! quotes the text and says why, and nothing opens.
  await them.goto("/courses");
  const card = them.locator(`[data-testid="courses-card"][data-course-id="${id}"]`);
  await expect(card.getByTestId("courses-open")).toHaveCount(0);
  await card.getByTestId("courses-card-flagged").click();
  const dialog = card.getByTestId("courses-card-flag-dialog");
  await expect(dialog).toContainText("The AI check found text that can't be in a course:");
  await expect(dialog.getByTestId("courses-card-flag-match")).toHaveText("encourages self-harm · h****** ******** ** *** ****** · A bad day (/summary)");
  await expect(dialog.getByTestId("masked-word")).toHaveAttribute("data-tip", "Click to reveal text");
  await dialog.getByTestId("courses-card-flag-close").click();

  // A superuser overrides it with a note.
  await table(page, id);
  await row(page, id).getByTestId("superuser-course-open").click();
  await detail.getByTestId("superuser-course-override").click();
  await page.getByTestId("superuser-override-note").fill("A story about a character's low point, discussed in the next lesson; fine");
  await page.getByTestId("superuser-override-confirm").click();
  await expect(row(page, id)).toHaveAttribute("data-state", "overridden");

  // An unrelated edit: scanned, nothing scores high, it stays open.
  const manifest = await file(them, id, "course.json");
  manifest.description = "A short course about feelings.";
  await save(them, id, "course.json", manifest);
  await share(them, id);
  await table(page, id);
  await expect(row(page, id)).toHaveAttribute("data-state", "overridden");
  await them.goto("/courses");
  await expect(card.getByTestId("courses-open")).toBeVisible();

  // New harmful text the note doesn't cover: flagged again, and the report says so.
  manifest.description = "A short course about feelings. People like them are vermin. [fixture:harmful-new]";
  await save(them, id, "course.json", manifest);
  await share(them, id);
  await waitForState(page, id, "flagged");
  await row(page, id).getByTestId("superuser-course-open").click();
  await expect(detail.getByTestId("superuser-course-reason")).toContainText("The override note was considered");
  await expect(detail.getByTestId("superuser-course-match").getByTestId("masked-word")).toHaveAttribute("data-word", "people like them are vermin");
  await expect(detail.getByTestId("superuser-course-match")).toContainText("Hate speech");
  await expect(detail.getByTestId("superuser-course-override-note")).toContainText("a character's low point");
  await them.context().close();
});
