// Course files -> passages: the stretches of text a provenance check judges
// whole. Coarser than the fact check's units on purpose: copying shows in
// wording that runs on, and in which items were picked in what order, not in
// a single word. One passage per content section, per conversation, and per
// kind of exercise list (vocabulary, quiz, listening, builds…). Passages
// under MIN_WORDS words are generic ("Me llamo Ana"), never
// judged alone, and counted as such.
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";

type Json = Record<string, any>;

export const MIN_WORDS = 12;
export const WORD = /[\p{L}\p{M}\p{N}]+(?:['’][\p{L}]+)?/gu;

export interface Passage {
  course: string;
  /** Repo-relative path. */
  file: string;
  /** Where in the file: `content § Heading`, `conversations[0]`, `dmode.builds`. */
  at: string;
  text: string;
  /** Course and lesson, the context given to the judge. */
  ctx: Json;
}

export interface Course {
  id: string;
  dir: string;
  title: string;
  passages: Passage[];
  /** Passages too short to judge alone. */
  generic: number;
}

const arr = (v: unknown): Json[] => (Array.isArray(v) ? v : []);
export const words = (s: string): string[] => (s.match(WORD) ?? []).map((w) => w.toLowerCase());

function jsonFiles(dir: string): string[] {
  if (!existsSync(dir)) return [];
  return readdirSync(dir).sort().flatMap((n) => {
    const p = join(dir, n);
    return statSync(p).isDirectory() ? jsonFiles(p) : n.endsWith(".json") ? [p] : [];
  });
}

/** `## Heading` sections of Markdown, each with its heading. */
function sections(md: string): { title: string; text: string }[] {
  const out: { title: string; text: string }[] = [];
  let title = "Introduction", buf: string[] = [];
  const flush = () => {
    const text = buf.join("\n").trim();
    if (text) out.push({ title, text });
    buf = [];
  };
  for (const line of md.split("\n")) {
    if (line.startsWith("## ")) { flush(); title = line.slice(3).trim(); } else buf.push(line);
  }
  flush();
  return out;
}

const pair = (a: unknown, b: unknown) => [a, b].filter((x) => typeof x === "string" && x).join(" — ");

export function loadCourse(repo: string, dir: string, lessonFilter = ""): Course {
  const id = dir.split("/").pop()!;
  const m: Json = JSON.parse(readFileSync(join(dir, "course.json"), "utf8"));
  const passages: Passage[] = [];
  let generic = 0;
  for (const f of jsonFiles(join(dir, "lessons"))) {
    const file = relative(repo, f);
    if (lessonFilter && !file.includes(lessonFilter)) continue;
    let l: Json;
    try {
      l = JSON.parse(readFileSync(f, "utf8"));
    } catch (e) {
      throw new Error(`${file}: not valid JSON (${(e as Error).message}); cargo test names the problem`);
    }
    const ctx: Json = { course: m.title, lesson: l.title, focus: l.focus ?? l.target_grammar_point };
    const push = (at: string, text: string, c: Json = ctx) => {
      text = text.trim();
      if (words(text).length < MIN_WORDS) { generic++; return; }
      passages.push({ course: id, file, at, text, ctx: c });
    };
    block(l, "", ctx, push);
  }
  return { id, dir, title: m.title, passages, generic };
}

function block(l: Json, p: string, ctx: Json, push: (at: string, text: string, c?: Json) => void) {
  const md = l.content ?? l.markdown_content;
  if (typeof md === "string") for (const s of sections(md)) push(`${p}content § ${s.title}`, s.text, { ...ctx, section: s.title });
  arr(l.conversations).forEach((c, i) =>
    push(`${p}conversations[${i}]${c.id ? " " + c.id : ""}`, arr(c.lines).map((x) => pair(x.text ?? x.es, x.meaning ?? x.en)).join("\n")),
  );
  push(`${p}vocabulary`, arr(l.vocabulary).map((v) => pair(pair(v.term ?? v.es, v.meaning ?? v.en), pair(v.example ?? v.example_es, v.example_meaning ?? v.example_en))).join("\n"));
  push(`${p}quiz`, arr(l.quiz).map((q) => [q.prompt, ...(Array.isArray(q.options) ? q.options : []), "→ " + q.correct_answer, q.explanation].filter(Boolean).join(" | ")).join("\n"));
  push(`${p}listening`, arr(l.listening).map((s) => pair(s.text, s.meaning)).join("\n"));
  const d: Json = l.dmode ?? {};
  push(`${p}bites`, arr(d.bites).map((b) => [b.title, b.rule, pair(b.example ?? b.example_es, b.example_meaning ?? b.example_en)].filter(Boolean).join(" | ")).join("\n"));
  push(`${p}sorts`, arr(d.sorts).map((s) => `${s.title}: ${arr(s.items).map((i) => `${i.text} → ${s.buckets?.[i.bucket]}`).join("; ")}`).join("\n"));
  push(`${p}spots`, arr(d.spots).map((s) => pair(s.sentence, s.why)).join("\n"));
  push(`${p}builds`, arr(d.builds).map((b) => pair(b.text ?? b.es, b.meaning ?? b.en)).join("\n"));
  arr(l.guide ?? l.journey).forEach((g, i) => g.concept && push(`${p}guide[${i}] concept`, `${g.concept.title}\n${g.concept.markdown}`));
}
