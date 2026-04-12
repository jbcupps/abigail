#!/usr/bin/env node

import fs from "node:fs";

function readJson(path) {
  return JSON.parse(fs.readFileSync(path, "utf8"));
}

function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

const tauriConfig = readJson("tauri-app/tauri.conf.json");
assert(
  tauriConfig.bundle?.createUpdaterArtifacts === false,
  "Unsigned stabilization lane must keep createUpdaterArtifacts disabled by default."
);
assert(
  !tauriConfig.plugins?.updater,
  "Unsigned stabilization lane must not ship updater config in tauri.conf.json."
);

const nsisHooks = fs.readFileSync("tauri-app/nsis-hooks.nsh", "utf8");
for (const forbidden of [
  "BackupUserData",
  "RestoreUserData",
  "CheckForExistingInstall",
  "ShowUpgradeDialog",
  "abigail_upgrade_backup",
]) {
  assert(
    !nsisHooks.includes(forbidden),
    `NSIS hooks must not retain alpha upgrade preservation logic (${forbidden}).`
  );
}

const releaseFast = fs.readFileSync(".github/workflows/release-fast.yml", "utf8");
assert(
  releaseFast.includes("workflow_dispatch:"),
  "Stabilization build lane must stay manual/opt-in."
);
assert(
  !releaseFast.includes("\n  push:\n"),
  "Stabilization build lane must not trigger automatically."
);
for (const forbidden of [
  "TAURI_UPDATER_PUBKEY",
  "TAURI_SIGNING_PRIVATE_KEY",
  "windows_signing_preflight",
  "generate_tauri_latest_manifest",
  "createUpdaterArtifacts must be true",
]) {
  assert(
    !releaseFast.includes(forbidden),
    `Unsigned stabilization workflow must not require updater/signing logic (${forbidden}).`
  );
}
assert(
  releaseFast.includes("cargo build --release -p abigail-hive-app -p abigail-entity-runtime-app"),
  "Unsigned stabilization workflow must build the split Hive and Entity Runtime apps."
);

const release = fs.readFileSync(".github/workflows/release.yml", "utf8");
assert(
  release.includes("tags:"),
  "Beta/release lane must stay explicit via tags or manual dispatch."
);

const prereqs = fs.readFileSync("scripts/enforce_release_prereqs.sh", "utf8");
assert(
  prereqs.includes("Release prerequisite enforcement skipped"),
  "Release prerequisite script must be able to skip signing enforcement when disabled."
);

console.log("Unsigned stabilization lane checks passed.");
