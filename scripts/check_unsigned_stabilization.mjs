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
  release.includes("resources[\\\\\\\\/]$binary"),
  "Beta installer verification must assert the installed resources path for hive-daemon.exe."
);
assert(
  release.includes("Verify packaged frontend assets"),
  "Beta installer verification must assert packaged frontend assets are relative and include startup video."
);
assert(
  release.includes("data-abigail-app-css") && release.includes("--color-primary"),
  "Beta installer verification must assert Abigail CSS is bundled into the frontend JavaScript."
);

const staging = fs.readFileSync("scripts/stage_split_installer_resources.ps1", "utf8");
const buildArguments = staging.match(/\$cargoArgs\s*=\s*@\(([\s\S]*?)\n\)/)?.[1] ?? "";
for (const required of ["--locked", "abigail-hive-app", "abigail-entity-runtime-app", "--features", "tauri/custom-protocol"]) {
  assert(buildArguments.includes(`"${required}"`), `Installer staging must build ${required} explicitly.`);
}
assert(
  staging.includes("$embeddedAssets.Contains($entry)") && staging.includes("$shellVersion -ne $hiveVersion"),
  "Staging must reject shells that do not embed current frontend entries and the requested installer version."
);
assert(
  /finally\s*\{\s*\$env:TAURI_CONFIG\s*=\s*\$previousTauriConfig/.test(staging),
  "Installer staging must restore the caller's Tauri configuration even when the build fails."
);
assert(
  !release.includes("ref: ${{ github.ref }}") && release.includes("ref: ${{ github.sha }}"),
  "Installer build and publication must use the triggering commit rather than a moving branch."
);
const installedVerification = release.indexOf("- name: Verify installed unsigned payload and synthetic contract");
const installerUpload = release.indexOf("- name: Upload NSIS installer");
assert(
  installedVerification >= 0 && installedVerification < installerUpload &&
    release.includes("./scripts/verify-mvp-windows.ps1") &&
    release.includes("./scripts/tests/run-mvp-acceptance.ps1 -BinaryDir $installDir"),
  "Unsigned releases must verify and exercise the actual installed payload before uploading the installer."
);
assert(
  release.includes("../scripts/nsis-mvp-hooks.nsh"),
  "Unsigned installer acceptance must use the hooks that preserve family data without prompts."
);
const ciGate = release.indexOf("- name: Require successful CI gate for this commit");
const releasePublication = release.indexOf("- name: Create GitHub Release");
assert(
  ciGate >= 0 && ciGate < releasePublication &&
    release.includes('--commit "$RELEASE_SHA"') &&
    release.includes('.name == "gate"') &&
    release.includes('[[ "$TAG_SHA" != "${{ github.sha }}" ]]'),
  "Publication must require the same-commit CI gate and reject an existing tag on another commit."
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
  const main = fs.readFileSync(`${app}/src-ui/src/main.tsx`, "utf8");
  assert(
    main.includes('import appCss from "./index.css?inline"') &&
      main.includes("data-abigail-app-css"),
    `${app} must inject the processed CSS from JavaScript so packaged WebViews cannot render unstyled.`
  );
}

const prereqs = fs.readFileSync("scripts/enforce_release_prereqs.sh", "utf8");
assert(
  prereqs.includes("Release prerequisite enforcement skipped"),
  "Release prerequisite script must be able to skip signing enforcement when disabled."
);

console.log("Unsigned stabilization lane checks passed.");
