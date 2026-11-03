// Set the clock (#1340): set a clock face (static/clock.js) with its keys, as
// a keyboard user would. Page Up moves an hour, the up arrow one step.
import { expect, type Locator } from "@playwright/test";

/** Minutes after 12:00 of a time "H:MM". */
export function minutesOf(time: string): number {
  const [h, m] = time.split(":").map(Number);
  return (h % 12) * 60 + m;
}

/** Set the clock in `box` (the element holding it) to `time` ("H:MM"). */
export async function setClock(box: Locator, time: string): Promise<void> {
  const face = box.locator("[role=slider]");
  const step = Number(await face.getAttribute("data-step"));
  const now = Number(await face.getAttribute("aria-valuenow"));
  const diff = (minutesOf(time) - now + 720) % 720;
  await face.focus();
  for (let i = 0; i < Math.floor(diff / 60); i++) await face.press("PageUp");
  for (let i = 0; i < (diff % 60) / step; i++) await face.press("ArrowUp");
  await expect(face, `the clock shows ${time}`).toHaveAttribute("aria-valuenow", String(minutesOf(time)));
}

/** A time the clock can show that is not `time`: an hour later. */
export function hourLater(time: string): string {
  const [h, m] = time.split(":");
  return `${(Number(h) % 12) + 1}:${m}`;
}
