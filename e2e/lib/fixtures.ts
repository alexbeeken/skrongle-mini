// The smoke specs' `test`: every page starts with a seeded Math.random and
// a fake voice, so a run replays exactly and the listening exercises are on
// offer in headless Chromium (which has no voices). The seed is E2E_SEED
// (scripts/e2e.sh gives the server the same one as SKRONGLE_RNG_SEED) and is
// printed when a test fails, so the failure can be replayed:
//   E2E_SEED=<n> scripts/e2e.sh --grep "<test>"
import { test as base, expect, type Page } from "@playwright/test";

export const SEED = Number(process.env.E2E_SEED ?? 1) || 1;

/** Run before any page script: a seeded PRNG (mulberry32) for Math.random, and speech that says nothing at once. */
function browserInit({ seed, langs }: { seed: number; langs: string[] }) {
  let s = seed >>> 0;
  Math.random = () => {
    s = (s + 0x6d2b79f5) >>> 0;
    let t = s;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  const voices = langs.map((lang) => ({ lang, name: `Test ${lang}`, voiceURI: `test-${lang}`, default: false, localService: true }));
  class Utterance {
    text: string;
    lang = "";
    rate = 1;
    voice: unknown = null;
    onstart: (() => void) | null = null;
    onend: (() => void) | null = null;
    onerror: ((e: unknown) => void) | null = null;
    constructor(text: string) {
      this.text = text;
    }
  }
  const synth = {
    speaking: false,
    pending: false,
    paused: false,
    getVoices: () => voices,
    speak(u: Utterance) {
      setTimeout(() => {
        u.onstart?.();
        u.onend?.();
      }, 0);
    },
    cancel() {},
    pause() {},
    resume() {},
    addEventListener() {},
    removeEventListener() {},
    onvoiceschanged: null,
  };
  Object.defineProperty(window, "speechSynthesis", { value: synth, configurable: true });
  Object.defineProperty(window, "SpeechSynthesisUtterance", { value: Utterance, configurable: true });
}

/** Seed `page` and give it a voice for each of `langs`; call before the first goto. */
export async function seedPage(page: Page, langs = ["es", "en"]): Promise<void> {
  await page.addInitScript(browserInit, { seed: SEED, langs });
}

export const test = base.extend<{ seeded: void }>({
  seeded: [
    async ({ page }, use, info) => {
      await seedPage(page);
      info.annotations.push({ type: "seed", description: String(SEED) });
      // A page that answers with an error fails the test by name, whatever the step that tripped over it.
      const broken: string[] = [];
      page.on("response", (r) => {
        if (r.status() >= 400 && r.request().isNavigationRequest() && r.request().frame() === page.mainFrame())
          broken.push(`HTTP ${r.status()} opening ${new URL(r.url()).pathname}`);
      });
      await use();
      if (info.status !== info.expectedStatus) console.log(`   seed ${SEED}: replay with E2E_SEED=${SEED} scripts/e2e.sh --grep "${info.title}"`);
      expect(broken, "pages that answered with an HTTP error").toEqual([]);
    },
    { auto: true },
  ],
});
export { expect };
