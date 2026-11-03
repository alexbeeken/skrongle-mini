// TOTP (RFC 6238) worked out independently of the app, with Node's crypto,
// so a spec can check the codes the dev authenticator (/dev/authenticator)
// shows, or type one into a sign-in, without trusting src/totp.rs.
import { createHmac } from "node:crypto";
import { expect, request as playwrightRequest, test, type APIRequestContext } from "@playwright/test";

let dev: Promise<APIRequestContext> | undefined;

/** Requests to the dev pages (no sign-in), a context of their own made once per worker. */
function devRequest(): Promise<APIRequestContext> {
  dev ??= playwrightRequest.newContext({ baseURL: test.info().project.use.baseURL });
  return dev;
}

const B32 = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

export function base32Decode(s: string): Buffer {
  let bits = 0;
  let value = 0;
  const out: number[] = [];
  for (const c of s.toUpperCase().replace(/[\s=-]/g, "")) {
    const v = B32.indexOf(c);
    if (v < 0) throw new Error(`not base32: ${c}`);
    value = ((value << 5) | v) & 0xffff;
    bits += 5;
    if (bits >= 8) {
      bits -= 8;
      out.push((value >> bits) & 0xff);
    }
  }
  return Buffer.from(out);
}

export function base32Encode(b: Buffer): string {
  let bits = 0;
  let value = 0;
  let out = "";
  for (const byte of b) {
    value = (value << 8) | byte;
    bits += 8;
    while (bits >= 5) {
      bits -= 5;
      out += B32[(value >> bits) & 31];
    }
    value &= 0xff;
  }
  if (bits > 0) out += B32[(value << (5 - bits)) & 31];
  return out;
}

/** The HOTP code for a time step (SHA-1 unless told otherwise). */
export function totpForStep(secretBase32: string, step: number, digits = 6, algorithm = "sha1"): string {
  const counter = Buffer.alloc(8);
  counter.writeBigUInt64BE(BigInt(step));
  const mac = createHmac(algorithm, base32Decode(secretBase32)).update(counter).digest();
  const offset = mac[mac.length - 1] & 0x0f;
  const bin = mac.readUInt32BE(offset) & 0x7fffffff;
  return String(bin % 10 ** digits).padStart(digits, "0");
}

/** The code at a moment (unix seconds; default now). */
export function totp(secretBase32: string, unixTime = Date.now() / 1000, period = 30): string {
  return totpForStep(secretBase32, Math.floor(unixTime / period));
}

export interface AuthAccount {
  id: number;
  issuer: string;
  account: string;
  code: string;
  remaining: number;
  period: number;
  digits: number;
  algorithm: string;
  secret: string;
}

/** What the dev authenticator holds, with the codes it shows now. */
export async function authenticator(): Promise<{ now: number; accounts: AuthAccount[] }> {
  const r = await (await devRequest()).get("/dev/authenticator", { params: { format: "json" } });
  expect(r.status(), "GET /dev/authenticator").toBe(200);
  return r.json();
}

/** A code from the dev authenticator for `account` (an address) with at
 * least `minLeft` seconds to run, so a sign-in typed with it is not refused
 * for having just expired. */
export async function authenticatorCode(account: string, minLeft = 5): Promise<string> {
  let code = "";
  await expect
    .poll(async () => {
      const a = (await authenticator()).accounts.find((x) => x.account === account);
      if (!a || a.remaining < minLeft) return false;
      code = a.code;
      return true;
    }, { message: `no code for ${account} in the dev authenticator`, timeout: 15_000 })
    .toBe(true);
  return code;
}
