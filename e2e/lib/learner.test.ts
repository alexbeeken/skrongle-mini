// newAccount never hands out the same address twice (#867), no browser:
// node --test e2e/lib/ (or `npm --prefix e2e run unit`).
import assert from "node:assert/strict";
import { test } from "node:test";
import { newAccount } from "./learner.ts";

test("10,000 addresses across simulated workers are all different", () => {
  const seen = new Set<string>();
  for (let i = 0; i < 10_000; i++) seen.add(newAccount("smoke", i % 6).email);
  assert.equal(seen.size, 10_000);
});

// The bug itself: each Playwright worker is a fresh process whose counter
// starts at 0. Two fresh copies of the module with the same Math.random
// stand in for two workers; only the worker index keeps them apart.
test("two workers, each counting from 0 with the same luck, never share an address", async () => {
  const random = Math.random;
  Math.random = () => 0.123456789;
  try {
    const fresh = (w: string): Promise<typeof import("./learner.ts")> => import(`${new URL("./learner.ts", import.meta.url)}?worker=${w}`);
    const a = await fresh("a");
    const b = await fresh("b");
    const fromA = Array.from({ length: 50 }, () => a.newAccount("smoke", 0).email);
    const fromB = new Set(Array.from({ length: 50 }, () => b.newAccount("smoke", 1).email));
    assert.deepEqual(fromA.filter((e) => fromB.has(e)), []);
  } finally {
    Math.random = random;
  }
});

test("an address fits the register form's 254-character limit", () => {
  const email = newAccount("a-long-spec-prefix-for-a-learner", 9999).email;
  assert.ok(email.length <= 254, email);
  assert.match(email, /^a-long-spec-prefix-for-a-learner-\d{10}-w9999-\d+-[a-z0-9]+@example\.com$/);
});

test("without a worker index the process id stands in", () => {
  const saved = process.env.TEST_WORKER_INDEX;
  delete process.env.TEST_WORKER_INDEX;
  try {
    assert.match(newAccount("jev").email, new RegExp(`-wp${process.pid}-`));
  } finally {
    if (saved !== undefined) process.env.TEST_WORKER_INDEX = saved;
  }
});
