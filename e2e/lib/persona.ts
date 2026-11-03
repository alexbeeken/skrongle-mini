// Personas: user profiles that change how Jev walks the app and what counts
// as a problem for that user (issues #39, #40). Profiles are data: one file
// per profile in e2e/personas/<id>.ts whose default export is a `Persona`.
// The runner finds them by id, so adding a profile means adding a file:
//
//   scripts/e2e-explore.sh --list                 # the profiles in e2e/personas/
//   scripts/e2e-explore.sh impatient              # explore as one
//   scripts/e2e-explore.sh --persona-file draft.ts   # a profile not in the folder yet
//
// Statements follow the rules that made judgements reliable in #37: one idea
// each, plain words, judged on cleaned page text. See docs/testing.md,
// "Exploratory testing".
import type { Page } from "@playwright/test";
import { existsSync, readdirSync } from "node:fs";
import { isAbsolute, resolve } from "node:path";
import { pathToFileURL } from "node:url";

/**
 * A Noul statement judged on a page. A plain string is judged on every page
 * visited; `on` (a path pattern) and `when` (a testid that must be visible)
 * limit it to the pages where it makes sense.
 */
export type Statement = string | { text: string; on?: RegExp; when?: string };

/** Exact checks of the page itself, no model: what text can't show. */
export type CheckId =
  | "overflow" //       content wider than the viewport (text runs off a phone screen); always on at phone width
  | "labels" //         buttons and links whose label says nothing ("▾", "✕") and have no aria-label or title
  | "class-state" //    state (sel, done, right, wrong…) shown only in classes, not in an attribute
  | "hidden-menus" //   links that only appear after opening a closed menu, and nowhere else on the page
  | "error-role"; //    visible error messages that aren't role=alert

/** Pairs of items on a page judged one by one (the `accurate` profile's content audit). */
export interface ItemProbe {
  /** Rows with this testid; their cells become the fields of each item. */
  testid: string;
  /** Names for the cells (td) in order; extra cells are dropped. */
  cells: string[];
  /** Judged for each item; `item` names that item's fields. */
  statement: string;
  /** Items judged per page (default 6): a sample, not the whole table. */
  max?: number;
}

export interface PersonaGoal {
  /** The goal in plain words, as for a journey. */
  goal: string;
  /** A statement Jev judges true once the goal is reached. */
  done: string;
  /** Where to start (default "/"). */
  start?: string;
  /** Exact end-state check; when given it decides whether the goal was reached. */
  check?: (page: Page) => Promise<boolean>;
  /** What to type: testid -> text ("*" for any box). Added to the profile's `inputs`. */
  inputs?: Record<string, string>;
  /** A topic the goal is about; judged statements can name it as `topic`. */
  topic?: string;
  /**
   * Exact work after the goal is reached, for what Jev can't do itself
   * (finish a Sprint mode session). `look` observes the page as it is now.
   */
  then?: (page: Page, look: (why: string) => Promise<void>) => Promise<void>;
}

export type Viewport = "phone" | "desktop";
export const VIEWPORTS: Record<Viewport, { width: number; height: number }> = {
  phone: { width: 390, height: 844 },
  desktop: { width: 1280, height: 800 },
};

export interface Persona {
  /** Stable kebab-case id, the same as the file name: the runner's argument and the report folder name. */
  id: string;
  /** Put into every step's state, so each choice is made as this person. */
  who: string;
  /** Added to the Choice instructions: tie-breakers between actions (and between sections, with --wander). */
  prefers: string;
  /** Default steps per goal; also how many no-effect steps before the persona counts as lost. */
  patience: number;
  /** Viewports; with two, goals alternate between them (phone first). */
  viewports: Viewport[];
  /** Open goals for exploratory runs. */
  goals: PersonaGoal[];
  /** Should hold on the pages visited; a low probability is an "unclear" finding. */
  concerns: Statement[];
  /** Would make this person leave; a high probability is a "frustrating" finding. */
  frustrations: Statement[];
  /** Exact page checks to run on every page visited. */
  checks?: CheckId[];
  /** Item-by-item content checks. */
  items?: ItemProbe[];
  /** Testids this person never touches, on top of the built-in never-click list. */
  avoid?: string[];
  /** What this person types: testid -> text ("*" for any box). A goal's `inputs` win. */
  inputs?: Record<string, string>;
}

const DIR = new URL("../personas/", import.meta.url);

/** The ids of the profiles in e2e/personas/. */
export function personaIds(): string[] {
  return readdirSync(DIR)
    .filter((f) => f.endsWith(".ts") && !f.startsWith("_"))
    .map((f) => f.slice(0, -3))
    .sort();
}

/**
 * A profile by id (a file in e2e/personas/) or by path to a .ts file
 * (relative paths resolve from the repo root, where the scripts run).
 */
export async function loadPersona(idOrPath: string): Promise<Persona> {
  const isPath = idOrPath.endsWith(".ts") || idOrPath.includes("/");
  let url: URL;
  if (isPath) {
    const path = isAbsolute(idOrPath) ? idOrPath : resolve(new URL("../..", import.meta.url).pathname, idOrPath);
    if (!existsSync(path)) throw new Error(`no persona file ${path}`);
    url = pathToFileURL(path);
  } else {
    url = new URL(`${idOrPath}.ts`, DIR);
    if (!existsSync(url)) throw new Error(`no persona "${idOrPath}"; try one of: ${personaIds().join(", ")}`);
  }
  // A file outside e2e/ (no "type": "module" above it) loads as CommonJS: its default export comes wrapped once more.
  const mod = (await import(url.href)) as { default?: Persona & { default?: Persona } };
  const persona = mod.default?.default ?? mod.default;
  if (!persona?.id || !persona.who || !Array.isArray(persona.goals)) {
    throw new Error(`${url.pathname}: the default export must be a Persona (id, who, prefers, patience, viewports, goals, concerns, frustrations)`);
  }
  if (!isPath && persona.id !== idOrPath) throw new Error(`${url.pathname}: id "${persona.id}" must match the file name "${idOrPath}"`);
  return persona;
}
