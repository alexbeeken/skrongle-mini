// Asking Jev for every unit: the answer cache, the two stages, parallel calls.
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import { ask as jevAsk, MODEL, usage } from "../lib/jev.ts";
import { decide, questions, stateOf, type Ask, type Finding, type Signals } from "./probe.ts";
import type { Course, Unit } from "./units.ts";

export const hash = (...parts: unknown[]) => createHash("sha1").update(JSON.stringify(parts)).digest("hex");

/** Jev's answers by (model, state, questions): a rerun only pays for what changed. */
export class Cache {
  data: Record<string, unknown> = {};
  hits = 0;
  readonly file: string;
  readonly enabled: boolean;
  constructor(file: string, enabled: boolean) {
    this.file = file;
    this.enabled = enabled;
    if (enabled && existsSync(file)) {
      try {
        this.data = JSON.parse(readFileSync(file, "utf8"));
      } catch {
        this.data = {};
      }
    }
  }
  ask: Ask = async (state, qs) => {
    const k = hash(MODEL, state, qs);
    if (this.enabled && k in this.data) {
      this.hits++;
      return this.data[k] as Record<string, any>;
    }
    const a = await jevAsk(state as never, qs);
    this.data[k] = a;
    return a;
  };
  save() {
    if (!this.enabled) return;
    mkdirSync(dirname(this.file), { recursive: true });
    writeFileSync(this.file, JSON.stringify(this.data));
  }
}

export interface Result {
  unit: Unit;
  finding: Finding | null;
  /** The numbers the verdict came from. */
  signals: Signals;
}

/** Probe every unit, `concurrency` at a time; errors are collected, not thrown. */
export async function probeAll(units: Unit[], courses: Map<string, Course>, cache: Cache, concurrency: number, progress?: (done: number, found: number) => void) {
  const results: Result[] = [];
  const errors: string[] = [];
  let next = 0, done = 0, found = 0;
  async function worker() {
    while (next < units.length) {
      const u = units[next++];
      const course = courses.get(u.course)!;
      try {
        const a = await cache.ask(stateOf(u), questions(u, course.lang));
        const signals: Signals = {};
        const finding = await decide(u, a, course, cache.ask, signals);
        if (finding) found++;
        results.push({ unit: u, finding, signals });
      } catch (e) {
        errors.push(`${u.file} ${u.at}: ${(e as Error).message}`);
      }
      done++;
      if (done % 25 === 0) {
        progress?.(done, found);
        if (done % 500 === 0) cache.save();
      }
    }
  }
  await Promise.all(Array.from({ length: Math.min(concurrency, units.length) }, worker));
  progress?.(done, found);
  return { results, errors, calls: usage.calls };
}
