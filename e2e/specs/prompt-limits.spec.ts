// Exact specs for the AI prompt boxes' limits (#186) and the Generate
// dialog's calm refusal (#188). Under the Generate dialog's wish and notes
// and Use AI's text a live count reads "45 / 2,000 characters": red below 20
// characters (an optional box may stay empty) and above the maximum, normal
// text in between. The server refuses the same lengths with the same limits.
// A Generate form that is too short is refused in place: no reload, the
// dialog stays where it is, the error eases open at the top and is scrolled
// into view. Fixtures only.
import { expect, test, type Locator, type Page } from "@playwright/test";
import { createCourse, devLogin, devReset } from "../lib/app";

test.beforeEach(async ({ page }) => {
  await devLogin(page, "keys=fake&next=/courses");
});
test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devReset(page);
  await page.close();
});

const TOO_SHORT = "Your prompt is too short! Please tell us what you'd like to learn or paste your notes so the AI has enough information to get to work.";

/** The computed colour of `var(name)` on this page. */
async function colour(page: Page, name: string): Promise<string> {
  return page.evaluate((v) => {
    const s = document.createElement("span");
    s.style.color = `var(${v})`;
    document.body.append(s);
    const c = getComputedStyle(s).color;
    s.remove();
    return c;
  }, name);
}

/** Put `n` characters in the box past its maxlength, as a paste the browser allowed would. */
async function setLength(box: Locator, n: number): Promise<void> {
  await box.evaluate((el: HTMLTextAreaElement, len) => {
    el.value = "x".repeat(len);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  }, n);
}

/** The count at each length: its words, and red (`bad`) or normal text. */
async function expectCounts(page: Page, box: Locator, count: Locator, max: number, optional: boolean) {
  const [bad, normal] = [await colour(page, "--bad"), await colour(page, "--text")];
  const grouped = (n: number) => n.toLocaleString("en-US");
  const cases: [number, string, boolean][] = [
    [0, optional ? "" : " · at least 20", !optional],
    [19, " · at least 20", true],
    [20, "", false],
    [max, "", false],
  ];
  for (const [n, problem, red] of cases) {
    await box.fill("x".repeat(n));
    await expect(count, `${n} characters`).toHaveText(`${grouped(n)} / ${grouped(max)} characters${problem}`);
    await expect(count, `${n} characters`).toHaveAttribute("data-state", red ? "bad" : "ok");
    await expect(count, `${n} characters`).toHaveCSS("color", red ? bad : normal);
  }
  // The browser stops typing at the maximum ...
  await box.fill("x".repeat(max + 1));
  await expect(box).toHaveValue("x".repeat(max));
  // ... but a longer text that got in anyway is red too.
  await setLength(box, max + 1);
  await expect(count).toHaveText(`${grouped(max + 1)} / ${grouped(max)} characters · at most ${grouped(max)}`);
  await expect(count).toHaveCSS("color", bad);
  // Whitespace around the text does not count, as on the server.
  await box.fill(`   ${"x".repeat(20)}  \n`);
  await expect(count).toHaveText(`20 / ${grouped(max)} characters`);
}

test("the Generate dialog counts the wish and the notes, red outside 20 to the maximum", async ({ page }) => {
  await createCourse(page);
  // The wish is needed (the course has no title yet): red when empty.
  await expectCounts(page, page.getByTestId("generate-wish"), page.getByTestId("generate-wish-count"), 2000, false);
  // The notes are optional: empty is fine, a few characters are not.
  await expectCounts(page, page.getByTestId("generate-notes"), page.getByTestId("generate-notes-count"), 60_000, true);
});

test("the server refuses Generate prompts outside the limits, in the same words", async ({ page }) => {
  const id = await createCourse(page);
  const post = async (form: Record<string, string>) => {
    const r = await page.request.post(`/courses/${id}/generate`, { form: { provider: "anthropic", ...form }, maxRedirects: 0 });
    expect(r.status(), JSON.stringify(form).slice(0, 80)).toBe(200);
    return r.text();
  };
  const x = (n: number) => "x".repeat(n);
  expect(await post({ wish: x(19) })).toContain(TOO_SHORT.replace("'", "&#39;"));
  expect(await post({ wish: x(25), notes: x(19) })).toContain(TOO_SHORT.replace("'", "&#39;"));
  expect(await post({ wish: x(2001) })).toContain("Keep what you want to learn to 2,000 characters at most; put longer material in the notes box.");
  expect(await post({ wish: x(25), notes: x(60_001) })).toContain("Keep your material to 60,000 characters at most; trim it or split the course in two.");
  // Nothing was started by any of them.
  await page.goto(`/courses/${id}`);
  await expect(page.getByTestId("course-ai-modal")).toHaveCount(0);
});

test("Use AI counts its text, red outside 20 to the maximum, and the server agrees", async ({ page }) => {
  const id = await createCourse(page);
  await page.getByTestId("generate-close").click();
  const text = page.getByTestId("course-ai-text");
  // A draft can only be added to: one lesson's brief, up to 1,000 characters.
  await expectCounts(page, text, page.getByTestId("course-ai-text-count"), 1000, false);

  const post = async (t: string) => {
    const r = await page.request.post(`/courses/${id}/ai`, { form: { text: t }, maxRedirects: 0 });
    expect(r.status()).toBe(200);
    return r.text();
  };
  expect(await post("x".repeat(19))).toContain("Say what the new lesson should teach, in at least 20 characters.");
  expect(await post("x".repeat(1001))).toContain("Keep it to 1,000 characters at most.");
});

test("a Generate form that is too short is refused in place, calmly", async ({ page }) => {
  await createCourse(page);
  const dialog = page.getByTestId("generate");
  const slot = page.locator("#gen-error-slot");
  const error = page.getByTestId("generate-error");
  const wish = page.getByTestId("generate-wish");
  await expect(error).toBeHidden();

  await wish.fill("ab");
  // A page that reloaded would lose this.
  await page.evaluate(() => ((window as unknown as { stayed: boolean }).stayed = true));
  await page.getByTestId("generate-submit").scrollIntoViewIfNeeded();
  const before = (await dialog.boundingBox())!;
  const url = page.url();

  // Watch the slot's height on every frame while the error opens.
  const heights = page.evaluate(() => new Promise<number[]>((done) => {
    const el = document.getElementById("gen-error-slot")!;
    const seen: number[] = [];
    const t0 = performance.now();
    const tick = () => {
      seen.push(el.getBoundingClientRect().height);
      if (performance.now() - t0 < 700) requestAnimationFrame(tick);
      else done(seen);
    };
    requestAnimationFrame(tick);
  }));
  await page.getByTestId("generate-submit").click();

  await expect(error).toHaveText(TOO_SHORT);
  await expect(error).toBeVisible();
  expect(page.url()).toBe(url);
  expect(await page.evaluate(() => (window as unknown as { stayed?: boolean }).stayed)).toBe(true);
  await expect(wish).toHaveValue("ab");
  await expect(wish).toHaveAttribute("aria-invalid", "");
  await expect(wish).toBeFocused();

  // It eased open: some frames saw it part-way, none saw it shrink.
  const seen = await heights;
  const full = seen[seen.length - 1];
  expect(full).toBeGreaterThan(20);
  expect(seen.some((h) => h > 1 && h < full - 1), `heights: ${seen.join(", ")}`).toBe(true);
  expect(seen.every((h, i) => i === 0 || h >= seen[i - 1] - 0.5), `heights: ${seen.join(", ")}`).toBe(true);

  // The dialog did not move or resize; it scrolled (smoothly) until the error is in view, then stopped.
  await expect.poll(async () => {
    const a = await dialog.evaluate((d) => d.scrollTop);
    await page.waitForTimeout(100);
    return a === (await dialog.evaluate((d) => d.scrollTop));
  }).toBe(true);
  const after = (await dialog.boundingBox())!;
  for (const k of ["x", "y", "width", "height"] as const) expect(Math.abs(after[k] - before[k]), k).toBeLessThanOrEqual(1);
  const box = (await error.boundingBox())!;
  expect(box.y).toBeGreaterThanOrEqual(after.y);
  expect(box.y + box.height).toBeLessThanOrEqual(after.y + after.height);

  // Fixing the wish closes the error again, in place.
  await wish.fill("Fractions for my Friday test");
  await expect(slot).not.toHaveAttribute("data-shown");
  await expect(error).toBeHidden();
  await expect(wish).not.toHaveAttribute("aria-invalid");
});

test("short notes are refused in place too, and the notes box is the one marked", async ({ page }) => {
  await createCourse(page);
  await page.getByTestId("generate-wish").fill("Fractions for my Friday test");
  await page.getByTestId("generate-notes").fill("halves");
  await page.getByTestId("generate-submit").click();
  await expect(page.getByTestId("generate-error")).toHaveText(TOO_SHORT);
  await expect(page.getByTestId("generate-notes")).toHaveAttribute("aria-invalid", "");
  await expect(page.getByTestId("generate-wish")).not.toHaveAttribute("aria-invalid");
  await expect(page.getByTestId("generate-submit")).toBeEnabled();
});
