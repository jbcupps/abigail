#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const npmCommand = process.platform === "win32" ? "npm.cmd" : "npm";

const steps = [
  {
    label: "cargo check -p entity-daemon",
    command: "cargo",
    args: ["check", "-p", "entity-daemon"],
  },
  {
    label: "cargo check -p abigail-app",
    command: "cargo",
    args: ["check", "-p", "abigail-app"],
  },
  {
    label: "cargo test --workspace --exclude abigail-app --no-run",
    command: "cargo",
    args: ["test", "--workspace", "--exclude", "abigail-app", "--no-run"],
  },
  {
    label: "node scripts/check_crypto_claims.mjs",
    command: process.execPath,
    args: ["scripts/check_crypto_claims.mjs"],
  },
  {
    label: "npm run check:command-contract",
    command: npmCommand,
    args: ["run", "check:command-contract"],
    cwd: path.join(repoRoot, "tauri-app", "src-ui"),
  },
  {
    label: "npm test",
    command: npmCommand,
    args: ["test"],
    cwd: path.join(repoRoot, "tauri-app", "src-ui"),
  },
];

function resolveCommand(step) {
  if (process.platform === "win32" && step.command.toLowerCase().endsWith(".cmd")) {
    return {
      command: process.env.ComSpec || "cmd.exe",
      args: ["/d", "/s", "/c", step.command, ...step.args],
    };
  }

  return {
    command: step.command,
    args: step.args,
  };
}

for (const step of steps) {
  console.log(`\n==> ${step.label}`);
  const invocation = resolveCommand(step);
  const result = spawnSync(invocation.command, invocation.args, {
    cwd: step.cwd ?? repoRoot,
    stdio: "inherit",
  });

  if (result.error) {
    console.error(`Step failed to start: ${result.error.message}`);
    process.exit(1);
  }

  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

console.log("\nStability gates passed.");
