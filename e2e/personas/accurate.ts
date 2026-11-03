// Profile "accurate" (see e2e/lib/persona.ts): reads everything, audits the facts.
import type { Page } from "@playwright/test";
import type { Persona } from "../lib/persona";

const visible = (page: Page, testid: string) => page.getByTestId(testid).first().isVisible();

export default {
  id: "accurate",
  who: "A serious adult learner who studies each lesson in depth in Study mode, reads every explanation, checks the answers themselves and loses trust at the first wrong fact.",
  prefers: "Prefer lessons, vocabulary tables, conjugation tables and explanations; read a lesson to the end before testing; avoid games.",
  patience: 20,
  viewports: ["desktop"],
  goals: [
    {
      goal: "Open a lesson that you haven't read yet and read it to the end, including its vocabulary table.",
      done: "The page shows a single lesson's teaching content.",
      check: async (page) => (await visible(page, "lesson")) && (await visible(page, "lesson-vocabulary")),
    },
    {
      goal: "Practise a lesson's questions and read the explanation that comes after an answer.",
      done: "The page shows feedback on an answer, with an explanation.",
      check: async (page) => (await visible(page, "quiz-feedback")) || (await visible(page, "stage-feedback")),
      inputs: { "*": "no sé" },
    },
    {
      goal: "Browse the vocabulary list of the course.",
      done: "The page is a list of Spanish words with their meanings.",
      check: (page) => visible(page, "vocabulary-term"),
    },
  ],
  concerns: [
    "The Spanish on the page uses accents and ¿ ¡ correctly.",
    { text: "The feedback explains why the answer is right or wrong.", when: "quiz-feedback" },
    { text: "The feedback explains why the answer is right or wrong.", when: "stage-feedback" },
  ],
  frustrations: [
    "The page says a correct answer is wrong.",
    "The page contradicts itself about a Spanish rule.",
  ],
  items: [
    { testid: "lesson-vocab-item", cells: ["spanish", "english", "example"], statement: "In `item`, `english` is a correct English meaning of the Spanish word `spanish`." },
    { testid: "vocabulary-term", cells: ["spanish", "english", "example"], statement: "In `item`, `english` is a correct English meaning of the Spanish word `spanish`." },
  ],
} satisfies Persona;
