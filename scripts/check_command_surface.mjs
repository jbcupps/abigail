#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";

const repoRoot = path.resolve(path.dirname(new URL(import.meta.url).pathname), "..");
const tauriLibPath = path.join(repoRoot, "tauri-app", "src", "lib.rs");
const uiSrcPath = path.join(repoRoot, "tauri-app", "src-ui", "src");
const harnessPath = path.join(uiSrcPath, "browserTauriHarness.ts");

const FRONTEND_EXCLUDE_PATHS = [
  path.join(uiSrcPath, "__tests__"),
  path.join(uiSrcPath, "test"),
  harnessPath,
  path.join(uiSrcPath, "components", "HarnessDebugPanel.tsx"),
];

const HARNESS_ALLOWED_NON_NATIVE = new Set([
  "harness_debug_snapshot",
  "harness_debug_get_traces",
  "harness_debug_set_fault",
  "harness_debug_reset",
  "harness_debug_config",
  "harness_debug_set_provider_validation",
  "plugin:event|listen",
  "plugin:event|unlisten",
  "plugin:event|emit",
  "plugin:event|emit_to",
]);

function readFile(filePath) {
  return fs.readFileSync(filePath, "utf8");
}

function extractNativeCommands(libSource) {
  const match = libSource.match(/generate_handler!\s*\[([\s\S]*?)\]\s*\)/m);
  if (!match) {
    throw new Error("Could not locate tauri::generate_handler![] block in tauri-app/src/lib.rs");
  }
  return new Set(
    match[1]
      .split("\n")
      .map((line) => line.replace(/\/\/.*$/, "").trim())
      .map((line) => line.replace(/,$/, "").trim())
      .filter((line) => line.length > 0)
  );
}

function walkFiles(dirPath, out = []) {
  const entries = fs.readdirSync(dirPath, { withFileTypes: true });
  for (const entry of entries) {
    const fullPath = path.join(dirPath, entry.name);
    if (FRONTEND_EXCLUDE_PATHS.some((excluded) => fullPath === excluded || fullPath.startsWith(`${excluded}${path.sep}`))) {
      continue;
    }
    if (entry.isDirectory()) {
      walkFiles(fullPath, out);
      continue;
    }
    if (entry.isFile() && /\.(ts|tsx)$/.test(entry.name)) {
      out.push(fullPath);
    }
  }
  return out;
}

function extractInvokedCommands(source) {
  const result = [];
  const regex = /(?:invoke|invokeWithTimeout)(?:<[^>]+>)?\s*\(\s*"([a-zA-Z0-9_:\-|]+)"/g;
  let match;
  while ((match = regex.exec(source)) !== null) {
    result.push(match[1]);
  }
  return result;
}

function extractHarnessCommands(source) {
  const result = [];
  const regex = /case\s+"([^"]+)"\s*:/g;
  let match;
  while ((match = regex.exec(source)) !== null) {
    result.push(match[1]);
  }
  return result;
}

function rel(filePath) {
  return path.relative(repoRoot, filePath);
}

function main() {
  const nativeCommands = extractNativeCommands(readFile(tauriLibPath));

  const frontendFiles = walkFiles(uiSrcPath);
  const frontendCommandToFiles = new Map();

  for (const file of frontendFiles) {
    const commands = extractInvokedCommands(readFile(file));
    for (const command of commands) {
      const files = frontendCommandToFiles.get(command) ?? new Set();
      files.add(rel(file));
      frontendCommandToFiles.set(command, files);
    }
  }

  const frontendCommands = new Set(frontendCommandToFiles.keys());
  const missingNativeForFrontend = [...frontendCommands].filter((cmd) => !nativeCommands.has(cmd)).sort();

  const harnessCommands = new Set(extractHarnessCommands(readFile(harnessPath)));
  const invalidHarnessCommands = [...harnessCommands]
    .filter((cmd) => !nativeCommands.has(cmd) && !HARNESS_ALLOWED_NON_NATIVE.has(cmd))
    .sort();

  if (missingNativeForFrontend.length === 0 && invalidHarnessCommands.length === 0) {
    console.log("[PASS] Command surface check: frontend invokes and harness mocks are aligned with native command registry.");
    console.log(`Frontend commands checked: ${frontendCommands.size}`);
    console.log(`Native commands registered: ${nativeCommands.size}`);
    console.log(`Harness command cases checked: ${harnessCommands.size}`);
    process.exit(0);
  }

  if (missingNativeForFrontend.length > 0) {
    console.error("[FAIL] Frontend invokes missing from tauri::generate_handler![]:");
    for (const command of missingNativeForFrontend) {
      const files = [...(frontendCommandToFiles.get(command) ?? [])].sort().join(", ");
      console.error(`  - ${command} (used in: ${files})`);
    }
  }

  if (invalidHarnessCommands.length > 0) {
    console.error("[FAIL] Browser harness contains command mocks not present in native registry:");
    for (const command of invalidHarnessCommands) {
      console.error(`  - ${command}`);
    }
  }

  process.exit(1);
}

main();
