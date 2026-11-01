// The answer key: the right answer to any question or Sprint mode exercise on
// screen, read from the course's own files, never from the server. Specs
// answer from it instead of hard-coding answers, so they survive edited,
// reordered and new questions (issue #48, docs/testing.md "Smoke specs").
//
// Study and Play questions carry their question id (`data-qid`). Sprint mode's stage
// carries the exercise's id (`sprint-play-stage[data-exercise-id]`), which names
// where the exercise came from: `sort:<lesson id>:<n>`, `pick:s:<lesson
// id>:<vocabulary index>`, … (see src/sprint.rs). The lookups mirror those
// builders, including the old field names (`es`/`en`, `verb`/`tense`) and
// a worn theme's material, which goes first.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { courseDir } from "./app.ts";
import { hourLater } from "./clock.ts";

/* eslint-disable @typescript-eslint/no-explicit-any */
type Json = any;

export interface Question {
  question_id: string;
  type: string;
  prompt: string;
  options: string[];
  correct_answer: string;
  accepted_answers: string[];
  octave: boolean; // keyboard: only the exact key counts
  lesson: string; // lesson id
}

interface Vocab {
  term: string;
  meaning: string;
}

interface Lesson {
  id: string;
  slug: string;
  title: string;
  quiz: Question[];
  vocabulary: Vocab[];
  conjugations: { item: string; set: string; forms: Record<string, string> }[];
  conversations: { lines: { text: string; meaning: string }[] }[];
  listening: { text: string; meaning: string }[];
  sorts: { items: { text?: string; play?: string; bucket: number }[]; buckets: string[] }[];
  spots: { sentence?: string; abc?: string; wrong: string | number; fix: string }[];
  builds: { text: string }[];
  scales: { title: string; notes: string; decoys?: string }[];
  themes: Record<string, Json>;
}

/** What to do on a Sprint mode exercise to get it right. */
export type SprintAnswer =
  | { kind: "option"; value: string } // choice, pick, hear-pick, hear-which: the option whose value is this
  | { kind: "tile"; value: string } // tiles, hear-gap: the tile that is this, or typed when there are no tiles
  | { kind: "clock"; value: string } // a clock to set: this time
  | { kind: "words"; words: string[] } // build, hear-build: tap these tiles in order
  | { kind: "spot"; wrong: string } // the word to tap
  | { kind: "note"; index: number; fix: string } // a spot on a staff: the note to tap (from 0), then the fix tile (its name)
  | { kind: "sort"; bucketOf: Map<string, number> } // item text (or a speaker tile's play) -> bucket index
  | { kind: "ladder"; forms: string[] } // tap in this order (a scale: the note names of rungs 2 to 8)
  | { kind: "pairs"; meaningOf: Map<string, string> } // match: term -> meaning
  | { kind: "same" } // hear-match: the speaker's data-value is the word
  | { kind: "bar"; tiles: string[] | null } // fill-bar: tap these tiles (null: any that fill the gap)
  | { kind: "chord"; notes: string[] } // chord-spell: tap these notes (ABC) into the slots
  | { kind: "staff"; abc: string } // staff-note: place this note (ABC with its octave)
  | { kind: "key"; answers: string[]; exact: boolean } // keyboard: tap a key of one of these ABC notes
  | { kind: "none" }; // a bite: nothing to answer

/** "azul." -> "azul", as `bare` in src/sprint.rs. */
export const bare = (w: string) => w.replace(/^[^\p{L}\p{N}]+|[^\p{L}\p{N}]+$/gu, "");
const words = (s: string) => s.split(/\s+/).filter(Boolean);
const lc = (s: string) => s.trim().toLowerCase();

function walk(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir).sort()) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) walk(path, out);
    else if (name.endsWith(".json")) out.push(path);
  }
  return out;
}

const vocab = (v: Json): Vocab => ({ term: v.term ?? v.es, meaning: v.meaning ?? v.en ?? "" });
const line = (l: Json) => ({ text: l.text ?? l.es, meaning: l.meaning ?? l.en ?? "" });
const conversation = (c: Json) => ({ lines: (c.lines ?? []).map(line) });
const question = (lesson: string) => (q: Json): Question => ({
  question_id: q.question_id,
  type: q.type ?? q.kind,
  prompt: q.prompt,
  options: q.options ?? [],
  correct_answer: String(q.correct_answer ?? ""), // a fill-bar question may leave it out
  accepted_answers: (q.accepted_answers ?? []).map(String),
  octave: q.octave === true,
  lesson,
});

function readLesson(path: string): Lesson {
  const j = JSON.parse(readFileSync(path, "utf8"));
  const d = j.sprint ?? j.dmode ?? {};
  return {
    id: j.id,
    slug: j.slug,
    title: j.title,
    quiz: (j.quiz ?? []).map(question(j.id)),
    vocabulary: (j.vocabulary ?? []).map(vocab),
    conjugations: (j.conjugations ?? []).map((c: Json) => ({ item: c.item ?? c.verb, set: c.set ?? c.tense, forms: c.forms ?? {} })),
    conversations: (j.conversations ?? []).map(conversation),
    listening: (j.listening ?? []).map(line),
    sorts: d.sorts ?? [],
    spots: d.spots ?? [],
    builds: (d.builds ?? []).map((b: Json) => ({ text: b.text ?? b.es })),
    scales: d.scales ?? [],
    themes: j.themes ?? {},
  };
}

/** An ABC note (`F`, `^c`, `_B,`) in letter names, as the test course shows them (`music.note_names` letters). */
export function noteName(abc: string): string {
  const m = /^([\^_=]*)([A-Ga-g])/.exec(abc.trim());
  if (!m) return abc;
  const sign: Record<string, string> = { "": "", "^": "♯", "^^": "𝄪", _: "♭", __: "𝄫", "=": "♮" };
  return m[2].toUpperCase() + (sign[m[1]] ?? "");
}

/** The lesson as the server serves it while `theme` is worn (apply_theme in src/content.rs). */
function themed(l: Lesson, theme: string | undefined): Lesson {
  const t = theme ? l.themes[theme] : undefined;
  if (!t) return l;
  const vocabulary = l.vocabulary.slice();
  for (const tv of t.vocabulary ?? []) if (!vocabulary.some((v) => v.term === tv.term)) vocabulary.push(vocab(tv));
  return {
    ...l,
    quiz: [...(t.quiz ?? []).map(question(l.id)), ...l.quiz],
    vocabulary,
    listening: [...(t.listening ?? []).map(line), ...l.listening],
    conversations: [...(t.conversations ?? []).map(conversation), ...l.conversations],
    sorts: [...(t.sorts ?? []), ...l.sorts],
    spots: [...(t.spots ?? []), ...l.spots],
    builds: [...(t.builds ?? []).map((b: Json) => ({ text: b.text ?? b.es })), ...l.builds],
  };
}

/** A typed answer that is wrong for every question of the type, in the test course and Spanish. */
export const WRONG_TYPED: Record<string, string> = {
  "fill-in-blank": "zzz",
  numeric: "987654",
  expression: "7x + 3",
  "algebra-tiles": "(x + 9)(x + 9)",
};

export class AnswerKey {
  readonly course: string;
  readonly lessons: Lesson[];
  private readonly slots: string[];
  private theme: string | undefined;

  constructor(courseId = "spanish", theme?: string) {
    const dir = courseDir(courseId);
    const manifest = JSON.parse(readFileSync(join(dir, "course.json"), "utf8"));
    this.course = courseId;
    this.slots = (manifest.paradigm?.slots ?? []).map((s: Json) => s.key);
    this.lessons = walk(join(dir, "lessons")).map(readLesson);
    this.theme = theme;
  }

  /** The theme the learner wears from now on (`undefined`: none). */
  wear(theme: string | undefined): this {
    this.theme = theme;
    return this;
  }

  lesson(idOrSlug: string): Lesson {
    const l = this.lessons.find((x) => x.id === idOrSlug || x.slug === idOrSlug);
    if (!l) throw new Error(`answer key: no lesson "${idOrSlug}" in course ${this.course}`);
    return themed(l, this.theme);
  }

  /** A quiz question by id, themed ones included (they exist only while their theme is worn). */
  question(id: string): Question {
    for (const l of this.lessons) {
      const q = l.quiz.find((x) => x.question_id === id) ?? Object.values(l.themes).flatMap((t: Json) => (t.quiz ?? []).map(question(l.id))).find((x) => x.question_id === id);
      if (q) return q;
    }
    throw new Error(`answer key: no question "${id}" in course ${this.course}`);
  }

  /** The text to give for question `id`: an option's value as shown (True/False included), or what to type. */
  answer(id: string, options: string[] = []): string {
    const q = this.question(id);
    return options.find((o) => o === q.correct_answer) ?? options.find((o) => lc(o) === lc(q.correct_answer)) ?? q.correct_answer;
  }

  /**
   * A wrong answer to question `id`: an option that is neither the answer nor
   * an accepted one, else something to type that no question of its type
   * accepts (still readable, so it is graded wrong rather than refused).
   */
  wrong(id: string, options: string[] = []): string {
    const q = this.question(id);
    const right = new Set([q.correct_answer, ...q.accepted_answers].map(lc));
    if (options.length) {
      const o = options.find((x) => !right.has(lc(x)));
      if (o === undefined) throw new Error(`answer key: question ${id} has no wrong option among ${JSON.stringify(options)}`);
      return o;
    }
    if (q.type === "clock") return hourLater(q.correct_answer);
    return WRONG_TYPED[q.type] ?? WRONG_TYPED["fill-in-blank"];
  }

  /** What to do on the Sprint mode exercise `id` (`sprint-play-stage[data-exercise-id]`) of kind `kind`. */
  sprint(kind: string, id: string): SprintAnswer {
    if (kind === "bite" || !id) return { kind: "none" };
    const [tag, ...rest] = id.split(":");
    const n = (s: string) => Number(s);
    switch (tag) {
      case "sort": {
        const s = this.lesson(rest[0]).sorts[n(rest[1])];
        // A speaker tile (#1219) has `play` and no text: its card says the music in data-play.
        return { kind: "sort", bucketOf: new Map(s.items.map((i) => [i.text || i.play || "", i.bucket])) };
      }
      case "spot": {
        const s = this.lesson(rest[0]).spots[n(rest[1])];
        if (s.abc) return { kind: "note", index: Number(s.wrong) - 1, fix: noteName(s.fix) };
        return { kind: "spot", wrong: String(s.wrong) };
      }
      case "build":
        return { kind: "words", words: words(this.lesson(rest[0]).builds[n(rest[1])].text) };
      case "cbuild":
        return { kind: "words", words: words(this.lesson(rest[0]).conversations[n(rest[1])].lines[n(rest[2])].text) };
      case "ladder": {
        // ladder:<lesson>:<item>:<set>; the item may not contain ':', the set comes last.
        const set = rest.at(-1)!, item = rest.slice(1, -1).join(":");
        const k = this.lesson(rest[0]).conjugations.find((c) => c.item === item && c.set === set);
        if (!k) throw new Error(`answer key: no conjugation ${item}/${set} in ${rest[0]}`);
        return { kind: "ladder", forms: this.slots.map((s) => k.forms[s] ?? "") };
      }
      case "scale": {
        // Build the scale (#1229): the first rung comes filled; tap the other seven notes in order.
        const s = this.lesson(rest[0]).scales[n(rest[1])];
        return { kind: "ladder", forms: words(s.notes).slice(1).map(noteName) };
      }
      case "pick": {
        const v = this.lesson(rest[1]).vocabulary[n(rest[2])];
        return { kind: "option", value: rest[0] === "s" ? v.term : v.meaning };
      }
      case "match": {
        const ls = rest[0] === "*" ? this.lessons.map((l) => themed(l, this.theme)) : [this.lesson(rest[0])];
        return { kind: "pairs", meaningOf: new Map(ls.flatMap((l) => l.vocabulary.map((v) => [v.term, v.meaning] as [string, string]))) };
      }
      case "hbuild":
        // The tiles are the sentence's words without the punctuation around them (issue #413).
        return { kind: "words", words: words(this.lesson(rest[0]).listening[n(rest[1])].text).map(bare).filter(Boolean) };
      case "hgap":
        return { kind: "tile", value: bare(words(this.lesson(rest[0]).listening[n(rest[1])].text)[n(rest[2])]) };
      case "hpick":
      case "hwhich":
        return { kind: "option", value: this.lesson(rest[0]).vocabulary[n(rest[1])].term };
      case "hmean":
        return { kind: "option", value: this.lesson(rest[0]).listening[n(rest[1])].meaning };
      case "hmatch":
        return { kind: "same" };
    }
    // Otherwise a quiz question: choice, or tiles (typed when there are none).
    const q = this.question(id);
    if (kind === "choice" || (kind === "clock" && q.options.length)) return { kind: "option", value: q.correct_answer };
    if (kind === "fill-bar") return { kind: "bar", tiles: q.correct_answer ? words(q.correct_answer) : null };
    if (kind === "chord-spell") return { kind: "chord", notes: words(q.correct_answer) };
    if (kind === "staff-note") return { kind: "staff", abc: q.correct_answer };
    if (kind === "keyboard") return { kind: "key", answers: [q.correct_answer, ...q.accepted_answers], exact: q.octave };
    if (kind === "clock") return { kind: "clock", value: q.correct_answer };
    return { kind: "tile", value: q.correct_answer };
  }
}
