// Jev-judged checks: statements that exact-text assertions can't express or
// that would break when copy changes. Each reports its probability; below
// PASS it fails. Skipped without a Jev key.
import { expect, test } from "@playwright/test";
import { devLogin, generateCourse, generationEnds } from "../lib/app";
import { judge, jevEnabled, PASS, SKIP_REASON } from "../lib/jev";

/** Page text without the lines that are just buttons like "á" or "¿": they read as noise and lower Jev's certainty. */
const words = (text: string) => text.split("\n").filter((l) => l.trim().length > 2).join("\n");

test.describe("judged", { tag: "@jev" }, () => {
  test.skip(!jevEnabled, SKIP_REASON);

  test("a lesson page teaches a topic and ends with a check", async ({ page }, info) => {
    await devLogin(page);
    await page.goto("/lessons");
    // An unread lesson, so the check is still a quiz (a read one shows "Lesson read" instead).
    await page.locator('[data-testid="lessons-item"][data-read="false"]').first().getByTestId("lessons-item-link").click();
    // The teaching and the check go in as separate fields, so neither is cut off.
    const state = {
      page: "a lesson in a Spanish course",
      content: words(await page.getByTestId("lesson-content").innerText()).slice(0, 5_000),
      check: words(await page.getByTestId("lesson-check").innerText()).slice(0, 2_000),
    };
    // One atomic statement per judgement: compound ones come out less sure.
    const teaches = await judge(state, "The `content` teaches a Spanish language topic, with examples.");
    const quiz = await judge({ check: state.check }, "The `check` asks the learner questions to test what they read.");
    info.annotations.push({ type: "jev", description: `teaches p=${teaches.toFixed(2)}, quiz p=${quiz.toFixed(2)}` });
    console.log(`lesson page judged: teaches p=${teaches.toFixed(2)}, quiz p=${quiz.toFixed(2)}`);
    expect(teaches, "Jev: the page teaches a topic with examples").toBeGreaterThanOrEqual(PASS);
    expect(quiz, "Jev: the page ends with a check").toBeGreaterThanOrEqual(PASS);
  });

  test("a failed generation explains itself in plain words", async ({ page }, info) => {
    await generateCourse(page, "Fractions [fixture:provider-error]");
    expect(await generationEnds(page)).toBe("failed");
    const error = await page.getByTestId("generate-error").innerText();

    const p = await judge(
      { error_banner: error },
      "The `error_banner` tells a non-technical learner that course generation failed and hints at what to do next.",
    );
    info.annotations.push({ type: "jev", description: `p=${p.toFixed(2)}` });
    console.log(`generation error judged: p=${p.toFixed(2)}: ${error.replace(/\s+/g, " ").slice(0, 120)}`);
    expect(p).toBeGreaterThanOrEqual(PASS);
  });
});
