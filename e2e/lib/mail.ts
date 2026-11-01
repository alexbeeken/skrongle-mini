// The dev mailbox (src/mail/sink.rs): in dev mode the app sends its mail over
// SMTP to a small server of its own, and /dev/inbox shows what arrived. It
// needs no sign-in (it is for whoever is testing, every account's mail), so
// these helpers read it with a request context of their own: the learner a
// spec drives is not touched.
import { expect, request as playwrightRequest, test, type APIRequestContext, type Browser, type Page } from "@playwright/test";

let dev: Promise<APIRequestContext> | undefined;

/** Requests to the dev mailbox, a context of their own made once per worker. */
export function devRequest(): Promise<APIRequestContext> {
  dev ??= playwrightRequest.newContext({ baseURL: test.info().project.use.baseURL });
  return dev;
}

/** A page of its own (a new browser context, signed out) opened at `path` in the dev mailbox. */
export async function devPage(browser: Browser, path = "/dev/inbox"): Promise<Page> {
  const page = await (await browser.newContext()).newPage();
  await page.goto(path);
  return page;
}

export interface InboxMail {
  id: number;
  outbox_id: number | null;
  envelope_to: string[];
  from: string;
  to: string;
  subject: string;
  text: string;
  html: string;
  links: string[];
}

/** What the dev mailbox holds, newest first; only mail to `to` when given. */
export async function inbox(to?: string): Promise<InboxMail[]> {
  const r = await (await devRequest()).get("/dev/inbox", { params: { format: "json", ...(to ? { to } : {}) } });
  expect(r.status(), "GET /dev/inbox").toBe(200);
  return (await r.json()).messages;
}

/** The newest message to `to` (whose subject matches, if given), waiting for
 * the job runner to deliver it. */
export async function waitForMail(to: string, subject?: RegExp): Promise<InboxMail> {
  let found: InboxMail | undefined;
  await expect
    .poll(async () => {
      found = (await inbox(to)).find((m) => !subject || subject.test(m.subject));
      return found !== undefined;
    }, { message: `no mail to ${to}${subject ? ` matching ${subject}` : ""} in /dev/inbox` })
    .toBe(true);
  return found!;
}

/** The message's link whose URL matches `pattern`. */
export function mailLink(mail: InboxMail, pattern: RegExp): string {
  const link = mail.links.find((l) => pattern.test(l));
  if (!link) throw new Error(`no link matching ${pattern} in "${mail.subject}": ${mail.links.join(", ")}`);
  return link;
}
