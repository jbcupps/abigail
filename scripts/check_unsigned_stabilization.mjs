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
  releaseFast.includes(
    "cargo build --release -p hive-daemon -p entity-daemon -p abigail-hive-app -p abigail-entity-runtime-app"
  ),
  "Unsigned stabilization workflow must build the full split product."
);
for (const requiredAsset of [
  "hive-daemon-windows-x64.exe",
  "entity-daemon-windows-x64.exe",
  "hive-daemon-linux-x64",
  "entity-daemon-linux-x64",
]) {
  assert(
    releaseFast.includes(requiredAsset),
    `Unsigned stabilization workflow must ship side-by-side daemon binary ${requiredAsset}.`
  );
}
assert(
  releaseFast.includes("publish_stable_release"),
  "Unsigned stabilization workflow must be able to publish a corrected stable split-product release."
);

const release = fs.readFileSync(".github/workflows/release.yml", "utf8");
assert(
  release.includes("tags:"),
  "Beta/release lane must stay explicit via tags or manual dispatch."
);
assert(
  release.includes("branches:") && release.includes("- beta"),
  "Full installer release must trigger from the permanent beta branch."
);
assert(
  release.includes('-beta.${{ github.run_number }}'),
  "Beta installer releases must receive run-numbered beta tags."
);
assert(
  release.includes("prerelease: ${{ needs.build.outputs.prerelease }}"),
  "Beta installer releases must be published as prereleases."
);
for (const forbidden of [
  "cd tauri-app",
  "tauri-app/src-ui",
  "tauri-app/tauri.conf.json",
]) {
  assert(
    !release.includes(forbidden),
    `Full installer release must not build the legacy tauri-app path (${forbidden}).`
  );
}
for (const required of [
  "hive-app/tauri.conf.json",
  "stage_split_installer_resources.ps1",
  "hive-app/resources/abigail-entity-runtime-app.exe",
  "hive-app/resources/hive-daemon.exe",
  "hive-app/resources/entity-daemon.exe",
]) {
  assert(
    release.includes(required),
    `Full installer release must stage the split Abigail product (${required}).`
  );
}
assert(
  release.includes("resources\\\\hive-daemon.exe"),
  "Beta installer verification must assert the installed resources path for hive-daemon.exe."
);
assert(
  release.includes("Verify packaged frontend assets"),
  "Beta installer verification must assert packaged frontend assets are relative and include startup video."
);

for (const app of ["hive-app", "entity-runtime-app"]) {
  const viteConfig = fs.readFileSync(`${app}/src-ui/vite.config.ts`, "utf8");
  assert(
    viteConfig.includes('base: "./"'),
    `${app} Vite config must emit relative asset paths for packaged Tauri WebViews.`
  );
  const splash = fs.readFileSync(`${app}/src-ui/src/components/SplashScreen.tsx`, "utf8");
  assert(
    splash.includes('src="./video/startup.mp4"'),
    `${app} splash video must use a relative packaged asset path.`
  );
}

const prereqs = fs.readFileSync("scripts/enforce_release_prereqs.sh", "utf8");
assert(
  prereqs.includes("Release prerequisite enforcement skipped"),
  "Release prerequisite script must be able to skip signing enforcement when disabled."
);

console.log("Unsigned stabilization lane checks passed.");
