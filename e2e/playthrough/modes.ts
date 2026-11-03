// The modes, as data: where a mode lists its lessons, the phases of playing
// one lesson there, and what the player must leave alone. Everything else
// (the loop, the fudge, the checks, the report) is shared in core.ts, and
// Jev does the playing (player.ts). A new mode or a new kind of session is
// one more entry here. Nothing in this file is about one course.
import type { Page } from "@playwright/test";
import type { Phase } from "./player.ts";

export interface Mode {
  id: string;
  title: string;
  /** The page that lists the lessons in the order the mode offers them. */
  index: string;
  /** The testid of each lesson on it; it carries data-lesson-id, data-passed and data-state (locked). */
  item: string;
  phases: (slug: string) => Phase[];
  avoid: string[];
  skip: string[];
  advice: string;
  viewport: { width: number; height: number };
}

const visible = (page: Page, testid: string) => page.getByTestId(testid).first().isVisible();
const PHONE = { width: 390, height: 844 };
const DESK = { width: 1280, height: 900 };

export const MODES: Record<string, Mode> = {
  sprint: {
    id: "sprint",
    title: "Sprint mode",
    index: "/sprint?reduce-motion=1",
    item: "sprint-lesson",
    phases: (slug) => [
      {
        name: "session",
        url: `/sprint/play?lesson=${encodeURIComponent(slug)}`,
        goal: "Finish this lesson session: read each quick bite, answer every exercise right, and continue until the session is complete.",
        // The done screen, once it has saved the session.
        done: async (page) => (await visible(page, "sprint-play-done")) && (await page.getByTestId("sprint-play-done").getAttribute("data-saved")) === "true",
      },
    ],
    avoid: ["sprint-play-exit", "sprint-play-again", "sprint-play-audio", "sprint-play-sound"],
    skip: [".sprint-link:not(.sprint-cant)"], // "type it instead": the tiles are the way here
    advice: "Tap tiles and options; to build a sentence tap its words in order, then Check. To sort, tap the bucket the item belongs in. To match, tap a term then its meaning.",
    viewport: PHONE,
  },
  study: {
    id: "study",
    title: "Study mode",
    index: "/lessons",
    item: "lessons-item",
    phases: (slug) => [
      {
        name: "read",
        url: `/lessons/${encodeURIComponent(slug)}`,
        goal: "Read this lesson, then answer the reading check at the bottom and submit it (or press the button that says you've read it).",
        done: async (page) =>
          (await page.getByTestId("lesson").getAttribute("data-read")) === "true" || (await visible(page, "lesson-check-result")),
      },
      {
        name: "practice",
        url: `/practice?lesson=${encodeURIComponent(slug)}`,
        goal: "Practise this lesson: answer every question right, press Next after each, until the quiz is complete.",
        done: (page) => visible(page, "quiz-summary"),
      },
    ],
    // The conversation starts an AI call; the rest leave the page.
    avoid: ["lesson-conversation-form", "lesson-conversation-start", "lesson-listen", "lesson-tutor", "nav-logout", "quiz-again"],
    skip: [],
    advice: "The lesson's text above the questions has what you need.",
    viewport: DESK,
  },
  play: {
    id: "play",
    title: "Play mode",
    index: "/guide",
    item: "guide-stage",
    phases: (slug) => [
      {
        name: "stage",
        url: `/guide/${encodeURIComponent(slug)}`,
        goal: "Play this Guide stage to the end: read each concept, look at the terms, answer each quick check right, and continue until the stage is complete.",
        done: (page) => visible(page, "stage-done"),
      },
    ],
    avoid: ["stage-replay"],
    skip: [],
    advice: "Flashcards turn over when tapped; Next moves on.",
    viewport: DESK,
  },
};
