// Profile "impatient" (see e2e/lib/persona.ts): a teen on a phone in Sprint mode.
import type { Page } from "@playwright/test";
import { playSprintSession } from "../lib/app";
import type { Persona } from "../lib/persona";

const visible = (page: Page, testid: string) => page.getByTestId(testid).first().isVisible();

export default {
  id: "impatient",
  who: "A teenager learning Spanish on their phone in short bursts, mostly in Sprint mode. They skim, don't read instructions and tap the first plausible option.",
  prefers: "Prefer big buttons and anything that says Start, Continue or Play; prefer streaks and the daily goal; never choose long text to read.",
  patience: 6,
  viewports: ["phone"],
  goals: [
    {
      goal: "Start a short Sprint mode practice session from the home page.",
      done: "A Sprint mode exercise is on screen.",
      check: async (page) => page.url().includes("/sprint/play") && (await visible(page, "sprint-play-stage")),
      // Jev can't solve multi-tap puzzles: the exact driver finishes the session.
      then: async (page, look) => {
        let looked = false;
        await playSprintSession(page, 300, async () => {
          if (looked) return;
          const shown = await page.getByTestId("sprint-play-result").waitFor({ timeout: 2_000 }).then(() => true, () => false);
          if (shown) {
            looked = true;
            await look("after the first answer");
          }
        });
        await look("session finished");
      },
    },
    {
      goal: "Come back to the app and carry on with the lesson you were doing.",
      done: "A Sprint mode exercise is on screen.",
      check: async (page) => page.url().includes("/sprint/play") && (await visible(page, "sprint-play-stage")),
    },
    {
      goal: "See your progress: today's goal, your streak or how far through the course you are.",
      done: "The page shows the learner's daily goal, streak or course progress.",
    },
  ],
  concerns: [
    "The next thing to do is obvious without reading a paragraph.",
    { text: "The page shows the correct answer.", when: "sprint-play-result" },
    { text: "The page shows how far the learner is towards today's goal.", when: "sprint-play-done" },
    { text: "The page shows the learner's streak.", when: "sprint-play-done" },
  ],
  frustrations: [
    "The page starts with a long explanation to read.",
    "The page asks the learner to type before anything else.",
  ],
  checks: ["overflow"],
} satisfies Persona;
