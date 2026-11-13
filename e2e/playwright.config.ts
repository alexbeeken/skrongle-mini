// Browser tests against a running dev server. scripts/e2e.sh starts one
// (fresh database, seeded dev user, LLM fixtures) and sets E2E_BASE_URL;
// without it the tests use ./.dev-start's URL file.
import { defineConfig } from "@playwright/test";
import { readFileSync } from "node:fs";

function baseURL(): string {
  if (process.env.E2E_BASE_URL) return process.env.E2E_BASE_URL;
  try {
    return readFileSync(new URL("../.dev-server.url", import.meta.url), "utf8").trim();
  } catch {
    throw new Error("No server: run scripts/e2e.sh, or set E2E_BASE_URL, or start ./.dev-start --seed --llm fixtures --background");
  }
}

// Existing specs that never touch /dev and pass against a release server
// started with SKRONGLE_SIGNUP=open (they register their own accounts), as
// many as fit the 8 accounts a server holds. docs/testing.md, "Release specs".
const RELEASE_OPEN = [
  "landing-try",
  "mistakes",
  "profile-nav",
  "smoke/motivator",
  "smoke/sprint-sort",
  "smoke/sprint-check-error",
].map((f) => new RegExp(`/specs/${f}\\.spec\\.ts$`));

export default defineConfig({
  testDir: "./specs",
  // One dev user is shared by every spec, so run them one at a time.
  workers: 1,
  fullyParallel: false,
  timeout: 60_000,
  expect: { timeout: 10_000 },
  retries: 0,
  outputDir: "./results/artifacts",
  reporter: [
    ["list"],
    ["json", { outputFile: "./results/last-run.json" }],
    ["html", { outputFolder: "./results/report", open: "never" }],
  ],
  use: {
    baseURL: baseURL(),
    headless: true,
    actionTimeout: 5_000,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  projects: [
    // The dev specs, as two suites that CI runs in shards (#203): `exact` is
    // every spec outside specs/smoke and specs/release, `smoke` is
    // specs/smoke. scripts/e2e.sh runs both. A new spec file lands in one of
    // them by where it is; lib/suites.test.ts fails if one is in neither.
    // The release specs need a server of their own (invite-only, an owner
    // link), so scripts/e2e.sh does not run them.
    { name: "exact", testIgnore: ["**/release/**", "**/smoke/**"], use: { browserName: "chromium" } },
    { name: "smoke", testDir: "./specs/smoke", use: { browserName: "chromium" } },
    // Release QA (#40): what a self-hoster has, against whatever E2E_BASE_URL points at.
    // `npx playwright test --project release`; scripts/release-e2e.sh starts a server for it.
    // The first-run spec makes the owner through the owner link, so it runs before the rest.
    { name: "release-first-run", testDir: "./specs/release", testMatch: "first-run.spec.ts", use: { browserName: "chromium" } },
    {
      name: "release",
      dependencies: ["release-first-run"],
      testDir: "./specs",
      testMatch: /release\/(?!first-run).*\.spec\.ts$/,
      use: { browserName: "chromium" },
    },
    // Dev-free specs of the other folders; the server needs SKRONGLE_SIGNUP=open.
    { name: "release-open", testDir: "./specs", testMatch: RELEASE_OPEN, use: { browserName: "chromium" } },
  ],
});
