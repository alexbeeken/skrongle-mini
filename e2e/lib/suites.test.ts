// Every dev spec file runs in CI (#203): each *.spec.ts under e2e/specs
// (outside specs/release, which has its own job) is in exactly one of the
// two suites CI runs in shards, the `exact` and `smoke` projects of
// playwright.config.ts, and the workflow runs both. Asks Playwright itself
// (`--list`), so a testIgnore or testMatch that drops a file fails here, in
// the e2e unit tests (`npm --prefix e2e run unit`; CI: exact shard 3, before
// its browser tests). No server and no browser.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { test } from "node:test";

const E2E = resolve(import.meta.dirname, "..");
const SPECS = join(E2E, "specs");
const SUITES = ["exact", "smoke"];

function specsOnDisk(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const path = join(dir, e.name);
    if (e.isDirectory()) return relative(SPECS, path) === "release" ? [] : specsOnDisk(path);
    return e.name.endsWith(".spec.ts") ? [relative(SPECS, path)] : [];
  });
}

type Suite = { file?: string; specs?: { tests: { projectName: string }[] }[]; suites?: Suite[] };

// file (relative to specs/) -> the projects that list a test of it.
function listed(): Map<string, Set<string>> {
  const out = execFileSync(
    join(E2E, "node_modules", ".bin", "playwright"),
    ["test", "--list", "--reporter=json", ...SUITES.flatMap((p) => ["--project", p])],
    // The config wants a server address; --list never opens it.
    { cwd: E2E, encoding: "utf8", env: { ...process.env, E2E_BASE_URL: "http://127.0.0.1:9" }, maxBuffer: 64 << 20 },
  );
  const report = JSON.parse(out) as { config: { rootDir: string }; suites: Suite[] };
  const files = new Map<string, Set<string>>();
  const walk = (s: Suite, file: string) => {
    for (const spec of s.specs ?? []) {
      for (const t of spec.tests) {
        if (!files.has(file)) files.set(file, new Set());
        files.get(file)!.add(t.projectName);
      }
    }
    for (const child of s.suites ?? []) walk(child, file);
  };
  for (const s of report.suites) walk(s, relative(SPECS, join(report.config.rootDir, s.file ?? "")));
  return files;
}

test("every spec file outside specs/release is in exactly one CI suite", () => {
  const onDisk = specsOnDisk(SPECS).sort();
  assert.ok(onDisk.length > 50, `found only ${onDisk.length} spec files under ${SPECS}`);
  const files = listed();
  const problems = onDisk.flatMap((f) => {
    const projects = [...(files.get(f) ?? [])];
    if (projects.length === 1) return [];
    return [`${f}: ${projects.length ? `in ${projects.join(" and ")}` : "in neither suite (a file with no test, or a project's testIgnore drops it)"}`];
  });
  assert.deepEqual(problems, [], `every spec file must be in exactly one of ${SUITES.join(", ")}:\n${problems.join("\n")}`);
  assert.ok(files.size > 0 && [...files.values()].some((p) => p.has("smoke")), "the smoke project lists nothing");
});

test("the checks workflow runs both suites", () => {
  const workflow = readFileSync(join(E2E, "..", ".github", "workflows", "smoke.yml"), "utf8");
  for (const p of SUITES) {
    assert.match(workflow, new RegExp(`^\\s+specs: --project ${p}\\s*$`, "m"), `.github/workflows/smoke.yml has no e2e job with "specs: --project ${p}"`);
  }
});
