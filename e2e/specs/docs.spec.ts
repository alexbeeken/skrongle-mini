// The user docs (#97): reachable logged out, every user page as HTML and as
// raw Markdown, developer pages in their own section, and /llms.txt pointing
// an LLM at the course format and its schema. No account, no key.
import { expect, test } from "@playwright/test";

const USER_PAGES = ["index", "getting-started", "course-structure", "making-courses", "faq"];

test("the docs index is reachable logged out, with user and developer pages apart", async ({ page }) => {
  await page.goto("/docs");
  await expect(page).toHaveURL(/\/docs$/);
  await expect(page.getByTestId("docs-body").locator("h1")).toHaveText("Documentation");

  const guides = page.locator('[data-testid="docs-section"][data-section="guides"] [data-testid="docs-page"]');
  expect(await guides.evaluateAll((li) => li.map((l) => l.getAttribute("data-slug")))).toEqual(USER_PAGES);
  // Guides open the sidebar; Reference follows; Developers holds the developer pages (and anything unlisted).
  await expect(page.getByTestId("docs-section")).toHaveCount(3);
  await expect(page.getByTestId("docs-section-title")).toHaveText(["Guides", "Reference", "Developers"]);
  const dev = page.locator('[data-testid="docs-section"][data-section="dev"]');
  for (const slug of ["dev", "architecture", "testing", "qa"]) await expect(dev.locator(`[data-slug="${slug}"]`)).toHaveCount(1);
  for (const slug of USER_PAGES) await expect(dev.locator(`[data-slug="${slug}"]`)).toHaveCount(0);

  // Every user guide is linked from the index body, and opens.
  for (const slug of USER_PAGES.slice(1)) {
    await expect(page.getByTestId("docs-body").locator(`[data-testid="docs-link"][data-slug="${slug}"]`)).not.toHaveCount(0);
  }
  await page.getByTestId("docs-body").locator('[data-testid="docs-link"][data-slug="getting-started"]').click();
  await expect(page).toHaveURL(/\/docs\/getting-started$/);
  await expect(page.getByTestId("docs-raw")).toHaveAttribute("href", /\/docs\/getting-started\.md$/);
});

test("every user page is served as HTML and as raw Markdown", async ({ request }) => {
  for (const slug of USER_PAGES) {
    const html = await request.get(`/docs/${slug}`);
    expect(html.status(), `/docs/${slug}`).toBe(200);
    expect(html.headers()["content-type"]).toContain("text/html");
    const md = await request.get(`/docs/${slug}.md`);
    expect(md.status(), `/docs/${slug}.md`).toBe(200);
    expect(md.headers()["content-type"]).toContain("text/markdown");
    expect(await md.text()).toMatch(/^# /);
  }
});

test("/llms.txt lists the course format and its schema, and both are served", async ({ request }) => {
  const res = await request.get("/llms.txt");
  expect(res.status()).toBe(200);
  expect(res.headers()["content-type"]).toContain("text/plain");
  const body = await res.text();
  expect(body).toMatch(/^# Skrongle/);
  expect(body).toContain("/docs/course-format.md");
  expect(body).toContain("/docs/course.schema.json");
  expect(body).not.toContain("{base}");
  for (const [, url] of body.matchAll(/\]\(([^)]+)\)/g)) {
    const got = await request.get(url);
    expect(got.status(), url).toBe(200);
  }
});

/** Every page in the sidebar, by slug. */
async function sidebarSlugs(page: import("@playwright/test").Page): Promise<string[]> {
  await page.goto("/docs");
  return page.locator('[data-testid="docs-section"] [data-testid="docs-page"]').evaluateAll((li) => li.map((l) => l.getAttribute("data-slug")!));
}

test("no docs page scrolls sideways at phone width", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  for (const slug of await sidebarSlugs(page)) {
    await page.goto(`/docs/${slug}`);
    const { sw, cw } = await page.evaluate(() => ({ sw: document.documentElement.scrollWidth, cw: document.documentElement.clientWidth }));
    expect(sw, `/docs/${slug} is ${sw}px wide in a ${cw}px window`).toBeLessThanOrEqual(cw);
  }
});

// #676: a long bare URL in a paragraph wraps instead of pushing the page sideways.
test("a long bare link in docs prose wraps at phone width", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/docs/features");
  await page.getByTestId("docs-body").evaluate((body) => {
    const url = "https://docs.aws.amazon.com/AmazonRDS/latest/PostgreSQLReleaseNotes/postgresql-extensions.html";
    const p = document.createElement("p");
    p.innerHTML = `Source: <a href="${url}">${url}</a>`;
    body.append(p);
  });
  const { sw, cw } = await page.evaluate(() => ({ sw: document.documentElement.scrollWidth, cw: document.documentElement.clientWidth }));
  expect(sw, `the page is ${sw}px wide in a ${cw}px window`).toBeLessThanOrEqual(cw);
});

test("every in-app link on every docs page opens, and its #heading exists", async ({ page, request }) => {
  const checked = new Map<string, string[] | null>(); // path -> ids on it, null for a non-HTML target
  const idsOf = async (path: string) => {
    if (!checked.has(path)) {
      const res = await request.get(path);
      expect(res.status(), path).toBe(200);
      const html = (res.headers()["content-type"] ?? "").includes("text/html") ? await res.text() : null;
      checked.set(path, html === null ? null : [...html.matchAll(/\sid="([^"]+)"/g)].map((m) => m[1]));
    }
    return checked.get(path)!;
  };
  for (const slug of await sidebarSlugs(page)) {
    await page.goto(`/docs/${slug}`);
    const hrefs = await page.getByTestId("docs-body").locator("a[href]").evaluateAll((as) => as.map((a) => a.getAttribute("href")!));
    for (const href of hrefs) {
      if (/^[a-z]+:/i.test(href)) continue; // https:, mailto:
      const [path, frag] = href.startsWith("#") ? [`/docs/${slug}`, href.slice(1)] : href.split("#");
      const ids = await idsOf(path);
      if (frag && ids) expect(ids, `/docs/${slug} links to ${href}`).toContain(frag);
    }
  }
});

test.describe("at phone width", () => {
  test.use({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true, deviceScaleFactor: 2 });

  // #279: the list of pages comes after the page, so the heading is on the first screen.
  test("a docs page's heading is on the first screen, the page list after it", async ({ page }) => {
    for (const slug of ["features", "course-format", "index"]) {
      await page.goto(`/docs/${slug}`);
      const h1 = page.getByTestId("docs-body").locator("h1").first();
      await expect(h1).toBeInViewport();
      const box = (await h1.boundingBox())!;
      expect(box.y + box.height, `/docs/${slug}: h1 bottom`).toBeLessThanOrEqual(844);
      const article = (await page.getByTestId("docs-body").boundingBox())!;
      const nav = (await page.getByTestId("docs-page").first().boundingBox())!;
      expect(nav.y, `/docs/${slug}: the page list follows the page`).toBeGreaterThan(article.y);
    }
  });

  // #1105: the page list at the bottom is shown whole, not in a scroll box of its own.
  test("the page list is not cut short, and its last page opens", async ({ page }) => {
    await page.goto("/docs/features");
    const nav = page.getByTestId("docs-nav");
    expect(await nav.evaluate((n) => n.scrollHeight - n.clientHeight)).toBeLessThanOrEqual(1);
    const last = page.getByTestId("docs-page-link").last();
    const href = (await last.getAttribute("href"))!;
    await last.click();
    await expect(page).toHaveURL(new RegExp(`${href}$`));
  });
});

test("on a desktop the page list stays beside the page", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto("/docs/features");
  const article = (await page.getByTestId("docs-body").boundingBox())!;
  const nav = (await page.getByTestId("docs-page").first().boundingBox())!;
  expect(nav.x + nav.width).toBeLessThanOrEqual(article.x);
});

// #1105: the sticky page list is taller than an 800 px window. It fits the
// window and scrolls on its own, so its last pages can be reached mid-page.
test("on a desktop every page in the list can be reached from the middle of a long page", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto("/docs/features");
  await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight / 2));
  const nav = page.getByTestId("docs-nav");
  const box = (await nav.boundingBox())!;
  expect(box.y + box.height, "the page list ends inside the window").toBeLessThanOrEqual(800);
  await nav.evaluate((n) => n.scrollTo(0, n.scrollHeight));
  const last = page.getByTestId("docs-page-link").last();
  await expect(last).toBeInViewport();
  const scrolled = await page.evaluate(() => window.scrollY);
  expect(scrolled + 800, "still mid-page").toBeLessThan(await page.evaluate(() => document.documentElement.scrollHeight));
  const href = (await last.getAttribute("href"))!;
  await last.click();
  await expect(page).toHaveURL(new RegExp(`${href}$`));
});
