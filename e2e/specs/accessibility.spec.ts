// Accessibility fixes from the axe-core pass on #342: large text on a phone,
// the system's reduce-motion setting, the skip link, /sprint's heading and
// the status pills' contrast.
import { expect, test, type Page } from "@playwright/test";
import { devLogin } from "../lib/app";

/** How far the page scrolls sideways, in CSS pixels. */
async function sidewaysScroll(page: Page): Promise<number> {
  return page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
}

test("pages fit a phone at 200 % text size (#342)", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await devLogin(page);
  // /guide and /tutor since #1058. /self-host was here too, until the page
  // was hidden on the hosted site (#1435); put it back when mini ships.
  for (const path of ["/sprint", "/profile", "/", "/courses", "/lessons", "/listening", "/guide", "/tutor"]) {
    await page.goto(path);
    // What the browser's text size setting does: the root font size doubles.
    await page.addStyleTag({ content: "html { font-size: 200% !important; }" });
    expect(await sidewaysScroll(page), `${path} scrolls sideways at 200 % text`).toBeLessThanOrEqual(1);
  }
});

test("the root font size follows the browser's text size (#342)", async ({ page }) => {
  await devLogin(page);
  for (const path of ["/", "/sprint"]) {
    await page.goto(path);
    // A percentage, not a fixed 16px, so the browser's own setting still applies.
    const ratio = await page.evaluate(() => {
      const html = document.documentElement;
      const root = parseFloat(getComputedStyle(html).fontSize);
      html.style.setProperty("font-size", "initial");
      const browser = parseFloat(getComputedStyle(html).fontSize);
      html.style.removeProperty("font-size");
      return root / browser;
    });
    expect(ratio, path).toBeGreaterThanOrEqual(1);
    expect(ratio, path).toBeLessThan(1.1);
  }
});

test("the system's reduce-motion setting stops animations and transitions (#342)", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await devLogin(page);
  for (const path of ["/", "/courses", "/profile", "/lessons"]) {
    await page.goto(path);
    const moving = await page.evaluate(() =>
      [...document.querySelectorAll("*")]
        .filter((e) => {
          const s = getComputedStyle(e);
          const secs = (v: string) => Math.max(...v.split(",").map((d) => parseFloat(d) || 0));
          return (s.animationName !== "none" && secs(s.animationDuration) > 0) || secs(s.transitionDuration) > 0;
        })
        .map((e) => `${e.tagName.toLowerCase()}.${[...e.classList].join(".")}`)
        .slice(0, 5),
    );
    expect(moving, path).toEqual([]);
  }
});

test("the first Tab reaches a skip link that jumps to the content (#342)", async ({ page }) => {
  await devLogin(page);
  await page.goto("/lessons");
  await page.keyboard.press("Tab");
  const skip = page.getByTestId("skip-to-content");
  await expect(skip).toBeFocused();
  await expect(skip).toBeInViewport();
  await page.keyboard.press("Enter");
  await expect(page.locator("main#main")).toBeFocused();
});

test("/sprint has one page heading (#342)", async ({ page }) => {
  await devLogin(page);
  await page.goto("/sprint");
  await expect(page.locator("h1")).toHaveCount(1);
  await expect(page.getByTestId("sprint-title")).toHaveText(/\S/);
});

test("status pills and links in text have 4.5:1 contrast (#342)", async ({ page }) => {
  await devLogin(page);
  await page.goto("/lessons");
  const low = await page.evaluate(() => {
    const rgb = (c: string) => (c.match(/[\d.]+/g) ?? []).slice(0, 3).map(Number);
    const lum = (c: string) => {
      const [r, g, b] = rgb(c).map((v) => v / 255).map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
      return 0.2126 * r + 0.7152 * g + 0.0722 * b;
    };
    const ratio = (a: string, b: string) => {
      const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x);
      return (hi + 0.05) / (lo + 0.05);
    };
    // The nearest ancestor with a solid background.
    const bg = (e: Element | null): string => {
      for (; e; e = e.parentElement) {
        const c = getComputedStyle(e).backgroundColor;
        if (!/rgba\(.*,\s*0\)$/.test(c) && c !== "transparent") return c;
      }
      return "rgb(255, 255, 255)";
    };
    return [...document.querySelectorAll(".pill")]
      .map((e) => ({ cls: e.className, r: ratio(getComputedStyle(e).color, bg(e)) }))
      .filter((x) => x.r < 4.5)
      .map((x) => `${x.cls} ${x.r.toFixed(2)}`);
  });
  expect(low).toEqual([]);
});

test("inline code keeps its size with the % root font size (#342)", async ({ page }) => {
  // A lone `monospace` under a % root size makes Chrome shrink code to 13/16.
  // /self-host was checked too, until it was hidden (#1435).
  for (const path of ["/docs"]) {
    await page.goto(path);
    const ratio = await page.locator("main p code").first().evaluate((c) =>
      parseFloat(getComputedStyle(c).fontSize) / parseFloat(getComputedStyle(c.parentElement!).fontSize));
    expect(ratio, path).toBeCloseTo(0.9, 2);
  }
});
