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
  projects: [{ name: "chromium", use: { browserName: "chromium" } }],
});
