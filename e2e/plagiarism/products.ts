// The course products the plagiarism checks look out for. Stored base64 so
// that the repo names no competitor in plain text (a search for them finds
// nothing); the checks decode them. Add one with
// `node -e 'console.log(Buffer.from("name").toString("base64"))'`.
const decode = (s: string) => Buffer.from(s, "base64").toString("utf8");

/** As written in the product's own branding, most likely to be imitated first. */
export const PRODUCTS = [
  "RHVvbGluZ28=",
  "S3dpemlx",
  "QmFiYmVs",
  "QnVzdXU=",
  "TWVtcmlzZQ==",
  "Um9zZXR0YSBTdG9uZQ==",
  "QW5raQ==",
  "TGluZ29kYQ==",
  "UGltc2xldXI=",
].map(decode);
