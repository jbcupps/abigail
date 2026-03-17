#!/usr/bin/env node

function fail(message) {
  console.error(`ERROR: ${message}`);
  process.exit(1);
}

function decodeBase64Binary(value, label) {
  const normalized = String(value ?? "").trim().replace(/\s+/g, "");
  if (!normalized) {
    fail(`${label} is empty.`);
  }

  let decoded;
  try {
    decoded = Buffer.from(normalized, "base64");
  } catch {
    fail(`${label} is not valid base64.`);
  }

  if (!decoded.length) {
    fail(`${label} is not valid base64.`);
  }

  const roundTrip = decoded.toString("base64").replace(/=+$/g, "");
  if (roundTrip !== normalized.replace(/=+$/g, "")) {
    fail(`${label} is not valid base64.`);
  }

  return decoded;
}

function decodeBase64Text(value, label) {
  const decoded = decodeBase64Binary(value, label).toString("utf8").trim();
  if (!decoded) {
    fail(`${label} did not decode to UTF-8 text.`);
  }
  return decoded;
}

function encodeBase64Text(value) {
  return Buffer.from(String(value ?? ""), "utf8").toString("base64");
}

function normalizeMultilineKey(value) {
  let normalized = String(value ?? "");
  if (normalized.includes("\\n")) {
    normalized = normalized.replace(/\\n/g, "\n");
  }
  return normalized.trim();
}

function validateMultilineKey(value) {
  const normalized = normalizeMultilineKey(value);
  const lines = normalized.split(/\r?\n/).filter(Boolean);
  const line1 = lines[0] ?? "";
  const line2 = (lines[1] ?? "").replace(/\s+/g, "");

  if (!line1.startsWith("untrusted comment:")) {
    fail("TAURI_SIGNING_PRIVATE_KEY first line must start with 'untrusted comment:'.");
  }

  if (!line2.startsWith("RW")) {
    fail("TAURI_SIGNING_PRIVATE_KEY second line must start with 'RW'.");
  }

  decodeBase64Binary(line2, "TAURI_SIGNING_PRIVATE_KEY line 2");
  return `${line1}\n${line2}\n`;
}

const keyRaw = String(process.env.TAURI_SIGNING_PRIVATE_KEY ?? "");
const keyPassword = String(process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD ?? "");

if (!keyRaw.trim()) {
  fail("TAURI_SIGNING_PRIVATE_KEY is empty or missing.");
}

if (!keyPassword.trim()) {
  fail("TAURI_SIGNING_PRIVATE_KEY_PASSWORD is empty or missing.");
}

let multilineKey;
if (
  keyRaw.includes("\n") ||
  keyRaw.includes("\\n") ||
  keyRaw.trim().startsWith("untrusted comment:")
) {
  multilineKey = validateMultilineKey(keyRaw);
} else {
  multilineKey = validateMultilineKey(
    decodeBase64Text(keyRaw, "TAURI_SIGNING_PRIVATE_KEY")
  );
}

process.stdout.write(encodeBase64Text(multilineKey));
