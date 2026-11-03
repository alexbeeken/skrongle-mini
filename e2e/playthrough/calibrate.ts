// How well does Jev answer a course's questions, asked the way the player
// asks them? Each lesson's quiz questions, typed ones through the player's own
// `compose` and multiple-choice ones as a Choice with the same state a step
// gets, scored against the lesson files. The answer key is used only here, to
// score; the player never sees it. Run it after changing how the player asks
// Jev (player.ts) or JEV_MODEL, and compare: scripts/jev-playthrough.sh calibrate.
import { chromium } from "@playwright/test";
import { readdirSync, readFileSync, statSync, writeFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { choose, costUSD, jevEnabled, MODEL, SKIP_REASON, usage } from "../lib/jev.ts";
import { compose, readLesson } from "./player.ts";

const REPO = fileURLToPath(new URL("../..", import.meta.url));
const a = process.argv.slice(2);
const arg = (name: string, def: string) => (a.includes(name) ? a[a.indexOf(name) + 1] : def);
const course = arg("--course", "spanish");
const count = Number(arg("--lessons", "12"));
const base = (process.env.PLAYTHROUGH_URL ?? "").replace(/\/$/, "");
if (!jevEnabled) throw new Error(SKIP_REASON);
if (!base) throw new Error("no server: run scripts/jev-playthrough.sh calibrate");

interface Q { slug: string; title: string; unit: string; prompt: string; type: string; options: string[]; answers: string[] }
const files = new Map<string, string>();
const walk = (dir: string) => {
  for (const n of readdirSync(dir)) {
    const p = join(dir, n);
    if (statSync(p).isDirectory()) walk(p);
    else if (n.endsWith(".json")) files.set(JSON.parse(readFileSync(p, "utf8")).slug, p);
  }
};
walk(join(REPO, "data", "courses", course, "lessons"));

const browser = await chromium.launch();
const page = await browser.newPage({ baseURL: base });
await page.goto(`/dev/login?open=${course}`);
const lessons = (await (await page.request.get("/api/progress")).json()).lessons.slice(0, count);
// Same normalisation as a lenient grader: case and punctuation, not accents.
const norm = (s: string) => s.toLowerCase().replace(/[¿¡.,;:!?"'«»]/g, "").replace(/\s+/g, " ").trim();

const rows: { slug: string; type: string; prompt: string; got: string; want: string; ok: boolean; confidence: number }[] = [];
for (const l of lessons) {
  const lesson = await readLesson(page, `/lessons/${l.slug}`);
  const quiz = JSON.parse(readFileSync(files.get(l.slug)!, "utf8")).quiz ?? [];
  const goal = "Practise this lesson: answer every question right.";
  await Promise.all(
    quiz.map(async (q: any) => {
      const question = `${l.title} · ${l.unit}\n${q.prompt}`;
      const answers = [q.correct_answer, ...(q.accepted_answers ?? [])];
      let got = "", confidence = 0;
      if (q.type === "multiple-choice") {
        const options = Object.fromEntries((q.options as string[]).map((o, i) => [`a${i}`, `Choose "${o}"`]));
        const res = await choose(
          { goal, lesson_you_read: lesson.text, page_text: `${question}\n${q.options.join("\n")}\nCheck` },
          "You are a learner doing this lesson and you want every answer right. Choose the answer to the question.",
          options,
        );
        got = q.options[Number(res.choice.slice(1))] ?? "";
        confidence = res.confidence;
      } else {
        let conf: number[] = [];
        got = await compose({ question, box: "Your answer", lang: lesson.lang }, goal, lesson, (t) => conf.push(t.confidence));
        confidence = Math.min(...conf);
      }
      rows.push({ slug: l.slug, type: q.type === "multiple-choice" ? "choice" : "typed", prompt: q.prompt, got, want: answers[0], ok: answers.some((x: string) => norm(x) === norm(got)), confidence: Math.round(confidence * 100) / 100 });
    }),
  );
}
await browser.close();

const pct = (xs: typeof rows) => (xs.length ? Math.round((100 * xs.filter((r) => r.ok).length) / xs.length) : 0);
const typed = rows.filter((r) => r.type === "typed"), choice = rows.filter((r) => r.type === "choice");
console.log(rows.filter((r) => !r.ok).map((r) => `✗ ${r.type.padEnd(6)} ${r.slug.slice(0, 26).padEnd(26)} ${r.prompt.slice(0, 60)} → "${r.got}" (want "${r.want}", conf ${r.confidence})`).join("\n"));
console.log(`==> ${MODEL} on ${lessons.length} lessons of ${course}: typed ${typed.filter((r) => r.ok).length}/${typed.length} (${pct(typed)} %), choice ${choice.filter((r) => r.ok).length}/${choice.length} (${pct(choice)} %)`);
console.log(`==> Jev ${usage.calls} calls ≈ $${costUSD().toFixed(4)}`);
const out = join(REPO, "e2e", "results", "playthrough", "calibrate.json");
mkdirSync(join(REPO, "e2e", "results", "playthrough"), { recursive: true });
writeFileSync(out, JSON.stringify({ model: MODEL, course, lessons: lessons.length, typed_pct: pct(typed), choice_pct: pct(choice), rows }, null, 2));
console.log(`==> e2e/results/playthrough/calibrate.json`);
