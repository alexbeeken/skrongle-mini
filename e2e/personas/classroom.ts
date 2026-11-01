// Profile "classroom" (see e2e/lib/persona.ts): brings a topic from class.
import type { Persona } from "../lib/persona";

/** Topics a student might bring from class; one goal each. "Numbers over 100" was a coverage finding until #491 added the A2 lesson. */
export const CLASS_TOPICS = ["ser vs estar", "preterite vs imperfect", "gustar", "por vs para", "numbers over 100"];

export default {
  id: "classroom",
  who: "A student taking a Spanish class who opens the app when something from class is confusing, wants that topic right now, and switches between modes freely.",
  prefers: "Prefer the lesson list, the map and the tutor; prefer anything that names the topic; prefer practising one topic over 'recommended'.",
  patience: 10,
  viewports: ["phone", "desktop"],
  goals: CLASS_TOPICS.map((topic) => ({
    goal: `Find the lesson that teaches "${topic}" (the \`topic\`), and open it.`,
    done: "The page is a lesson that teaches the `topic`.",
    topic,
    // The lessons list has a filter box: the student types the topic there.
    inputs: { "lessons-filter": topic },
  })),
  concerns: [
    { text: "The page is about the `topic`.", on: /^\/lessons\/./ },
    { text: "There is a way to practise just this lesson from here.", on: /^\/lessons\/./ },
  ],
  frustrations: ["The page only offers the recommended next lesson, with no way to pick a topic."],
} satisfies Persona;
