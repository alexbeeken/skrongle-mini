// Free checks, no key: word-shingle overlap. Catches verbatim and near
// verbatim reuse only, never a paraphrase. Run against the course itself
// (the same lines in two lessons) and against any reference corpus the
// maintainer is allowed to use (--sources: public-domain or openly licensed
// text, or excerpts kept locally; never competitors' pages fetched by us).
import { words } from "./passages.ts";
import type { Passage } from "./passages.ts";

export const SHINGLE = 6;

export const shingles = (text: string, n = SHINGLE): Set<string> => {
  const w = words(text), out = new Set<string>();
  for (let i = 0; i + n <= w.length; i++) out.add(w.slice(i, i + n).join(" "));
  return out;
};

export interface Hit {
  /** Where the overlap is: another lesson, or a reference source. */
  other: string;
  /** Shared shingles, and the longest shared run of words as text. */
  shared: number;
  quote: string;
  /** Share of this passage's shingles that are in the other text. */
  coverage: number;
}

/** The longest run of words that `a` and `b` share, as text. */
function longestRun(a: string[], b: string[]): string {
  let best = 0, end = 0;
  let prev = new Array(b.length + 1).fill(0);
  for (let i = 1; i <= a.length; i++) {
    const cur = new Array(b.length + 1).fill(0);
    for (let j = 1; j <= b.length; j++)
      if (a[i - 1] === b[j - 1]) {
        cur[j] = prev[j - 1] + 1;
        if (cur[j] > best) { best = cur[j]; end = i; }
      }
    prev = cur;
  }
  return a.slice(end - best, end).join(" ");
}

export interface Ref { name: string; text: string }

/**
 * Overlap of each passage with the references; only hits sharing at least `min` shingles.
 * `skip` leaves a reference out for a passage (its own lesson, say).
 */
export function overlaps(passages: Passage[], refs: Ref[], min = 1, skip?: (p: Passage, r: Ref) => boolean): Map<Passage, Hit[]> {
  const index = new Map<string, Set<number>>();
  refs.forEach((r, i) => { for (const s of shingles(r.text)) (index.get(s) ?? index.set(s, new Set()).get(s)!).add(i); });
  const out = new Map<Passage, Hit[]>();
  for (const p of passages) {
    const mine = shingles(p.text);
    if (!mine.size) continue;
    const per = new Map<number, number>();
    for (const s of mine) for (const i of index.get(s) ?? []) if (!skip?.(p, refs[i])) per.set(i, (per.get(i) ?? 0) + 1);
    const hits: Hit[] = [];
    for (const [i, n] of per) {
      if (n < min) continue;
      hits.push({ other: refs[i].name, shared: n, coverage: n / mine.size, quote: longestRun(words(p.text), words(refs[i].text)) });
    }
    if (hits.length) out.set(p, hits.sort((a, b) => b.shared - a.shared).slice(0, 3));
  }
  return out;
}

/** Passages that repeat text of another lesson of the same course. Self-overlap, not provenance. */
export function selfOverlaps(passages: Passage[]): Map<Passage, Hit[]> {
  const refs = passages.map((o) => ({ name: `${o.file} ${o.at}`, text: o.text, file: o.file }));
  return overlaps(passages, refs, 1, (p, r) => (r as (typeof refs)[number]).file === p.file);
}
