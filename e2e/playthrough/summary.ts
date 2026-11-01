// One table for several playthrough runs (scripts/jev-playthrough-all.sh):
// node e2e/playthrough/summary.ts <run folder>… prints Markdown.
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

const rows: string[] = [];
const failed: string[] = [];
let ok = true;
for (const dir of process.argv.slice(2)) {
  const file = join(dir, "report.json");
  if (!existsSync(file)) {
    ok = false;
    rows.push(`| ${dir} | **no report** | – | – | – | – | – |`);
    continue;
  }
  const r = JSON.parse(readFileSync(file, "utf8"));
  const s = r.summary;
  ok &&= s.ok;
  rows.push(`| ${s.title} | ${s.ok ? "✅ OK" : "❌ FAILED"} | ${s.passed_by_jev}/${s.lessons_played} | ${s.failed.length} | ${s.findings} (${s.failing_findings} failing) | $${s.jev.usd} | ${s.minutes} min |`);
  for (const f of r.findings) failed.push(`- ${s.title}: **${f.kind}** ${f.what} (\`${f.url}\`, lesson \`${f.lesson || "–"}\`)`);
  if (s.failed.length) failed.push(`- ${s.title}: Jev failed ${s.failed.map((x: string) => `\`${x}\``).join(", ")}`);
}
console.log(
  [
    `## Playthrough: ${ok ? "OK" : "FAILED"}`,
    "",
    "| Mode | Result | Jev passed | Failed (fudged) | Findings | Jev cost | Time |",
    "|---|---|---|---|---|---|---|",
    ...rows,
    "",
    ...(failed.length ? ["### Findings and failed lessons", "", ...failed] : []),
  ].join("\n"),
);
