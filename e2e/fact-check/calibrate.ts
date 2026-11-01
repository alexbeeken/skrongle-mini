// --calibrate: how good is the fact check on this course, today, with this
// model? Takes a sample of real units, makes corrupted copies with one known
// mistake each (the kinds authors and generators actually make), probes both,
// and prints per mistake kind how many were caught, pointed at and fixed,
// and how many untouched units were flagged. Rerun it when JEV_MODEL changes
// or the thresholds in probe.ts move.
import { candidates } from "./probe.ts";
import type { Course, Unit } from "./units.ts";
import { WORD } from "./units.ts";

export interface Trial {
  unit: Unit;
  /** null for an untouched unit. */
  mistake: string | null;
  /** The word put in, which a finding should point at. */
  word?: string;
  /** The word taken out, which a fix should bring back. */
  original?: string;
}

// English claims flipped into false ones: what a wrong rule usually looks like.
const FLIPS: [string, string][] = [
  ["before", "after"], ["after", "before"], ["masculine", "feminine"], ["feminine", "masculine"], ["always", "never"],
  ["never", "always"], ["singular", "plural"], ["plural", "singular"], ["formal", "informal"], ["informal", "formal"],
  ["past", "future"], ["future", "past"], ["more", "less"], ["less", "more"], ["first", "last"], ["yesterday", "tomorrow"],
  ["tomorrow", "yesterday"], ["increase", "decrease"], ["decrease", "increase"], ["higher", "lower"], ["lower", "higher"],
];

/** A small seeded RNG, so a calibration samples the same units every time. */
function rng(seed: number) {
  return () => ((seed = (seed * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff);
}

const strip = (s: string) => s.normalize("NFD").replace(/\p{M}/gu, "");

function replaceNth(text: string, i: number, by: string) {
  let n = -1;
  return text.replace(WORD, (w) => (++n === i ? by : w));
}

/** Corrupted copies of `u`, at most one per mistake kind. */
function corrupt(u: Unit, course: Course, sameFile: Unit[], all: Unit[], r: () => number): Trial[] {
  const out: Trial[] = [];
  const pick = <T,>(xs: T[]) => xs[Math.floor(r() * xs.length)];
  const wordMistakes = (text: string, set: (t: string) => Unit) => {
    if (!course.lang.lang) return;
    const toks = text.match(WORD) ?? [];
    const kinds = new Map<string, { i: number; to: string }[]>();
    toks.forEach((w, i) => {
      for (const c of candidates(w, course)) {
        const k = c.kind === "accent" && strip(c.to) === c.to ? "accent dropped" : c.kind;
        if (c.kind === "accent" && strip(c.to) !== c.to) continue; // adding an accent is rarely the authors' mistake
        if (!kinds.has(k)) kinds.set(k, []);
        kinds.get(k)!.push({ i, to: c.to });
      }
    });
    for (const [k, xs] of kinds) {
      const x = pick(xs);
      out.push({ unit: set(replaceNth(text, x.i, x.to)), mistake: k, word: x.to, original: toks[x.i] });
    }
  };
  const flip = (text: string, set: (t: string) => Unit) => {
    for (const [a, b] of FLIPS) {
      const re = new RegExp(`\\b${a}\\b`, "i");
      if (re.test(text)) return out.push({ unit: set(text.replace(re, b)), mistake: "false claim", word: b, original: a });
    }
  };
  switch (u.kind) {
    case "sentence": {
      wordMistakes(u.text, (t) => ({ ...u, text: t }));
      const others = sameFile.filter((x): x is Extract<Unit, { kind: "sentence" }> => x.kind === "sentence" && !!x.meaning && x.meaning !== u.meaning);
      if (u.meaning && others.length) out.push({ unit: { ...u, meaning: pick(others).meaning }, mistake: "translation swapped" });
      break;
    }
    case "line":
      wordMistakes(u.text, (t) => ({ ...u, text: t }));
      flip(u.text, (t) => ({ ...u, text: t }));
      break;
    case "mc": {
      const wrong = u.options.filter((o) => o !== u.answer);
      if (wrong.length) out.push({ unit: { ...u, answer: pick(wrong) }, mistake: "answer key" });
      break;
    }
    case "fill": {
      const others = sameFile.filter((x): x is Extract<Unit, { kind: "fill" }> => x.kind === "fill" && x.answer.toLowerCase() !== u.answer.toLowerCase());
      if (others.length) out.push({ unit: { ...u, answer: pick(others).answer }, mistake: "answer key" });
      break;
    }
    case "sort":
      if (u.buckets.length > 1) out.push({ unit: { ...u, bucket: (u.bucket + 1 + Math.floor(r() * (u.buckets.length - 1))) % u.buckets.length }, mistake: "bucket" });
      break;
    case "spot": {
      const fixes = sameFile.filter((x): x is Extract<Unit, { kind: "spot" }> => x.kind === "spot" && x.fix !== u.fix);
      if (fixes.length) out.push({ unit: { ...u, fix: pick(fixes).fix }, mistake: "spot fix" });
      break;
    }
    case "conj": {
      const slots = Object.keys(u.forms);
      const s = pick(slots);
      const other = slots.map((k) => u.forms[k]).filter((f) => f !== u.forms[s]);
      if (other.length) {
        const to = pick(other);
        out.push({ unit: { ...u, forms: { ...u.forms, [s]: to } }, mistake: "form of another person", word: to, original: u.forms[s] });
      }
      const acc = slots.filter((k) => strip(u.forms[k]) !== u.forms[k]);
      if (acc.length) {
        const k = pick(acc);
        out.push({ unit: { ...u, forms: { ...u.forms, [k]: strip(u.forms[k]) } }, mistake: "accent dropped", word: strip(u.forms[k]), original: u.forms[k] });
      }
      break;
    }
    case "vocab": {
      const elsewhere = all.filter((x): x is Extract<Unit, { kind: "vocab" }> => x.kind === "vocab" && x.file !== u.file);
      if (elsewhere.length) out.push({ unit: { ...u, meaning: pick(elsewhere).meaning }, mistake: "meaning" });
      break;
    }
  }
  return out;
}

/** Up to `n` untouched units per unit kind, and up to `n` corrupted ones per (unit kind, mistake). */
export function trials(course: Course, n: number): Trial[] {
  const r = rng(Number(process.env.FACT_CHECK_SEED) || 20261015);
  const byFile = new Map<string, Unit[]>();
  for (const u of course.units) byFile.set(u.file, [...(byFile.get(u.file) ?? []), u]);
  const shuffled = [...course.units].sort(() => r() - 0.5);
  const clean = new Map<string, Trial[]>(), bad = new Map<string, Trial[]>();
  for (const u of shuffled) {
    if (u.kind === "facts") continue;
    const c = clean.get(u.kind) ?? [];
    if (c.length < n) clean.set(u.kind, [...c, { unit: u, mistake: null }]);
    for (const t of corrupt(u, course, byFile.get(u.file)!, course.units, r)) {
      const k = `${u.kind}\t${t.mistake}`;
      const b = bad.get(k) ?? [];
      if (b.length < n) bad.set(k, [...b, t]);
    }
  }
  return [...clean.values(), ...bad.values()].flat();
}
