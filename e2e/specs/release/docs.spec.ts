// The docs on a release server (#40): /docs and every page it links answer
// 200 (HTML, and raw Markdown for each page), the pages /docs links to inside
// the app open, and nothing is wider than a phone. The fuller checks
// (specs/docs.spec.ts) need no dev route either, but run as the dev specs.
import { expect, test } from "../../lib/release.ts";

test("/docs and every page it links answer 200 and fit a phone", async ({ page, request }) => {
  test.setTimeout(120_000);
  await page.setViewportSize({ width: 390, height: 844 });
  const home = await page.goto("/docs");
  expect(home?.status()).toBe(200);
  const slugs = await page.locator('[data-testid="docs-section"] [data-testid="docs-page"]').evaluateAll((li) => li.map((l) => l.getAttribute("data-slug")!));
  expect(slugs.length, "pages in the sidebar").toBeGreaterThan(5);
  expect(slugs).toEqual(expect.arrayContaining(["getting-started", "self-hosting", "course-format"]));

  const links = new Set<string>();
  for (const slug of slugs) {
    const r = await page.goto(`/docs/${slug}`);
    expect(r?.status(), `/docs/${slug}`).toBe(200);
    const { sw, cw } = await page.evaluate(() => ({ sw: document.documentElement.scrollWidth, cw: document.documentElement.clientWidth }));
    expect(sw, `/docs/${slug} is ${sw}px wide in a ${cw}px window`).toBeLessThanOrEqual(cw);
    const md = await request.get(`/docs/${slug}.md`);
    expect(md.status(), `/docs/${slug}.md`).toBe(200);
    for (const href of await page.getByTestId("docs-body").locator("a[href]").evaluateAll((as) => as.map((a) => a.getAttribute("href")!))) {
      if (/^[a-z]+:/i.test(href) || href.startsWith("#")) continue;
      links.add(href.split("#")[0]);
    }
  }
  // Every link on them that stays in the app opens.
  for (const path of links) {
    const r = await request.get(path);
    expect(r.status(), `${path} (linked from the docs)`).toBe(200);
  }

  // /llms.txt and what it lists.
  const llms = await request.get("/llms.txt");
  expect(llms.status()).toBe(200);
  for (const [, url] of (await llms.text()).matchAll(/\]\(([^)]+)\)/g)) expect((await request.get(url)).status(), url).toBe(200);
});
