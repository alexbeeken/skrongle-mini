// The questions Jev is asked about each unit, and how its answers become a
// finding. Designed and tuned by corrupting real items on purpose (wrong
// gender, ser/estar, preposition, verb person or tense, dropped accent,
// swapped translation, flipped answer key or bucket, broken spot fix) and
// measuring which questions catch them; `--calibrate` reruns that test.
//
// The principles:
// - Check the item against the author's own reasoning. Jev answers the quiz
//   or sorts the item *following the lesson's rule* (the explanation, hints,
//   the sort's "why"), and separately judges the key given that rule. Asked
//   cold, Jev's own gaps flagged 1-2 % of correct keys, every one a false
//   alarm on the real course; with the rule, 0 of 233 quiz keys and 2 of 931
//   sort items, while still catching 94 % of flipped keys. An error needs
//   both framings to agree.
// - Ask *where*: a Choice over the words of the line, with "none". It finds
//   the planted word 65-95 % of the time and gives the report something to
//   point at.
// - Ask for the *repair*: Jev can't write, so the script proposes the
//   candidates (confusable words, other forms of the same verb, accent and
//   ending variants), each one a word the course itself uses, and Jev picks
//   the correct full sentence, the original included. A repair it prefers is
//   both strong evidence and the fix to apply.
// - Two signals before a judgement without a repair: "is there a mistake"
//   on its own fires on plenty of correct sentences.
import { choice, noul } from "@typesafe-ai/sdk";
import type { Course, Lang, Unit } from "./units.ts";
import { WORD } from "./units.ts";

export type Verdict = "error" | "check";

export interface Finding {
  verdict: Verdict;
  /** What is wrong, in a few words: "Spanish · ser/estar", "answer key", "translation". */
  what: string;
  /** The decisive probability. */
  p: number;
  /** The item as written, the suspect word in «». */
  show: string[];
  /** Why, one line each, with Jev's probabilities. */
  evidence: string[];
  /** The corrected text, when a repair won. */
  fix?: { from: string; to: string };
}

/**
 * Thresholds. Tuned 2026-10-15 on jev-1.13.0 against two things at once:
 * planted mistakes (`--calibrate`, 60 per kind) for how much is caught, and
 * every unit of the real Spanish course (6,040, nearly all correct) for how
 * often a correct item is flagged. At these values the real course gave 4
 * checks and no errors; 73 % of planted mistakes were caught, most as
 * errors with the right fix. A rate of false flags that looks small per unit
 * (1 %) is 60 bogus findings a run, which is why each rule needs two signals.
 */
export const T = {
  /** The second call (repairs) runs when the mistake detector or "where" reaches this. */
  suspect: 0.3,
  /** A repair Jev prefers at least this much is an error with a fix (0 false flags; 60 % of word mistakes, fix right). */
  repair: 0.8,
  /** Without a repair: "is there a mistake" and "where" (not "none") must both reach this; a check. */
  detect: 0.9,
  /** Translation: "faithful" under 1 - this and the item's kind "translation" at least `translationKind` (0 false flags, 97 %). */
  translation: 0.95,
  translationKind: 0.5,
  /** Keys, as [the exercise done by Jev, the key judged], each under these. */
  key: {
    mc: { error: [0.2, 0.2], check: [0.4, 0.4] },
    sort: { error: [0.2, 0.1], check: [0.4, 0.2] },
  },
  /** The corrected spot sentence still has a mistake (0 false flags, 88 %). */
  spot: { error: 0.9, check: 0.65 },
  /** A conjugated form under `form` gets a repair round (`conjRepair` to win); under `formCheck` with no repair it is a check. */
  form: 0.65,
  conjRepair: 0.7,
  formCheck: 0.5,
  /** Vocabulary: the meaning judged accurate under `meaning` and matched under `match`; both under 0.2 is an error. */
  meaning: 0.5,
  match: 0.3,
  /** An accepted answer of a fill-in question Jev rates under this. */
  accepted: 0.1,
};

/** The raw numbers behind a verdict, for calibration. */
export type Signals = Record<string, number | string>;

export type Ask = (state: Record<string, unknown>, questions: Record<string, any>) => Promise<Record<string, any>>;

const pr = (a: any, k: string) => (a?.probabilities?.[k] ?? 0) as number;
const f2 = (p: number) => p.toFixed(2);
const q = (s: string) => `"${s}"`;

// ---------------------------------------------------------------- shared question shapes

function mistake(lang: Lang) {
  return lang.lang
    ? noul(`Is there a mistake in the ${lang.L}?`, {
        true: "There is at least one mistake: wrong grammar, gender or agreement, wrong conjugation or person, a misspelling or wrong accent, or the wrong word.",
        false: "A native speaker would write exactly this.",
      })
    : noul("Is there a mistake here?", {
        true: "At least one: a false claim, a wrong number or formula, or a term used with the wrong meaning.",
        false: "Everything here is accurate.",
      });
}

function lineMistake(lang: Lang) {
  return lang.lang
    ? noul("Is there a mistake here?", {
        true: `At least one: a false rule or claim, a wrong ${lang.L} form, spelling, accent or word, or a wrong translation.`,
        false: "Everything here is correct.",
      })
    : mistake(lang);
}

function kinds(lang: Lang, line: boolean): Record<string, string> {
  if (!lang.lang)
    return {
      none: "Nothing is wrong",
      claim: "A claim or rule is false",
      number: "A number, formula or calculation is wrong",
      term: "A term is used with the wrong meaning",
      language: "A grammar or spelling mistake",
    };
  return {
    none: line ? "Nothing is wrong" : `Nothing is wrong: the ${lang.L} is correct and the ${lang.M} matches it`,
    grammar: `${lang.L} grammar: gender, agreement, conjugation or person`,
    spelling: `${lang.L} spelling or accents`,
    word: `${lang.L} word choice: the wrong word, preposition or verb`,
    translation: `A ${lang.M} translation does not say the same thing as the ${lang.L}`,
    ...(line ? { claim: "A rule or claim about the language is false" } : {}),
  };
}

const tokens = (text: string) => text.match(WORD) ?? [];

function where(text: string) {
  return choice("Which word is a mistake? Pick the single word a careful teacher would circle, or none.", {
    none: "No word is a mistake",
    ...Object.fromEntries(tokens(text).slice(0, 200).map((w, i) => [`w${i}`, `"${w}" (word ${i + 1})`])),
  });
}

/** `text` with its i-th word wrapped in «». */
function mark(text: string, i: number): string {
  let n = -1;
  return text.replace(WORD, (w) => (++n === i ? `«${w}»` : w));
}
function replaceWord(text: string, i: number, by: string): string {
  let n = -1;
  return text.replace(WORD, (w) => (++n === i ? by : w));
}

// ---------------------------------------------------------------- repairs

/** Words learners and authors confuse, by language; each group is named for the report. */
const CONFUSABLE: Record<string, [string, string[]][]> = {
  es: [
    ["ser/estar", ["es", "está", "son", "están", "soy", "estoy", "eres", "estás", "somos", "estamos", "sois", "estáis", "era", "estaba", "eran", "estaban", "fue", "estuvo", "ser", "estar"]],
    ["hay/estar", ["hay", "está", "están"]],
    ["article", ["el", "la", "los", "las", "lo", "un", "una", "unos", "unas", "al", "del"]],
    ["preposition", ["a", "en", "de", "por", "para", "con", "sin", "desde", "hasta", "al", "del"]],
    ["demonstrative", ["este", "esta", "estos", "estas", "esto", "ese", "esa", "esos", "esas", "eso", "aquel", "aquella"]],
    ["possessive", ["mi", "mis", "tu", "tus", "su", "sus", "nuestro", "nuestra", "nuestros", "nuestras"]],
    ["pronoun", ["me", "te", "se", "nos", "os", "le", "les", "lo", "la", "los", "las"]],
    ["accent (question word)", ["que", "qué", "como", "cómo", "donde", "dónde", "cuando", "cuándo", "quien", "quién", "cual", "cuál"]],
    ["accent", ["el", "él", "tu", "tú", "si", "sí", "mas", "más", "mi", "mí", "se", "sé", "te", "té", "de", "dé"]],
    ["quantity", ["muy", "mucho", "mucha", "muchos", "muchas"]],
    ["short form", ["bueno", "buen", "buena", "malo", "mal", "mala", "grande", "gran", "primero", "primer", "tercero", "tercer"]],
  ],
};

const strip = (s: string) => s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();

/** The course's words by their spelling without accents: "esta" -> esta, está. */
const accentIndexes = new WeakMap<Course, Map<string, string[]>>();
function accentIndex(course: Course): Map<string, string[]> {
  let ix = accentIndexes.get(course);
  if (!ix) {
    ix = new Map();
    for (const w of course.lexicon.keys()) ix.set(strip(w), [...(ix.get(strip(w)) ?? []), w]);
    accentIndexes.set(course, ix);
  }
  return ix;
}

/** Candidate repairs for `word`, each with what kind of mistake it would fix. */
export function candidates(word: string, course: Course): { to: string; kind: string }[] {
  const lw = word.toLowerCase();
  const out = new Map<string, string>();
  const add = (v: string, kind: string) => {
    const lv = v.toLowerCase();
    if (lv === lw || out.has(lv)) return;
    if (!lv.includes(" ") && !course.lexicon.has(lv)) return; // only words the course uses
    out.set(lv, kind);
  };
  for (const [name, g] of CONFUSABLE[course.lang.code.split("-")[0]] ?? []) if (g.includes(lw)) g.forEach((v) => add(v, name));
  for (const v of course.siblings.get(lw) ?? []) add(v, "verb form");
  for (const v of accentIndex(course).get(strip(lw)) ?? []) add(v, "accent");
  const m = lw.match(/^(.{2,}?)(o|a|os|as|e|es)$/);
  if (m) for (const e of ["o", "a", "os", "as", "e", "es"]) add(m[1] + e, "agreement");
  const cap = word[0] !== word[0].toLowerCase();
  return [...out].slice(0, 60).map(([v, kind]) => ({ to: cap ? v[0].toUpperCase() + v.slice(1) : v, kind }));
}

/** Jev picks the correct version of `text` among the original and one repair per candidate. */
async function repair(ask: Ask, state: Record<string, unknown>, text: string, i: number, course: Course, instructions: string) {
  const word = tokens(text)[i];
  const cs = candidates(word, course);
  if (!cs.length) return undefined;
  const opts = { s0: text, ...Object.fromEntries(cs.map((c, j) => [`s${j + 1}`, replaceWord(text, i, c.to)])) };
  const a = await ask(state, { pick: choice(instructions, opts) });
  const best = cs.map((c, j) => ({ ...c, p: pr(a.pick, `s${j + 1}`) })).sort((x, y) => y.p - x.p)[0];
  return { word, best, original: pr(a.pick, "s0") };
}

// ---------------------------------------------------------------- probes

const CRIT_TR = (lang: Lang) =>
  noul(`Does the ${lang.M} say exactly what the ${lang.L} says: the same people, tense, negation and facts?`, {
    true: "Yes, a faithful translation",
    false: "No, it adds, drops or changes something",
  });

/** The state Jev sees for a unit: context first, then only what the questions are about. */
export function stateOf(u: Unit): Record<string, unknown> {
  const c = u.ctx;
  switch (u.kind) {
    case "sentence": return { ...c, text: u.text, ...(u.meaning ? { translation: u.meaning } : {}), ...(u.note ? { note: u.note } : {}) };
    case "line": return { ...c, ...(u.section ? { section: u.section } : {}), ...(u.header ? { table_header: u.header, table_row: u.text } : { line: u.text }), ...(u.about ?? {}) };
    case "mc": return { ...c, prompt: u.prompt, options: u.options, ...(u.explain ? { lesson_rule: u.explain } : {}) };
    case "fill": return { ...c, prompt: u.prompt, answer_key: u.answer, ...(u.explain ? { lesson_rule: u.explain } : {}) };
    case "sort": return { ...c, sort_title: u.title, buckets: u.buckets, item: u.text, ...(u.why ? { rule: u.why } : {}) };
    case "spot": return { ...c, sentence: u.sentence, ...(u.why ? { why: u.why } : {}) };
    case "conj": return { ...c, item: u.item, meaning: u.meaning, set: u.set, forms: u.forms };
    case "vocab": return { ...c, term: u.term };
    case "facts": return { ...c, ...u.data };
  }
}

/** Stage one: the questions for a unit. */
export function questions(u: Unit, lang: Lang): Record<string, any> {
  switch (u.kind) {
    case "sentence":
      return {
        mistake: mistake(lang),
        kind: choice("What, if anything, is wrong with this item?", kinds(lang, false)),
        where: where(u.text),
        ...(u.meaning ? { translation: CRIT_TR(lang) } : {}),
        ...(u.note ? { note: noul("Is the note accurate?") } : {}),
      };
    case "line":
      return {
        mistake: lineMistake(lang),
        kind: choice(`What, if anything, is wrong with this line of a ${lang.L} lesson?`, kinds(lang, true)),
        where: where(u.text),
      };
    case "mc":
      return {
        answer: choice(`Answer the question${u.explain ? ", following the lesson's rule" : ""}: ${u.prompt}`, Object.fromEntries(u.options.map((o, i) => [`o${i}`, o]))),
        key: noul(`Is ${q(u.answer)} the right answer to the prompt?`, { true: "Yes, it is right", false: "No, another option is right" }),
      };
    case "fill": {
      const filled = u.prompt.replace("___", u.answer);
      return {
        mistake: noul(`Is there a mistake in this: ${q(filled)}?`, { true: "Yes, at least one mistake", false: lang.lang ? "No, a native speaker would write exactly this" : "No, it is right" }),
        where: where(filled),
        key: noul(`Is ${q(u.answer)} the right answer for the blank?`, { true: "Yes, it is right", false: "No, the blank needs something else" }),
        ...Object.fromEntries(u.accepted.slice(0, 8).map((a, i) => [`acc${i}`, noul(`Would ${q(a)} also be a right answer to the prompt?`)])),
      };
    }
    case "sort":
      return {
        bucket: choice(`${u.why ? "Apply the rule. " : u.title + " "}Which bucket does ${q(u.text)} belong in?`, Object.fromEntries(u.buckets.map((b, i) => [`b${i}`, b]))),
        key: noul(`${u.why ? "Following the rule, does" : "Does"} ${q(u.text)} belong in ${q(u.buckets[u.bucket])}?`, { true: "Yes", false: "No, it belongs in another bucket" }),
      };
    case "spot":
      return {
        where: where(u.sentence),
        fixed: noul(`Is there a mistake in this sentence: ${q(fixSpot(u.sentence, u.wrong, u.fix))}?`, {
          true: "Yes, at least one mistake", false: lang.lang ? "No, a native speaker would write exactly this" : "No, it is right",
        }),
      };
    case "conj":
      return Object.fromEntries(Object.entries(u.forms).map(([slot, f], i) => [`f${i}`, noul(`${q(f)} is the correct, correctly accented ${u.set} form of ${q(u.item)} for ${slot}.`)]));
    case "vocab":
      return {
        meaning: noul(lang.lang ? `${q(u.meaning)} is an accurate ${lang.M} meaning of the ${lang.L} ${q(u.term)}.` : `${q(u.meaning)} is an accurate definition of ${q(u.term)}.`),
        match: choice(lang.lang ? `What does the ${lang.L} ${q(u.term)} mean?` : `What does ${q(u.term)} mean?`, {
          given: u.meaning, ...Object.fromEntries(u.others.map((m, i) => [`m${i}`, m])), none: "None of these",
        }),
      };
    case "facts":
      return Object.fromEntries(Object.entries(u.statements).map(([k, s]) => [k, noul(s)]));
  }
}

/** The corrected sentence of a spot: the wrong word, as a word, replaced by the fix. */
export function fixSpot(sentence: string, wrong: string, fix: string): string {
  const re = new RegExp(`(?<![\\p{L}\\p{N}])${wrong.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}(?![\\p{L}\\p{N}])`, "iu");
  return sentence.replace(re, (m) => (m[0] !== m[0].toLowerCase() ? fix[0].toUpperCase() + fix.slice(1) : fix));
}

/** Stage two (repairs) where needed, then the finding, or null. */
export async function decide(u: Unit, a: Record<string, any>, course: Course, ask: Ask, sig: Signals = {}): Promise<Finding | null> {
  const lang = course.lang;
  switch (u.kind) {
    case "sentence":
    case "line":
      return textFinding(u.text, a, course, ask, stateOf(u), sig, { line: u.kind === "line", meaning: u.kind === "sentence" ? u.meaning : undefined, note: u.kind === "sentence" ? u.note : undefined });
    case "fill": {
      const filled = u.prompt.replace("___", u.answer);
      // Only the answer's words are under test; the rest of the prompt is the author's instructions and hints.
      const start = tokens(u.prompt.split("___")[0]).length;
      const f = await textFinding(filled, a, course, ask, stateOf(u), sig, { focus: [start, start + tokens(u.answer).length] });
      const pk = (sig.key = a.key?.noul ?? 1);
      if (f && pk < T.key.mc.check[1]) f.evidence.push(`Jev ${f2(pk)} that ${q(u.answer)} is the right answer${u.explain ? ", given the lesson's rule" : ""}`);
      // An accepted answer that is the key without accents, or the key with words around it, is the author's choice.
      const plain = (x: string) => strip(x).replace(/[^\p{L}\s]/gu, "").trim();
      const acc = u.accepted.slice(0, 8).map((x, i) => ({ x, p: a[`acc${i}`]?.noul ?? 1 }))
        .filter((x) => plain(x.x) !== plain(u.answer) && !plain(x.x).split(/\s+/).includes(plain(u.answer)) && x.p < T.accepted);
      sig.accepted = Math.min(1, ...acc.map((x) => x.p));
      if (!f && !acc.length) return null;
      const out: Finding = f ?? { verdict: "check", what: "accepted answer", p: 1 - acc[0].p, show: [u.prompt, `key: ${u.answer}  ·  accepted: ${u.accepted.join(" | ")}`], evidence: [] };
      if (f) out.what = f.fix && tokens(u.answer).some((w) => w.toLowerCase() === f.fix!.from.toLowerCase()) ? `answer key · ${f.what}` : f.what;
      for (const x of acc) out.evidence.push(`accepted answer ${q(x.x)}: Jev ${f2(x.p)} that it would also be right`);
      return out;
    }
    case "mc": {
      const pa = (sig.pa = pr(a.answer, `o${u.options.indexOf(u.answer)}`)), pk = (sig.pk = a.key?.noul ?? 1);
      const best = u.options.map((o, i) => ({ o, p: pr(a.answer, `o${i}`) })).sort((x, y) => y.p - x.p)[0];
      return keyFinding("answer key", T.key.mc, pa, pk, !!u.explain, [u.prompt, `options: ${u.options.join(" | ")}`, `key: ${u.answer}`],
        `${u.explain ? "following the lesson's rule, " : ""}Jev answers ${q(best.o)} (${f2(best.p)}); the key gets ${f2(pa)}`, u.explain);
    }
    case "sort": {
      const pa = (sig.pa = pr(a.bucket, `b${u.bucket}`)), pk = (sig.pk = a.key?.noul ?? 1);
      const best = u.buckets.map((b, i) => ({ b, p: pr(a.bucket, `b${i}`) })).sort((x, y) => y.p - x.p)[0];
      return keyFinding("sort bucket", T.key.sort, pa, pk, !!u.why, [`${u.title}  [${u.buckets.join(" | ")}]`, `item: ${u.text}  →  ${u.buckets[u.bucket]}`],
        `${u.why ? "applying the sort's rule, " : ""}Jev sorts it into ${q(best.b)} (${f2(best.p)}); the file's bucket gets ${f2(pa)}`, u.why);
    }
    case "spot": {
      const pf = (sig.pf = a.fixed?.noul ?? 0);
      if (pf < T.spot.check) return null;
      const fixed = fixSpot(u.sentence, u.wrong, u.fix);
      const ev = [`with the fix: ${q(fixed)} still has a mistake (Jev ${f2(pf)})`];
      const toks = tokens(u.sentence);
      const top = Object.entries<number>(a.where?.probabilities ?? {}).sort((x, y) => y[1] - x[1])[0];
      if (top?.[0] === "none" && top[1] >= 0.5) ev.push(`Jev sees no mistake in the original sentence (${f2(top[1])})`);
      else if (top && top[0] !== "none" && toks[+top[0].slice(1)]?.toLowerCase() !== u.wrong.toLowerCase() && top[1] >= 0.4)
        ev.push(`Jev puts the mistake at ${q(toks[+top[0].slice(1)])} (${f2(top[1])}), not ${q(u.wrong)}`);
      return { verdict: pf >= T.spot.error ? "error" : "check", what: "spot the mistake: the fix", p: pf, show: [u.sentence, `wrong: ${u.wrong} → fix: ${u.fix}`], evidence: ev };
    }
    case "conj": {
      const slots = Object.entries(u.forms);
      const weak = slots.map(([slot, f], i) => ({ slot, f, p: a[`f${i}`]?.noul ?? 1 })).filter((x) => x.p < T.form).sort((x, y) => x.p - y.p);
      sig.form = Math.min(1, ...slots.map((_, i) => a[`f${i}`]?.noul ?? 1));
      for (const w of weak) {
        if (!lang.lang || w.f.includes(" ")) continue;
        const r = await repair(ask, { ...u.ctx, item: u.item, set: u.set, slot: w.slot }, w.f, 0, course, `Which is the ${u.set} form of ${q(u.item)} for ${w.slot}?`);
        if (r) sig.repair = Math.max(Number(sig.repair ?? 0), r.best.p);
        if (r && r.best.p >= T.conjRepair)
          return {
            verdict: "error", what: `${u.set} of ${u.item} · ${r.best.kind}`, p: r.best.p,
            show: [`${u.item} (${u.set}): ${slots.map(([s, f]) => (s === w.slot ? `${s} «${f}»` : `${s} ${f}`)).join(", ")}`],
            evidence: [`fix: ${w.slot} ${q(w.f)} → ${q(r.best.to)}   (Jev ${f2(r.best.p)} for the fix, ${f2(r.original)} for the form as it is)`],
            fix: { from: w.f, to: r.best.to },
          };
      }
      const bad = weak.filter((x) => x.p < T.formCheck);
      if (!bad.length) return null;
      return {
        verdict: "check", what: `${u.set} of ${u.item}`, p: 1 - bad[0].p,
        show: [`${u.item} (${u.set}): ${slots.map(([s, f]) => (bad.some((b) => b.slot === s) ? `${s} «${f}»` : `${s} ${f}`)).join(", ")}`],
        evidence: bad.map((b) => `${b.slot} ${q(b.f)}: Jev ${f2(b.p)} that it is right`),
      };
    }
    case "vocab": {
      const pm = (sig.pm = a.meaning?.noul ?? 1), pg = (sig.pg = pr(a.match, "given"));
      if (pm >= T.meaning || pg >= T.match) return null;
      const alt = Object.entries<number>(a.match?.probabilities ?? {}).filter(([k]) => k !== "given").sort((x, y) => y[1] - x[1])[0];
      const ev = [`Jev ${f2(pm)} that ${q(u.meaning)} is what ${q(u.term)} means`];
      if (alt?.[0] === "none") ev.push(`none of the lesson's meanings fit it (${f2(alt[1])})`);
      else if (alt) ev.push(`Jev matches it to another entry's meaning: ${q(u.others[+alt[0].slice(1)])} (${f2(alt[1])})`);
      return { verdict: pm < 0.2 && pg < 0.2 ? "error" : "check", what: "meaning", p: 1 - Math.max(pm, pg), show: [`${u.term} = ${u.meaning}`], evidence: ev };
    }
    case "facts": {
      const bad = Object.entries(u.statements).filter(([k]) => (a[k]?.noul ?? 1) < 0.5);
      if (!bad.length) return null;
      return { verdict: "check", what: "calendar", p: 1 - Math.min(...bad.map(([k]) => a[k].noul)), show: [JSON.stringify(u.data)], evidence: bad.map(([k, s]) => `Jev ${f2(a[k].noul)}: ${s}`) };
    }
  }
}

/** A key (answer or bucket) both framings doubt: the exercise done by Jev (`pa`) and the key judged (`pk`). */
function keyFinding(what: string, t: { error: number[]; check: number[] }, pa: number, pk: number, ruled: boolean, show: string[], did: string, rule?: string): Finding | null {
  if (pa >= t.check[0] || pk >= t.check[1]) return null;
  // Without the author's rule Jev answers from its own knowledge, which is wrong on hard items: never more than a check.
  const verdict: Verdict = ruled && pa < t.error[0] && pk < t.error[1] ? "error" : "check";
  return { verdict, what, p: 1 - Math.max(pa, pk), show, evidence: [did, `Jev ${f2(pk)} that the key is right`, ...(rule ? [`lesson's rule: ${rule}`] : [])] };
}

/** Sentences, lesson lines, filled-in prompts: find the word, try repairs, else judge. */
async function textFinding(
  text: string, a: Record<string, any>, course: Course, ask: Ask, state: Record<string, unknown>, sig: Signals,
  { line = false, meaning, note, focus }: { line?: boolean; meaning?: string; note?: string; focus?: [number, number] },
): Promise<Finding | null> {
  const lang = course.lang;
  const det = (sig.det = Math.max(a.kind ? 1 - pr(a.kind, "none") : 0, a.mistake?.noul ?? 0));
  sig.mistake = a.mistake?.noul ?? 0;
  sig.kind = a.kind ? 1 - pr(a.kind, "none") : 0;
  const top = Object.entries<number>(a.where?.probabilities ?? {}).filter(([k]) => k !== "none").sort((x, y) => y[1] - x[1])[0];
  const i = top ? +top[0].slice(1) : -1;
  // A word outside the focus (the author's prompt around a blank) is not what is being checked.
  const inFocus = !focus || (i >= focus[0] && i < focus[1]);
  const wh = (sig.where = inFocus ? 1 - pr(a.where, "none") : 0);
  sig.top = top?.[1] ?? 0;
  const show = (t: string) => [i >= 0 && Math.max(det, wh) >= T.suspect ? mark(t, i) : t, ...(meaning ? [`${lang.M}: ${meaning}`] : [])];
  const kindTop = Object.entries<number>(a.kind?.probabilities ?? {}).filter(([k]) => k !== "none").sort((x, y) => y[1] - x[1])[0];
  const kindLabel = kindTop ? kinds(lang, line)[kindTop[0]] : "";

  if (lang.lang && i >= 0 && inFocus && Math.max(det, wh) >= T.suspect) {
    const r = await repair(ask, { ...state, ...(meaning ? { translation: meaning } : {}) }, text, i, course,
      meaning ? `Which version is correct ${lang.L} and says what the ${lang.M} says?` : `Which version of this line is correct?`);
    if (r) { sig.repair = r.best.p; sig.original = r.original; sig.repairKind = r.best.kind; }
    if (r && r.best.p >= T.repair)
      return {
        verdict: "error", what: `${lang.L} · ${r.best.kind}`, p: r.best.p, show: show(text),
        evidence: [`fix: ${q(r.word)} → ${q(r.best.to)}   (Jev ${f2(r.best.p)} for the fix, ${f2(r.original)} for the text as it is)`, `fixed: ${replaceWord(text, i, r.best.to)}`],
        fix: { from: r.word, to: r.best.to },
      };
  }
  const evidence: string[] = [];
  let verdict: Verdict | null = null, what = "", p = 0;
  if (det >= T.detect && wh >= T.detect) {
    verdict = "check"; what = kindTop?.[0] === "translation" ? "translation" : `${lang.lang && kindTop?.[0] !== "claim" ? lang.L + " · " : ""}${kindTop?.[0] ?? "mistake"}`; p = Math.min(det, wh);
    evidence.push(`Jev ${f2(det)} that something is wrong: ${kindLabel.toLowerCase()} (${f2(kindTop?.[1] ?? 0)})`);
    evidence.push(`the word it would circle: ${q(tokens(text)[i])} (${f2(top[1])})`);
  }
  const tr = (sig.tr = a.translation ? 1 - a.translation.noul : 0);
  sig.kindTr = pr(a.kind, "translation");
  sig.kindClaim = pr(a.kind, "claim");
  sig.note = a.note?.noul ?? 1;
  if (meaning && tr >= T.translation && pr(a.kind, "translation") >= T.translationKind) {
    evidence.push(`the ${lang.M} does not say what the ${lang.L} says (Jev ${f2(tr)} that it adds, drops or changes something)`);
    verdict = "error"; what = "translation"; p = tr;
  }
  if (note && (a.note?.noul ?? 1) < 1 - T.detect && det >= T.detect) {
    evidence.push(`the note looks wrong (Jev ${f2(a.note.noul)} that it is accurate): ${note}`);
    if (!verdict) { verdict = "check"; what = "note"; p = 1 - a.note.noul; }
  }
  return verdict ? { verdict, what, p, show: show(text), evidence } : null;
}
