// The questions Jev is asked about a passage, and how its answers become a
// finding. Jev is a classifier with no web access: "this looks copied" is a
// judgement from what it remembers, wrong in both directions. So a Jev
// finding alone is always a CHECK; an ERROR also needs hard evidence from
// outside the model (a verbatim run of words shared with a reference text,
// see prefilter.ts). Output is "similarity risk", not a legal finding: rules
// of grammar, conjugation tables and textbook-standard examples are free to
// use; selection and arrangement are what can be copied.
import { choice, noul } from "@typesafe-ai/sdk";
import type { Hit } from "./prefilter.ts";
import type { Passage } from "./passages.ts";
import { PRODUCTS } from "./products.ts";

/** Bump when the questions change: the answer cache is keyed on it. */
export const PROMPT_VERSION = 1;

export const SOURCES = {
  product_a: PRODUCTS[0],
  product_b: PRODUCTS[1],
  app: "another course app or website",
  textbook: "a textbook",
  grammar: "a grammar website or reference grammar",
  phrasebook: "a dictionary or phrasebook",
  none: "none: generic, common knowledge, or original",
} as const;

export const T = {
  /** "Reproduces or closely paraphrases a specific published source" at least this. */
  copied: 0.8,
  /** "Distinctive wording" at least this. */
  distinctive: 0.7,
  /** A shared run of this many words with a reference is hard evidence. */
  run: 12,
};

export interface Finding {
  verdict: "error" | "check";
  /** Suspected kind of source (Jev cannot give a citation). */
  source: string;
  p: number;
  show: string[];
  evidence: string[];
}

export type Ask = (state: Record<string, unknown>, questions: Record<string, any>) => Promise<Record<string, any>>;

export const stateOf = (p: Passage) => ({ prompt_version: PROMPT_VERSION, ...p.ctx, passage: p.text });

export function questions(): Record<string, any> {
  return {
    copied: noul(
      "This passage reproduces, or is a close paraphrase of, text from a specific published source you recognise (a course app, textbook, grammar site or phrasebook). Say no if you only suspect it.",
      { true: "Yes, I recognise it", false: "No, nothing in particular" },
    ),
    distinctive: noul(
      "Its wording is distinctive: two independent authors would be unlikely to write it. Not a textbook-standard example, a standard conjugation table or a rule everyone states the same way.",
      { true: "Yes, distinctive", false: "No, standard or generic" },
    ),
    selection: noul("The order and selection of its items (which examples, in what sequence, with which translations) mirrors one well-known source."),
    source: choice("If it resembles a published source, which kind?", SOURCES),
  };
}

const pct = (n: number) => n.toFixed(2);

/** Jev's answers + the prefilter's hits -> a finding, or null. */
export function decide(p: Passage, a: Record<string, any>, hits: Hit[] = []): Finding | null {
  const copied = a.copied.noul as number, distinctive = a.distinctive.noul as number, selection = a.selection.noul as number;
  const source = String(a.source.choice);
  const hard = hits.find((h) => words12(h.quote) >= T.run);
  const jevFlag = copied >= T.copied && distinctive >= T.distinctive && source !== "none";
  if (!jevFlag && !hard) return null;
  const evidence = [`Jev: copied ${pct(copied)}, distinctive ${pct(distinctive)}, same selection ${pct(selection)}, looks like ${SOURCES[source as keyof typeof SOURCES] ?? source}`];
  for (const h of hits) evidence.push(`shares ${h.shared} shingles (${Math.round(h.coverage * 100)} %) with ${h.other}: «${h.quote}»`);
  // A verbatim run alone is not enough: a generic sentence matches everywhere.
  const verdict = hard && distinctive >= T.distinctive ? "error" : "check";
  if (!jevFlag && verdict === "check") evidence.push("shared run found by the pre-filter; Jev does not find the wording distinctive");
  return { verdict, source, p: Math.max(copied, hard ? 1 : 0), show: p.text.split("\n").slice(0, 3).map((l) => l.slice(0, 160)), evidence };
}

const words12 = (s: string) => s.split(/\s+/).filter(Boolean).length;
