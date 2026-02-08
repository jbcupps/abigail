#!/usr/bin/env node

// Launcher for the Abigail desktop binary installed by scripts/install.js.
//
// Reads the metadata file written during postinstall to locate the correct
// binary, then spawns it with the user's arguments forwarded.

const fs = require("fs");
const path = require("path");
const { spawn } = require("child_process");

const binDir = __dirname;
const metaPath = path.join(binDir, ".abigail-meta.json");

// ---------------------------------------------------------------------------
// Locate the installed binary
// ---------------------------------------------------------------------------

function findBinary() {
  if (!fs.existsSync(metaPath)) {
    console.error(
      "Abigail binary not found. The postinstall script may not have run."
    );
    console.error("Try reinstalling: npm install -g abigail-desktop");
    process.exit(1);
  }

  let meta;
  try {
    meta = JSON.parse(fs.readFileSync(metaPath, "utf8"));
  } catch (err) {
    console.error(`Failed to read install metadata: ${err.message}`);
    process.exit(1);
  }

  const binaryPath = path.join(binDir, meta.filename);

  if (!fs.existsSync(binaryPath)) {
    console.error(`Abigail binary not found at: ${binaryPath}`);
    console.error("Try reinstalling: npm install -g abigail-desktop");
    process.exit(1);
  }

  return { binaryPath, meta };
}

// ---------------------------------------------------------------------------
// Platform-specific launch logic
// ---------------------------------------------------------------------------

function launch(binaryPath, meta) {
  const args = process.argv.slice(2);

  switch (meta.platform) {
    case "win32":
      // For .msi, we run the installed executable, not the installer.
      // Try to find Abigail in the standard install location.
      const winInstallPath = path.join(
        process.env.LOCALAPPDATA || "",
        "Abigail",
        "Abigail.exe"
      );

      if (fs.existsSync(winInstallPath)) {
        spawn(winInstallPath, args, { stdio: "inherit", detached: true }).unref();
      } else {
        // Fall back to launching the installer
        console.log(
          "Abigail is not installed yet. Launching the installer..."
        );
        spawn("msiexec", ["/i", binaryPath], {
          stdio: "inherit",
          detached: true,
        }).unref();
      }
      break;

    case "darwin":
      // For .dmg, try the Applications folder first
      const macAppPath = "/Applications/Abigail.app";
      if (fs.existsSync(macAppPath)) {
        spawn("open", ["-a", macAppPath, "--args", ...args], {
          stdio: "inherit",
        });
      } else {
        // Mount and open the dmg
        console.log(
          "Abigail is not installed yet. Opening the disk image..."
        );
        spawn("open", [binaryPath], { stdio: "inherit" });
      }
      break;

    case "linux":
      // AppImage is directly executable
      spawn(binaryPath, args, {
        stdio: "inherit",
        detached: true,
      }).unref();
      break;

    default:
      console.error(`Unsupported platform: ${meta.platform}`);
      process.exit(1);
  }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

const { binaryPath, meta } = findBinary();
launch(binaryPath, meta);
