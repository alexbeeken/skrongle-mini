// The converter on a hand-written trace, no browser: node --test e2e/record/
// (or `npm --prefix e2e run unit`).
import assert from "node:assert/strict";
import { test } from "node:test";
import { convert, diffExpects, locatorFor, type RecEvent, type Trace } from "./convert.ts";

const ev = (e: Partial<RecEvent>): RecEvent => ({ type: "click", tag: "a", ownTestid: "", testid: "", nth: 0, count: 1, ...e });

const trace: Trace = {
  id: "first-lesson",
  goal: "Read the first lesson and pass its reading check",
  start: "/register",
  course: "spanish",
  date: "2026-10-16",
  model: "jev-1.13.0",
  seed: 1,
  reached: true,
  reason: "Jev judged the goal reached (p=0.9)",
  jev: { calls: 5, usd: 0.0003 },
  steps: [
    { n: 0, url: "/register", state: {}, action: 'Type "rec-1@example.com" into …', events: [ev({ type: "fill", tag: "input", ownTestid: "register-email", value: "rec-1@example.com" })] },
    { n: 1, url: "/courses", state: {}, action: "Click …", events: [ev({ tag: "button", ownTestid: "courses-open" })] },
    {
      n: 2,
      url: "/study",
      state: { "nav-study": {} },
      action: 'Click the link "Lessons" [nav-study-lessons]',
      events: [ev({ ownTestid: "nav-study-lessons" })],
    },
    {
      n: 3,
      url: "/lessons",
      state: { "lessons-item": { passed: "false", read: "false", "lesson-id": "greetings" } },
      action: 'Click the link "Greetings" [lessons-item-link]',
      events: [ev({ ownTestid: "lessons-item-link", count: 30, nth: 0, lessonId: "greetings" })],
    },
    {
      n: 4,
      url: "/lessons/greetings",
      state: { lesson: { read: "false", passed: "false" }, "lesson-check": { done: "false" } },
      auto: "check",
      action: 'Click the button "Check my answers" [lesson-check-submit]',
      events: [
        // Jev picked an option too: it must become answerQuestion, not the literal.
        ev({ type: "check", tag: "input", inputType: "radio", testid: "lesson-check-option", ownTestid: "lesson-check-option", qid: "greet-01", count: 3, nth: 2 }),
        ev({ tag: "button", ownTestid: "lesson-check-submit" }),
      ],
    },
  ],
  final: { url: "/lessons/greetings", state: { lesson: { read: "true", passed: "false" }, "lesson-check": { done: "false" }, "lesson-check-result": { state: "passed" } } },
};

test("drops the registration steps and starts with a fresh account", () => {
  const spec = convert(trace);
  assert.match(spec, /await register\(page, newAccount\("first-lesson"\), COURSE\);/);
  assert.doesNotMatch(spec, /register-email/);
  assert.doesNotMatch(spec, /courses-open/);
  assert.match(spec, /await page\.goto\("\/study"\);/);
});

test("navigation is by testid, answers come from the key, never the literal", () => {
  const spec = convert(trace);
  assert.match(spec, /page\.locator\("\[data-testid=\\"nav-study-lessons\\"\]:visible"\)\.first\(\)\.click\(\)/);
  assert.match(spec, /await passReadingCheck\(page, key\);/);
  assert.doesNotMatch(spec, /greet-01|check\(\)/); // Jev's pick on the check is covered by passReadingCheck
  // passReadingCheck submits, so Jev's submit click is not repeated.
  assert.doesNotMatch(spec, /locator\(.*lesson-check-submit/);
  assert.match(spec, /^\/\/ DRAFT: review before commit/);
  assert.match(spec, /seed 1/);
});

test("expects the state that changed, and the page moving on", () => {
  const spec = convert(trace);
  assert.match(spec, /toHaveAttribute\("data-read", "true"\)/);
  assert.match(spec, /lesson-check-result.*toHaveAttribute\("data-state", "passed"\)/);
  assert.doesNotMatch(spec, /data-lesson-id", /); // content ids are not state
  assert.match(spec, /toHaveURL\(\/\\\/lessons\(\\\/\|\\\?\|\$\)\/\)/);
});

test("locators: a lesson's link by its lesson, a testid-less element flagged", () => {
  assert.match(locatorFor(ev({ ownTestid: "lessons-item-link", count: 30, nth: 4, lessonId: "ser-vs-estar" })), /data-lesson-id=\\"ser-vs-estar\\"/);
  assert.match(locatorFor(ev({ tag: "button", testid: "quiz", nth: 1, count: 2 })), /\[data-testid=\\"quiz\\"\]:visible"\)\.first\(\)\.locator\("button:visible"\)\.nth\(1\)/);
  assert.match(locatorFor(ev({ tag: "button" })), /REVIEW: no testid/);
});

test("Sprint mode taps become one playSprintRight", () => {
  const t: Trace = {
    ...trace,
    start: "/sprint",
    steps: [{ n: 0, url: "/sprint/play?lesson=x", state: {}, events: [ev({ tag: "button", testid: "sprint-play-stage", exercise: "sort:x:0" }), ev({ tag: "button", testid: "sprint-play-stage", exercise: "sort:x:0" }), ev({ tag: "button", ownTestid: "sprint-play-check" })] }],
  };
  const spec = convert(t);
  assert.equal(spec.match(/playSprintRight\(page, key\)/g)?.length, 1);
  assert.match(spec, /import \{ playSprintRight \} from "..\/..\/lib\/sprint-play.ts";/);
  assert.equal(diffExpects({ url: "/a", state: {} }, { url: "/a", state: {} }).length, 0);
});
