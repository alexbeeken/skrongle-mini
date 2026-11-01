// The privacy notice (#975): /privacy is public, carries its "Last updated"
// line, and is the published text, not the draft (no DRAFT banner, no
// [PLACEHOLDER] left), with the summary list and the section 14 table
// readable at phone width. No account, no key.
import { expect, test } from "@playwright/test";

test("the privacy notice is public, dated, and no longer a draft", async ({ page }) => {
  const r = await page.goto("/privacy");
  expect(r?.status()).toBe(200);
  await expect(page).toHaveURL(/\/privacy$/);
  const body = page.getByTestId("docs-body");
  await expect(body.locator("h1")).toHaveText("Privacy policy");
  await expect(body).toContainText(/Last updated:\s*[A-Z][a-z]+ \d{1,2}, \d{4}/);
  const text = (await body.innerText()) ?? "";
  expect(text).not.toContain("DRAFT");
  expect(text).not.toContain("[");
  expect(text).not.toContain("]");
  // The raw Markdown is served too, and is the same text.
  const md = await page.request.get("/docs/privacy.md");
  expect(md.status()).toBe(200);
  expect(await md.text()).toMatch(/^# Privacy policy\n/);
  // Linked from the footer, logged out.
  await page.goto("/login");
  await expect(page.getByTestId("footer-privacy")).toHaveAttribute("href", /\/privacy$/);
});

test("the notice has one heading per section, the summary list and the US-rights table", async ({ page }) => {
  await page.goto("/privacy");
  const body = page.getByTestId("docs-body");
  for (const n of [1, 14, 15, 20, 23]) {
    await expect(body.locator("h2", { hasText: new RegExp(`^${n}\\. `) }), `section ${n}`).toHaveCount(1);
  }
  await expect(body.locator("h2", { hasText: "SUMMARY OF KEY POINTS" })).toHaveCount(1);
  await expect(body.locator("h2 + p + ul li", { hasText: "What personal information do we process?" })).toHaveCount(1);
  const table = body.locator("table").first();
  await expect(table.locator("th")).toHaveText(["Category", "Examples", "Collected"]);
  await expect(table.locator("tbody tr")).toHaveCount(12);
  await expect(table.locator("tbody tr", { hasText: "Commercial information" }).locator("td").last()).toHaveText("YES");
  // The in-page links of the summary and the table of contents land on a heading.
  for (const a of await body.locator('a[href^="#"]').all()) {
    const id = (await a.getAttribute("href"))!.slice(1);
    await expect(body.locator(`[id="${id}"]`), `#${id}`).toHaveCount(1);
  }
});

test("at phone width the notice does not scroll sideways and the table can be scrolled on its own", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/privacy");
  const { sw, cw } = await page.evaluate(() => ({ sw: document.documentElement.scrollWidth, cw: document.documentElement.clientWidth }));
  expect(sw, `/privacy is ${sw}px wide in a ${cw}px window`).toBeLessThanOrEqual(cw);
  const table = page.getByTestId("docs-body").locator("table").first();
  await table.scrollIntoViewIfNeeded();
  await expect(table).toBeVisible();
  expect(await table.evaluate((t) => getComputedStyle(t).overflowX)).toBe("auto");
});
