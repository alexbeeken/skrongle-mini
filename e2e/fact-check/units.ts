// Course files -> units: the smallest things worth one verdict each. A
// sentence with its translation, one line of a lesson section (a table row,
// a list item, a paragraph), a quiz question, one item of a sort, a spot, a
// conjugation table, a vocabulary entry. Small units are what make a finding
// point at one line instead of "something in this section".
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";

type Json = Record<string, any>;

/** What the course teaches, as the questions need it. */
export interface Lang {
  /** A language course: terms in one language, meanings in another. */
  lang: boolean;
  /** "Spanish", or the subject ("Knots", "Music theory"). */
  L: string;
  /** "English", or "definition". */
  M: string;
  /** BCP 47 of the terms ("es"), for the language's confusable words. */
  code: string;
}

interface Base {
  course: string;
  /** Repo-relative path. */
  file: string;
  /** Where in the file: `listening[3]`, `quiz[0] <id>`, `content § Heading › row 3`. */
  at: string;
  /** Course and lesson, put at the top of every state. */
  ctx: Json;
}

export type Unit = Base &
  (
    | { kind: "sentence"; text: string; meaning?: string; note?: string }
    | { kind: "line"; text: string; section?: string; header?: string; about?: Json }
    | { kind: "mc"; prompt: string; options: string[]; answer: string; explain?: string }
    | { kind: "fill"; prompt: string; answer: string; accepted: string[]; explain?: string }
    | { kind: "sort"; title: string; buckets: string[]; text: string; bucket: number; why?: string }
    | { kind: "spot"; sentence: string; wrong: string; fix: string; why?: string }
    | { kind: "conj"; item: string; meaning?: string; set: string; forms: Record<string, string> }
    | { kind: "vocab"; term: string; meaning: string; others: string[] }
    | { kind: "facts"; data: Json; statements: Record<string, string> }
  );

/** A loaded course: its units, its language, and what repairs can draw on. */
export interface Course {
  id: string;
  dir: string;
  lang: Lang;
  units: Unit[];
  /** Every word in the course's files, with how often it occurs: repairs must be words the course uses. */
  lexicon: Map<string, number>;
  /** Each conjugated form -> every form of the same item (all sets), from the course's tables. */
  siblings: Map<string, Set<string>>;
  /** Conjugation tables by item and set, slot keys as in the files. */
  tables: { item: string; set: string; forms: Record<string, string> }[];
}

export const WORD = /[\p{L}\p{M}]+(?:['’][\p{L}]+)?/gu;
const arr = (v: unknown): Json[] => (Array.isArray(v) ? v : []);

function jsonFiles(dir: string): string[] {
  if (!existsSync(dir)) return [];
  return readdirSync(dir)
    .sort()
    .flatMap((n) => {
      const p = join(dir, n);
      return statSync(p).isDirectory() ? jsonFiles(p) : n.endsWith(".json") ? [p] : [];
    });
}

/** Split Markdown into lines worth judging alone, each with its section (and table header). */
export function markdownLines(md: string): { text: string; section: string; header?: string; n: number }[] {
  const out: { text: string; section: string; header?: string; n: number }[] = [];
  let section = "Introduction", header: string | undefined, fence = false, n = 0;
  for (const raw of md.split("\n")) {
    const s = raw.trim();
    if (s.startsWith("```")) { fence = !fence; continue; }
    if (fence || !s) { if (!s) header = undefined; continue; }
    if (s.startsWith("## ")) { section = s.slice(3).trim(); header = undefined; n = 0; continue; }
    if (s.startsWith("#")) continue;
    if (/^\|[\s|:-]+\|$/.test(s)) continue; // table separator
    if (s.startsWith("|")) {
      if (header === undefined) { header = s; continue; }
      out.push({ text: s, section, header, n: ++n });
      continue;
    }
    header = undefined;
    out.push({ text: s.replace(/^>\s?/, ""), section, n: ++n });
  }
  return out;
}

export function loadCourse(repo: string, dir: string, lessonFilter: string): Course {
  const id = dir.split("/").pop()!;
  const m: Json = JSON.parse(readFileSync(join(dir, "course.json"), "utf8"));
  const isLang = !!m.terms?.lang && m.terms?.lang !== m.meanings?.lang;
  const lang: Lang = { lang: isLang, L: m.subject ?? m.title, M: isLang ? (m.meanings?.label ?? "English") : "definition", code: m.terms?.lang ?? "" };
  const units: Unit[] = [];
  const lexicon = new Map<string, number>();
  const siblings = new Map<string, Set<string>>();
  const tables: Course["tables"] = [];
  const slots = new Map<string, string>(arr(m.paradigm?.slots).map((s) => [s.key, s.label]));
  const sets = new Map<string, string>(arr(m.paradigm?.sets).map((s) => [s.key, s.label]));
  const words = (v: unknown): void => {
    if (typeof v === "string") for (const w of v.match(WORD) ?? []) lexicon.set(w.toLowerCase(), (lexicon.get(w.toLowerCase()) ?? 0) + 1);
    else if (v && typeof v === "object") Object.values(v).forEach(words);
  };
  words(m);

  const manifest = relative(repo, join(dir, "course.json"));
  if (m.calendar && !lessonFilter)
    units.push({
      course: id, file: manifest, at: "calendar", ctx: { course: m.title }, kind: "facts",
      data: { language: lang.L, ...m.calendar },
      statements: {
        days: `"days" are the seven days of the week in ${lang.L}, Monday first, correctly spelled.`,
        months: `"months" are the twelve months in ${lang.L}, January first, correctly spelled.`,
        format: `"format" is how a full date is usually written in ${lang.L}.`,
      },
    });

  for (const f of jsonFiles(join(dir, "lessons"))) {
    let l: Json;
    try {
      l = JSON.parse(readFileSync(f, "utf8"));
    } catch (e) {
      throw new Error(`${relative(repo, f)}: not valid JSON (${(e as Error).message}); cargo test names the problem`);
    }
    words(l);
    for (const c of arr(l.conjugations)) {
      const forms: Record<string, string> = c.forms ?? {};
      tables.push({ item: c.item ?? c.verb, set: c.set ?? c.tense, forms });
    }
    const file = relative(repo, f);
    if (lessonFilter && !file.includes(lessonFilter)) continue;
    const ctx: Json = { course: lang.lang ? `${lang.L} for ${lang.M} speakers` : lang.L, lesson: l.title, focus: l.focus ?? l.target_grammar_point };
    const push = (u: Omit<Base, "course" | "file" | "ctx"> & { ctx?: Json } & Record<string, any>) => units.push({ course: id, file, ctx, ...u } as Unit);
    lessonUnits(l, "", ctx, push, lang, slots, sets);
  }
  for (const t of tables) {
    const all = Object.values(t.forms).filter((v) => !v.includes(" "));
    for (const v of all) {
      const s = siblings.get(v.toLowerCase()) ?? new Set<string>();
      for (const o of tables.filter((x) => x.item === t.item).flatMap((x) => Object.values(x.forms))) if (!o.includes(" ")) s.add(o);
      siblings.set(v.toLowerCase(), s);
    }
  }
  return { id, dir, lang, units, lexicon, siblings, tables };

  function lessonUnits(l: Json, p: string, ctx: Json, push: (u: any) => void, lang: Lang, slots: Map<string, string>, sets: Map<string, string>) {
    const sentence = (at: string, text: unknown, meaning: unknown, note?: unknown) => {
      if (typeof text !== "string" || !text.trim()) return;
      // In a course that is not about a language, a "sentence" is a claim like any other line.
      if (!lang.lang) push({ at, kind: "line", text, about: meaning ? { meaning } : undefined });
      else push({ at, kind: "sentence", text, meaning: typeof meaning === "string" ? meaning : undefined, note: typeof note === "string" ? note : undefined });
    };
    const lines = (at: string, md: unknown) => {
      if (typeof md !== "string") return;
      for (const x of markdownLines(md))
        push({ at: `${at} § ${x.section} › ${x.header ? "row" : "line"} ${x.n}`, kind: "line", text: x.text, section: x.section, header: x.header });
    };
    lines(`${p}content`, l.content ?? l.markdown_content);
    const vocab = arr(l.vocabulary);
    vocab.forEach((v, i) => {
      const term = v.term ?? v.es, meaning = v.meaning ?? v.en;
      if (term && meaning)
        push({ at: `${p}vocabulary[${i}]`, kind: "vocab", term, meaning, others: vocab.filter((o) => o !== v).map((o) => o.meaning ?? o.en).filter(Boolean).slice(0, 20) });
      const ex = v.example ?? v.example_es;
      if (ex) sentence(`${p}vocabulary[${i}] example`, ex, v.example_meaning ?? v.example_en);
    });
    arr(l.quiz).forEach((q, i) => {
      const at = `${p}quiz[${i}]${q.question_id ? " " + q.question_id : ""}`;
      const type = q.type ?? "multiple-choice";
      // The author's own reasoning: checking the key against it is what makes a finding trustworthy.
      const explain = [q.explanation, ...arr(q.hints)].filter((x) => typeof x === "string" && x).join(" ") || undefined;
      if (type === "multiple-choice" && arr(q.options).length > 1) push({ at, kind: "mc", prompt: q.prompt, options: q.options, answer: q.correct_answer, explain });
      else if (type === "true-false") push({ at, kind: "mc", prompt: `True or false? ${q.prompt}`, options: ["true", "false"], answer: q.correct_answer, explain });
      else if (type === "fill-in-blank" && String(q.prompt).includes("___")) push({ at, kind: "fill", prompt: q.prompt, answer: q.correct_answer, accepted: arr(q.accepted_answers), explain });
      else push({ at, kind: "line", text: `${q.prompt} → ${q.correct_answer}`, about: { question: q.prompt, answer: q.correct_answer, type } });
      if (q.explanation) push({ at: `${at} explanation`, kind: "line", text: q.explanation, about: { question: q.prompt, answer: q.correct_answer } });
    });
    arr(l.listening).forEach((s, i) => sentence(`${p}listening[${i}]`, s.text, s.meaning, s.note));
    arr(l.conversations).forEach((c, i) =>
      arr(c.lines).forEach((x, j) => sentence(`${p}conversations[${i}] ${c.id ?? ""} line ${j}`.replace("  ", " "), x.text ?? x.es, x.meaning ?? x.en, x.note)),
    );
    const d: Json = l.sprint ?? {};
    const dp = p ? p : "sprint.";
    arr(d.bites).forEach((b, i) => {
      if (b.rule) push({ at: `${dp}bites[${i}] rule`, kind: "line", text: b.rule, section: b.title });
      const ex = b.example ?? b.example_es;
      if (ex) sentence(`${dp}bites[${i}] example`, ex, b.example_meaning ?? b.example_en);
    });
    arr(d.sorts).forEach((s, i) =>
      arr(s.items).forEach((it, j) => push({ at: `${dp}sorts[${i}] item ${j}`, kind: "sort", title: s.title, buckets: s.buckets, text: it.text, bucket: it.bucket, why: s.why })),
    );
    arr(d.spots).forEach((s, i) => push({ at: `${dp}spots[${i}]`, kind: "spot", sentence: s.sentence, wrong: s.wrong, fix: s.fix, why: s.why }));
    arr(d.builds).forEach((b, i) => sentence(`${dp}builds[${i}]`, b.text ?? b.es, b.meaning ?? b.en));
    arr(l.guide ?? l.journey).forEach((g, i) => {
      if (g.tip) push({ at: `${p}guide[${i}] tip`, kind: "line", text: g.tip });
      if (g.concept) lines(`${p}guide[${i}] concept`, `## ${g.concept.title}\n\n${g.concept.markdown}`);
    });
    arr(l.conjugations).forEach((c, i) => {
      const item = c.item ?? c.verb;
      const set = sets.get(c.set ?? c.tense) ?? c.set ?? c.tense;
      const forms = Object.fromEntries(Object.entries<string>(c.forms ?? {}).map(([k, v]) => [slots.get(k) ?? k, v]));
      push({ at: `${p}conjugations[${i}]`, kind: "conj", item, meaning: c.meaning, set, forms });
    });
  }
}
