// Playthrough: an end-to-end run through a whole course in one mode, with
// Jev as the learner. Run by scripts/jev-playthrough.sh (and its -sprint, -study, -play,
// -all wrappers); see docs/testing.md, "Playthrough". The loop is core.ts,
// the modes are modes.ts, the playing is player.ts, the bug signals watch.ts.
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { jevEnabled, SKIP_REASON } from "../lib/jev.ts";
import { run } from "./core.ts";
import { MODES } from "./modes.ts";
import type { Kind } from "./watch.ts";

const REPO = fileURLToPath(new URL("../..", import.meta.url));
const USAGE = `usage: scripts/jev-playthrough.sh <${Object.keys(MODES).join("|")}> [options]

  --lessons N        stop after N lessons (default: all)
  --attempts N       tries per lesson before it is failed and passed artificially (default 2)
  --max-calls N      stop calling Jev after N calls (default 8000)
  --course ID        the course to open (default spanish)
  --fail-on KINDS    finding kinds that make the run exit 1 (default bug,invariant; also stuck; none)
  --trace            write the Jev traces of every lesson (default: failed lessons only)
  --headed           show the browser
  --keep             leave the server running afterwards
  --url URL          play on a running server (no checkpoints unless PLAYTHROUGH_DB is set)`;

function die(msg: string): never {
  console.error(msg);
  process.exit(2);
}

const a = process.argv.slice(2);
const mode = MODES[a[0] ?? ""] ?? die(USAGE);
const o = {
  base: (process.env.PLAYTHROUGH_URL ?? "").replace(/\/$/, ""),
  course: "spanish",
  lessons: Infinity,
  attempts: 2,
  maxCalls: 8000,
  headed: false,
  out: process.env.PLAYTHROUGH_OUT || join(REPO, "e2e", "results", "playthrough", `${new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19)}-${mode.id}`),
  db: process.env.PLAYTHROUGH_DB ?? "",
  failOn: ["bug", "invariant"] as Kind[],
  traceAll: false,
  repo: REPO,
};
for (let i = 1; i < a.length; i++) {
  const val = () => a[++i] ?? die(`${a[i - 1]} needs a value\n\n${USAGE}`);
  const num = () => Number(val()) || die(`${a[i]} is not a number`);
  switch (a[i]) {
    case "-h": case "--help": console.log(USAGE); process.exit(0);
    case "--lessons": o.lessons = num(); break;
    case "--attempts": o.attempts = num(); break;
    case "--max-calls": o.maxCalls = num(); break;
    case "--course": o.course = val(); break;
    case "--fail-on": { const v = val(); o.failOn = v === "none" ? [] : (v.split(",") as Kind[]); break; }
    case "--headed": o.headed = true; break;
    case "--trace": o.traceAll = true; break;
    case "--url": o.base = val().replace(/\/$/, ""); break;
    case "--keep": break; // the shell script's
    default: die(`unknown option ${a[i]}\n\n${USAGE}`);
  }
}
if (!jevEnabled) die(SKIP_REASON);
if (!o.base) die("no server: run scripts/jev-playthrough.sh, or pass --url");
process.exitCode = await run(mode, o);
