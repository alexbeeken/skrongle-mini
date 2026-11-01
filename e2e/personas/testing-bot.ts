// Profile "testing-bot" (see e2e/lib/persona.ts): sees only markup; its exact checks are testability findings.
import type { Persona } from "../lib/persona";

export default {
  id: "testing-bot",
  who: "An automated tester with no human intuition. It sees only what the markup says: testids, data- state, roles and visible labels.",
  prefers: "Prefer links and buttons with a testid and a clear label; prefer pages not visited yet; avoid anything whose effect the label doesn't say.",
  patience: 12,
  viewports: ["desktop"],
  goals: [
    {
      goal: "Visit as many different pages as you can from the navigation and the home page, one after another, without repeating a page.",
      done: "Every page of the app has been visited.",
      // Never reached: the bot walks until its patience runs out.
      check: async () => false,
    },
  ],
  concerns: ["Every action in `actions` has a label that says what it does."],
  frustrations: ["Some action in `actions` has a label made only of a symbol or an icon."],
  checks: ["labels", "class-state", "hidden-menus", "error-role", "overflow"],
} satisfies Persona;
