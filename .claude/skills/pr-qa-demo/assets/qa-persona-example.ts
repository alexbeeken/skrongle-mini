// Example throwaway QA persona, from PR #644's run (server: ./.dev-start --mini).
// Copy to e2e/personas/_qa-pr<N>.ts, one goal per checklist step; never commit the copy.
// `check(step, note, fn)` records one exact check per line in $QA_OUT for the report.
import { expect, type Page } from "@playwright/test";
import { appendFileSync } from "node:fs";
import type { Persona } from "../lib/persona";
import { logout, signUp, apiProgress, openCourse } from "../lib/learner";
import { playStage } from "../lib/answer";
import { AnswerKey } from "../lib/key";

const out = process.env.QA_OUT || "/dev/null";
const rec = (step: number, ok: boolean, note: string) => appendFileSync(out, JSON.stringify({ step, ok, note }) + "\n");
async function check(step: number, note: string, fn: () => Promise<unknown>) {
  try { await fn(); rec(step, true, note); } catch (e) { rec(step, false, `${note}: ${String(e).split("\n")[0]}`); }
}
const EMAIL = `qa644-${Date.now()}@example.org`;
const visible = (page: Page, id: string) => page.getByTestId(id).first().isVisible();

export default {
  id: "qa-pr644",
  who: "A person running Skrongle on a small home server with nothing set up: no e-mail, no text messages, no payments. They sign up and start learning Spanish.",
  prefers: "Go straight to the page the goal names; use the main navigation.",
  patience: 14,
  viewports: [process.env.QA_VP === "phone" ? "phone" : "desktop"],
  goals: [
    {
      goal: "Look at the home page.",
      done: "The page is the home page.",
      start: "/",
      check: async () => true,
      then: async (page, look) => {
        await logout(page);
        await signUp(page, { email: EMAIL, password: "qa-pr644-password" });
        await look("signed in after registering");
        await check(4, "registered and signed in as the new address", async () =>
          expect(page.getByTestId("nav-account-toggle").first()).toHaveAttribute("data-email", EMAIL));
      },
    },
    {
      goal: "Look at the course list.",
      done: "The page lists courses.",
      start: "/courses",
      check: async () => true,
      then: async (page, look) => {
        await openCourse(page, "spanish");
        await look("Spanish course home");
        await check(4, `on the course home (${new URL(page.url()).pathname}), Guide card visible`, () => expect(page.getByTestId("home-guide-open").first()).toBeVisible());
        await check(4, "no error alert on the course home", () => expect(page.locator('[role="alert"]:visible')).toHaveCount(0));
      },
    },
    {
      goal: "Open the Guide and start the first lesson's Play stage.",
      done: "A Guide stage with a step (a flashcard or a question) is showing.",
      start: "/guide",
      check: (page) => page.locator('[data-testid="stage-step"]:visible').first().isVisible(),
      then: async (page, look) => {
        const [first] = await apiProgress(page);
        rec(5, true, `INFO stage url ${page.url()}, first lesson ${first.slug}`);
        if (!new URL(page.url()).pathname.endsWith(first.slug)) await page.goto(`/guide/${first.slug}`);
        const [right, total] = await playStage(page, new AnswerKey("spanish"));
        await look("stage done");
        await check(5, `every quick check right (${right}/${total})`, async () => expect(right).toBe(total));
        await page.goto("/lessons");
        const item = page.locator(`[data-testid="lessons-item"][data-lesson-id="${first.slug}"]`);
        await item.scrollIntoViewIfNeeded();
        await look("lessons list after the stage");
        await check(5, `${first.slug} shows passed on /lessons`, () => expect(item).toHaveAttribute("data-passed", "true"));
        const mail = await (await page.request.get("/dev/inbox?format=json")).json();
        await check(5, `no mail anywhere: /dev/inbox messages=${mail.messages.length}, smtp=${mail.smtp}`, async () => { expect(mail.messages).toEqual([]); expect(mail.smtp).toBeNull(); });
        const phone = await (await page.request.get("/dev/phone?format=json")).json();
        await check(5, `no dev phone: phone=${phone.phone}`, async () => expect(phone.phone).toBe(false));
      },
    },
  ],
  concerns: ["The page asks for an e-mail confirmation that can never arrive.", "The page mentions payments or a subscription that cannot work."],
  frustrations: ["The page says an e-mail or a text was sent."],
  checks: ["labels", "error-role"],
} satisfies Persona;
