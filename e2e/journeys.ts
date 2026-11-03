// Journeys: goals Jev walks toward in a real browser. Each one becomes a
// test named "journey <id>" (specs/journeys.spec.ts), tagged @jev, so:
//
//   scripts/e2e-jev.sh               # every journey and Jev judgement
//   scripts/e2e-jev.sh open-lesson   # one journey, by id
//
// To add one, append an entry. Keep `goal` a plain instruction a person
// could follow, give `check` whenever the end state shows in markup (then
// Jev only steers and the pass/fail is exact), and give `inputs` for any
// box the journey has to type into. Journeys start logged in as the seeded
// dev user with Spanish open, on a server with LLM fixtures.
import type { Page } from "@playwright/test";
import { login, newAccount, register, signUp } from "./lib/learner";

export interface Journey {
  /** Stable kebab-case id: the test name and the report file name. */
  id: string;
  /** Query for /dev/login (default "open=spanish"); `keys=fake` saves the simulated provider's keys, which unlock the AI forms. */
  login?: string;
  /** Where to start, after logging in. */
  start: string;
  /** Runs after logging in, before Jev; may return the path to start from instead (e.g. a course it made). */
  setup?: (page: Page) => Promise<string | void>;
  /** The goal in plain words. Jev reads it each step to choose an action. */
  goal: string;
  /** A statement Jev judges true once the goal is reached; used when there is no `check`. */
  done: string;
  /** Exact end-state check. When given it decides pass/fail; Jev only steers. */
  check?: (page: Page) => Promise<boolean>;
  /** What to type: testid -> text ("*" for any box). Jev can choose these but not write. */
  inputs?: Record<string, string>;
  /** Testids never to touch, on top of the built-in list (logout, delete, …). */
  avoid?: string[];
  /** Steps before giving up (default 15). */
  maxSteps?: number;
  /** Phone-sized viewport (Sprint mode is phone-first). */
  phone?: boolean;
  /** Walk as this persona (a file in personas/): its `who`, `prefers` and `patience` (the default `maxSteps`). */
  persona?: string;
}

const visible = (page: Page, testid: string) => page.getByTestId(testid).first().isVisible();
/** The course editor saves by itself: its circle is green when everything is saved. */
/** The editor's circle is green. A save in flight ("saving", up to SETTLE_MS plus the request) is waited for: the step before it may have just made the edit. */
const saved = async (page: Page) => {
  const state = () => page.getByTestId("course-edit-save-status").getAttribute("data-state", { timeout: 1_000 }).catch(() => null);
  for (let i = 0; i < 15 && (await state()) === "saving"; i++) await page.waitForTimeout(200);
  return (await state()) === "saved";
};

/** The dev user's own course from the LLM fixtures (units Basics and Operations); returns its page. */
async function ownCourse(page: Page): Promise<string> {
  await page.goto("/courses");
  await page.getByTestId("courses-create").click();
  await page.getByTestId("generate-wish").fill("Fractions for a journey");
  await page.getByTestId("generate-submit").click();
  // The editor's progress modal closes by itself once the course is written.
  await page.waitForURL(/\?job=/);
  await page.getByTestId("course-ai-modal").waitFor({ state: "detached", timeout: 30_000 });
  return `/courses/${await page.getByTestId("course-edit").getAttribute("data-course-id")}`;
}
/** Course-page buttons a journey on the editor must not press unless it is the goal. */
const EDITOR_AVOID = ["course-edit-delete", "course-lessons-delete", "course-undo", "course-edit-export", "course-ai-modal-cancel"];

export const journeys: Journey[] = [
  {
    id: "open-lesson",
    start: "/",
    goal: "Open the page of any lesson in the course, the page that teaches it and ends with a short reading check.",
    done: "The page shows a single lesson's teaching content.",
    check: async (page) => (await visible(page, "lesson")) && (await visible(page, "lesson-content")),
    maxSteps: 6,
  },
  {
    id: "find-generate-form",
    login: "keys=fake",
    start: "/",
    // Since #144 the form is a dialog in a new course's editor: Create a course opens it.
    goal: "Go to your courses, start a new course of your own, and get to the form where you describe it and the AI writes it for you.",
    done: "A form for asking the AI to generate the new course is on screen.",
    check: (page) => visible(page, "generate-form"),
    maxSteps: 10,
  },
  {
    // Getting into Sprint mode, not playing it: the exact spec plays a whole session
    // (lib/app.ts playSprintSession). Jev chooses well between pages and links;
    // multi-tap puzzles like match-the-pairs are not its strength.
    id: "start-sprint",
    persona: "impatient",
    phone: true,
    start: "/",
    goal: "Start a short Sprint mode practice session on the phone, from the home page.",
    done: "A Sprint mode exercise is on screen.",
    check: async (page) => page.url().includes("/sprint/play") && (await visible(page, "sprint-play-stage")),
  },
  {
    // Promoted from an exploratory run of `classroom` (#40): reached in 3 steps, check from the final page's state.
    id: "find-topic-lesson",
    persona: "classroom",
    phone: true,
    start: "/",
    goal: 'Find the lesson that teaches "ser vs estar", and open it.',
    done: 'The page is a lesson that teaches "ser vs estar".',
    check: async (page) => (await page.locator('[data-testid="lesson"][data-lesson-id="ser-vs-estar"]').count()) > 0,
    inputs: { "lessons-filter": "ser vs estar" },
    maxSteps: 6,
  },
  {
    // The monkey run (#36): a sweep of the header's sections to catch dead links
    // and broken pages without a script per screen. No exact check: "every
    // section" spans pages, so Jev judges `done` from the steps so far.
    id: "monkey-run",
    start: "/",
    goal: "Visit every section in the main navigation: the home page, the course list (the course switch), your profile and the docs (the last two are in the account menu).",
    done: "The steps so far have visited the home page, the course list, the profile and the docs, each of which loaded.",
    avoid: ["courses-open", "courses-resume", "courses-cancel", "profile-password-submit"],
    maxSteps: 15,
  },
  // The course editor (#113, #114): each starts on a course of the dev user's own.
  {
    id: "share-course",
    login: "keys=fake",
    start: "/courses",
    setup: ownCourse,
    goal: "Share this course with the e-mail address friend@example.com so they can learn from it.",
    done: "The Share dialog lists friend@example.com as invited.",
    check: async (page) => (await page.locator('[data-testid="course-share-row"][data-email="friend@example.com"]').count()) > 0,
    inputs: { "course-share-emails": "friend@example.com" },
    avoid: [...EDITOR_AVOID, "course-share-remove"],
    maxSteps: 8,
  },
  {
    id: "add-lesson-with-ai",
    login: "keys=fake",
    start: "/courses",
    setup: ownCourse,
    goal: "Have the AI add one new lesson on comparing fractions to this course, without changing the rest, and wait until it is in.",
    done: "The page says the AI has added the lesson.",
    // Done: the review of what it changed opens by itself.
    check: async (page) => (await page.locator('[data-testid="course-review-modal"][data-after-job="true"]').filter({ hasText: "Added with AI" }).count()) > 0,
    inputs: { "course-ai-text": "A lesson on comparing two fractions" },
    avoid: EDITOR_AVOID,
    maxSteps: 8,
  },
  {
    // Leaving part of an AI run out (#189): the review's − and Accept are the point.
    id: "leave-out-ai-lesson",
    login: "keys=fake",
    start: "/courses",
    setup: ownCourse,
    goal: "Have the AI add one new lesson to this course, then, in the review of its changes, decide you do not want that lesson after all: leave it out and accept, so the course is as it was.",
    done: "The page says the AI's changes are in without the parts left out, and the lesson table has no new lesson.",
    check: async (page) => (await page.getByTestId("course-edit-notice").filter({ hasText: "without the parts you left out" }).count()) > 0,
    inputs: { "course-ai-text": "A lesson on comparing two fractions" },
    avoid: EDITOR_AVOID,
    maxSteps: 12,
  },
  {
    // Forms mode is behind the table's JSON switch (on = JSON): finding it is the point.
    id: "edit-lesson-as-form",
    login: "keys=fake",
    start: "/courses",
    setup: ownCourse,
    goal: 'Change the summary of the lesson "What a fraction is" using the form view rather than raw JSON, and make sure the change is saved (the editor saves by itself; there is no Save button).',
    done: "The circle by the course title is green (all changes saved) and the Undo line names one lesson edited.",
    check: async (page) => (await saved(page)) && /1 edited/.test((await page.getByTestId("course-undo-label").textContent().catch(() => "")) ?? ""),
    inputs: { "course-form-field": "Parts of a whole, written as a fraction." },
    avoid: [...EDITOR_AVOID, "course-lessons-json", "course-lessons-format", "course-form-remove", "course-form-add"],
    maxSteps: 12,
  },
  {
    id: "add-lesson-to-unit",
    login: "keys=fake",
    start: "/courses",
    setup: ownCourse,
    goal: 'Add a new lesson called "Mixed numbers" to the Operations unit by hand (no AI), and make sure it is saved (the editor saves by itself).',
    done: "The circle by the course title is green (all changes saved) and the Undo line names one lesson added.",
    check: async (page) =>
      (await saved(page)) &&
      /1 added/.test((await page.getByTestId("course-undo-label").textContent().catch(() => "")) ?? "") &&
      (await page.locator('[data-testid="course-lessons-row"]', { hasText: "Mixed numbers" }).getAttribute("data-unit")) ===
        (await page.locator('[data-testid="course-lessons-unit"]', { hasText: "Operations" }).getAttribute("data-unit")),
    inputs: { "course-lessons-rename": "Mixed numbers" },
    avoid: [...EDITOR_AVOID, "course-ai-submit", "course-lessons-grip"],
    maxSteps: 10,
  },
  {
    id: "name-new-course",
    start: "/courses",
    // Create a course opens the editor with the generate dialog (#144): closing it is part of the walk.
    goal: "Create a new course of your own without having the AI write it, and give it the title \"Music theory\" (the editor saves by itself; there is no Save button).",
    done: "The course editor shows the title Music theory, its circle is green (saved) and the Undo line says the course was renamed.",
    check: async (page) =>
      (await saved(page)) &&
      /Renamed the course/.test((await page.getByTestId("course-undo-label").textContent().catch(() => "")) ?? "") &&
      (await page.getByTestId("course-edit-title").first().textContent())?.trim() === "Music theory",
    inputs: { "course-edit-rename-input": "Music theory" },
    avoid: [...EDITOR_AVOID, "generate-submit", "course-ai-submit"],
    maxSteps: 10,
  },
  {
    id: "turn-off-ai-features",
    start: "/",
    // A fresh account, so the dev user the other journeys share keeps its AI features.
    setup: async (page) => {
      // Signed out first: /register shows no form to a signed-in user.
      await page.context().clearCookies();
      await register(page, newAccount("journey-ai"), "spanish");
      return "/";
    },
    goal: "You would rather not see any AI features on this site. Find the setting that hides them and turn it on.",
    done: "The profile says AI features are hidden.",
    check: async (page) => (await page.getByTestId("consent-ai").getAttribute("data-ai-off").catch(() => null)) === "true",
    maxSteps: 8,
  },
  {
    id: "start-at-a2",
    start: "/courses",
    // A fresh account: the level is the last of the wizard's four steps behind Start (#587,
    // #1106), and "Get Started!" sits under every step (#896): Start learning, three arrows,
    // A2, Get Started!. Keep the budget at 8; it catches the wizard growing again.
    setup: async (page) => {
      await page.context().clearCookies();
      const a = newAccount("journey-level");
      await signUp(page, a);
      return "/courses";
    },
    goal: "You already know beginner Spanish. Start the Spanish course at A2 instead of the very beginning.",
    done: "The course is open and the lessons page marks the A1 lessons Skipped.",
    check: async (page) => {
      const r = await page.request.get("/api/progress");
      return r.ok() && (await r.json()).start_unit === "A2";
    },
    maxSteps: 8,
  },
  {
    id: "change-level",
    start: "/",
    // Started at the beginning; the level is changed later (#333).
    setup: async (page) => {
      await page.context().clearCookies();
      await register(page, newAccount("journey-change-level"), "spanish");
      return "/";
    },
    goal: "You started the course at the beginning, but it is too easy. Move to level A2 without losing your progress.",
    done: "The course home says you are in unit A2.",
    check: async (page) => {
      const r = await page.request.get("/api/progress");
      return r.ok() && (await r.json()).start_unit === "A2";
    },
    maxSteps: 8,
  },
];
