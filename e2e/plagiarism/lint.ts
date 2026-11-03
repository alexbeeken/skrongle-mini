// Briefs must never tell an agent (or an LLM generating a course) to imitate
// a specific product's text. Free, no key. A line that names a course product
// (products.ts) next to an imitation word, or says "X-style", fails, unless it
// is on the reviewed list.
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { PRODUCTS } from "./products.ts";

const PRODUCT = `(?:${PRODUCTS.map((p) => p.toLowerCase().replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("|")})`;
const IMITATE = "(?:cop(?:y|ie[sd]|ying)|imitat\\w*|mimic\\w*|clon\\w*|replicat\\w*|reproduc\\w*|paraphras\\w*|style|modell?ed|in the spirit of|inspired by|based on|like|à la|a la)";
const RULES = [
  new RegExp(`${PRODUCT}[^.\\n]{0,40}\\b${IMITATE}\\b|\\b${IMITATE}\\b[^.\\n]{0,40}${PRODUCT}`, "i"),
  new RegExp(`${PRODUCT}[-\\s]style`, "i"),
  new RegExp(`${PRODUCT}['’]s\\b`, "i"),
];

const SKIP = new Set(["node_modules", "target", ".git", "results"]);
function files(dir: string, ok: (n: string) => boolean): string[] {
  if (!existsSync(dir)) return [];
  return readdirSync(dir).sort().flatMap((n) => {
    if (SKIP.has(n)) return [];
    const p = join(dir, n);
    return statSync(p).isDirectory() ? files(p, ok) : ok(n) ? [p] : [];
  });
}

/** Reviewed lines live in scripts/provenance-lint-ok.txt: `file: text of the line`. */
export function lint(repo: string): string[] {
  const okFile = join(repo, "scripts", "provenance-lint-ok.txt");
  const ok = new Set(existsSync(okFile) ? readFileSync(okFile, "utf8").split("\n").map((s) => s.replace(/\s+#.*$/, "").trim()).filter(Boolean) : []);
  const targets = [
    ...["README.md", "CLAUDE.md"].map((n) => join(repo, n)).filter(existsSync),
    ...files(join(repo, ".claude", "rules"), (n) => n.endsWith(".md")),
    ...files(join(repo, "docs"), (n) => n.endsWith(".md")),
    ...files(join(repo, "data"), (n) => n === "AUTHORING.md" || n.endsWith(".md")),
    ...files(join(repo, "scripts"), (n) => /\.(sh|md)$/.test(n)),
    ...files(join(repo, "src"), (n) => n.endsWith(".rs")),
  ];
  const bad: string[] = [];
  for (const f of targets) {
    const rel = relative(repo, f);
    readFileSync(f, "utf8").split("\n").forEach((line, i) => {
      if (RULES.some((r) => r.test(line)) && !ok.has(`${rel}: ${line.trim()}`))
        bad.push(`${rel}:${i + 1}: ${line.trim().slice(0, 200)}`);
    });
  }
  return bad;
}
