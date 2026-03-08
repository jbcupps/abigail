#!/usr/bin/env node

import fs from "node:fs";

const configPath = process.argv[2] ?? "tauri-app/tauri.conf.json";

function isTruthy(value) {
  return /^(1|true|yes)$/i.test(String(value ?? "").trim());
}

function decodeBase64Text(value, label) {
  const normalized = String(value ?? "").trim().replace(/\s+/g, "");
  if (!normalized) {
    throw new Error(`${label} is empty`);
  }

  const decoded = Buffer.from(normalized, "base64");
  if (decoded.length === 0) {
    throw new Error(`${label} is not valid base64`);
  }

  const roundTrip = decoded.toString("base64").replace(/=+$/g, "");
  if (roundTrip !== normalized.replace(/=+$/g, "")) {
    throw new Error(`${label} is not valid base64`);
  }

  const text = decoded.toString("utf8").trim();
  if (!text) {
    throw new Error(`${label} did not decode to UTF-8 text`);
  }
  return text;
}

const requireUpdaterPubkey = isTruthy(process.env.ABIGAIL_REQUIRE_UPDATER_PUBKEY);
const enableUpdaterArtifacts = isTruthy(process.env.ABIGAIL_ENABLE_UPDATER_ARTIFACTS);
const updaterPubkey = String(process.env.TAURI_UPDATER_PUBKEY ?? "").trim();
const windowsThumbprint = String(process.env.WINDOWS_CERTIFICATE_THUMBPRINT ?? "").trim();
const windowsTimestampUrl = String(process.env.WINDOWS_TIMESTAMP_URL ?? "").trim();

if (!fs.existsSync(configPath)) {
  throw new Error(`Tauri config not found: ${configPath}`);
}

if (requireUpdaterPubkey && !updaterPubkey) {
  throw new Error("TAURI_UPDATER_PUBKEY is required for this build.");
}

if (updaterPubkey) {
  const decodedPubkey = decodeBase64Text(updaterPubkey, "TAURI_UPDATER_PUBKEY");
  if (!decodedPubkey.startsWith("untrusted comment:")) {
    throw new Error(
      "TAURI_UPDATER_PUBKEY must decode to a minisign public key box that starts with 'untrusted comment:'."
    );
  }
}

const raw = fs.readFileSync(configPath, "utf8");
const config = JSON.parse(raw);
config.bundle ??= {};
config.bundle.windows ??= {};
config.plugins ??= {};
config.plugins.updater ??= {};

const createUpdaterArtifacts = Boolean(updaterPubkey) && enableUpdaterArtifacts;
config.bundle.createUpdaterArtifacts = createUpdaterArtifacts;

if (updaterPubkey) {
  config.plugins.updater.pubkey = updaterPubkey;
}

if (windowsThumbprint) {
  config.bundle.windows.certificateThumbprint = windowsThumbprint;
}

if (windowsTimestampUrl) {
  config.bundle.windows.timestampUrl = windowsTimestampUrl;
}

fs.writeFileSync(configPath, `${JSON.stringify(config, null, 2)}\n`);

console.log(
  JSON.stringify(
    {
      configPath,
      createUpdaterArtifacts,
      hasUpdaterPubkey: Boolean(updaterPubkey),
      windowsCertificateThumbprint: config.bundle.windows.certificateThumbprint ?? null,
      windowsTimestampUrl: config.bundle.windows.timestampUrl ?? "",
    },
    null,
    2
  )
);
