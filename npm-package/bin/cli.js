#!/usr/bin/env node

// AO Desktop CLI
//
// Downloads and installs/launches the correct AO binary for the current platform.
// Zero runtime dependencies - uses only Node.js built-ins.
//
// Usage:
//   npx ao-desktop           # Download & install AO
//   npx ao-desktop install   # Same as above
//   npx ao-desktop version   # Show CLI version
//   npx ao-desktop help      # Show usage

"use strict";

const https = require("node:https");
const http = require("node:http");
const fs = require("node:fs");
const path = require("node:path");
const os = require("node:os");
const { execSync, spawn } = require("node:child_process");

// ── Constants ────────────────────────────────────────────────────────────

const GITHUB_OWNER = "jbcupps";
const GITHUB_REPO = "ao";
const RELEASES_API = `https://api.github.com/repos/${GITHUB_OWNER}/${GITHUB_REPO}/releases/latest`;

// Map Node.js platform/arch to asset names
const PLATFORM_ASSETS = {
  "win32-x64": "AO-windows-x64-setup.exe",
  "darwin-x64": "AO-macos-universal.dmg",
  "darwin-arm64": "AO-macos-universal.dmg",
  "linux-x64": "AO-linux-x64.deb",
};

const CLI_VERSION = require("../package.json").version;

// ── Helpers ──────────────────────────────────────────────────────────────

/**
 * HTTPS GET with redirect following. Returns a Buffer.
 */
function httpsGet(url, headers = {}) {
  return new Promise((resolve, reject) => {
    const parsedUrl = new URL(url);
    const client = parsedUrl.protocol === "https:" ? https : http;

    const reqHeaders = {
      "User-Agent": `ao-desktop-cli/${CLI_VERSION}`,
      ...headers,
    };

    client
      .get(url, { headers: reqHeaders }, (res) => {
        // Follow redirects (GitHub uses 302 for asset downloads)
        if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
          return resolve(httpsGet(res.headers.location, headers));
        }

        if (res.statusCode !== 200) {
          res.resume();
          return reject(new Error(`HTTP ${res.statusCode} for ${url}`));
        }

        const chunks = [];
        res.on("data", (chunk) => chunks.push(chunk));
        res.on("end", () => resolve(Buffer.concat(chunks)));
        res.on("error", reject);
      })
      .on("error", reject);
  });
}

/**
 * Fetch the latest release metadata from GitHub API.
 */
async function fetchLatestRelease() {
  console.log("Fetching latest release info from GitHub...");
  const data = await httpsGet(RELEASES_API, {
    Accept: "application/vnd.github+json",
  });
  return JSON.parse(data.toString("utf-8"));
}

/**
 * Detect platform and return the expected asset name.
 */
function getAssetName() {
  const key = `${process.platform}-${process.arch}`;
  const asset = PLATFORM_ASSETS[key];
  if (!asset) {
    console.error(`Unsupported platform: ${key}`);
    console.error(`Supported platforms: ${Object.keys(PLATFORM_ASSETS).join(", ")}`);
    process.exit(1);
  }
  return asset;
}

/**
 * Download a file with progress display.
 */
async function downloadWithProgress(url, destPath) {
  return new Promise((resolve, reject) => {
    const parsedUrl = new URL(url);
    const client = parsedUrl.protocol === "https:" ? https : http;

    client
      .get(url, { headers: { "User-Agent": `ao-desktop-cli/${CLI_VERSION}` } }, (res) => {
        // Follow redirects
        if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
          return resolve(downloadWithProgress(res.headers.location, destPath));
        }

        if (res.statusCode !== 200) {
          res.resume();
          return reject(new Error(`HTTP ${res.statusCode}`));
        }

        const totalBytes = parseInt(res.headers["content-length"] || "0", 10);
        let downloadedBytes = 0;
        const file = fs.createWriteStream(destPath);

        res.on("data", (chunk) => {
          downloadedBytes += chunk.length;
          file.write(chunk);

          if (totalBytes > 0) {
            const pct = ((downloadedBytes / totalBytes) * 100).toFixed(1);
            const mb = (downloadedBytes / 1024 / 1024).toFixed(1);
            const totalMb = (totalBytes / 1024 / 1024).toFixed(1);
            process.stdout.write(`\r  Downloading: ${mb} MB / ${totalMb} MB (${pct}%)`);
          } else {
            const mb = (downloadedBytes / 1024 / 1024).toFixed(1);
            process.stdout.write(`\r  Downloading: ${mb} MB`);
          }
        });

        res.on("end", () => {
          file.end();
          console.log(""); // newline after progress
          resolve();
        });

        res.on("error", (err) => {
          file.close();
          fs.unlinkSync(destPath);
          reject(err);
        });
      })
      .on("error", reject);
  });
}

// ── Platform-specific installers ─────────────────────────────────────────

async function installWindows(filePath) {
  console.log("Running Windows installer...");
  console.log(`  File: ${filePath}`);
  console.log("  The installer will open in a new window.");
  spawn(filePath, [], { detached: true, stdio: "ignore" }).unref();
}

async function installMacOS(filePath) {
  console.log("Mounting DMG and copying to /Applications...");
  const mountPoint = `/Volumes/AO-mount-${Date.now()}`;

  try {
    execSync(`hdiutil attach "${filePath}" -mountpoint "${mountPoint}" -nobrowse`, {
      stdio: "pipe",
    });

    // Find the .app bundle inside the mounted DMG
    const contents = fs.readdirSync(mountPoint);
    const appBundle = contents.find((f) => f.endsWith(".app"));

    if (!appBundle) {
      console.error("No .app bundle found in the DMG.");
      return;
    }

    const src = path.join(mountPoint, appBundle);
    const dest = path.join("/Applications", appBundle);

    console.log(`  Copying ${appBundle} to /Applications...`);

    // Remove existing version if present
    if (fs.existsSync(dest)) {
      execSync(`rm -rf "${dest}"`, { stdio: "pipe" });
    }

    execSync(`cp -R "${src}" "${dest}"`, { stdio: "pipe" });
    console.log(`  Installed: ${dest}`);
    console.log("");
    console.log("  Note: On first launch, you may need to right-click the app");
    console.log("  and select Open to bypass macOS Gatekeeper (app is not notarized).");
  } finally {
    try {
      execSync(`hdiutil detach "${mountPoint}" -quiet`, { stdio: "pipe" });
    } catch {
      // Ignore detach errors
    }
  }
}

async function installLinux(filePath) {
  console.log("Installing .deb package...");

  // Check if dpkg is available
  try {
    execSync("which dpkg", { stdio: "pipe" });
  } catch {
    console.log(`  dpkg not found. You can install the package manually:`);
    console.log(`  sudo dpkg -i "${filePath}"`);
    console.log(`  sudo apt-get install -f`);
    return;
  }

  console.log(`  Running: sudo dpkg -i "${filePath}"`);
  console.log("  You may be prompted for your password.");

  try {
    execSync(`sudo dpkg -i "${filePath}"`, { stdio: "inherit" });
    console.log("  Installed successfully!");
  } catch {
    console.log("  dpkg install had issues. Attempting to fix dependencies...");
    try {
      execSync("sudo apt-get install -f -y", { stdio: "inherit" });
      console.log("  Dependencies fixed.");
    } catch {
      console.error("  Failed to resolve dependencies. Please install manually.");
    }
  }
}

// ── Commands ─────────────────────────────────────────────────────────────

async function cmdInstall() {
  const assetName = getAssetName();
  console.log(`Platform: ${process.platform} (${process.arch})`);
  console.log(`Looking for: ${assetName}`);
  console.log("");

  // Fetch release info
  const release = await fetchLatestRelease();
  console.log(`Latest release: ${release.tag_name}`);

  // Find the matching asset
  const asset = release.assets.find((a) => a.name === assetName);
  if (!asset) {
    console.error(`Asset ${assetName} not found in release ${release.tag_name}`);
    console.error("Available assets:");
    release.assets.forEach((a) => console.error(`  - ${a.name}`));
    process.exit(1);
  }

  // Download to temp directory
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "ao-desktop-"));
  const destPath = path.join(tmpDir, assetName);

  console.log(`Downloading ${assetName} (${(asset.size / 1024 / 1024).toFixed(1)} MB)...`);
  await downloadWithProgress(asset.browser_download_url, destPath);
  console.log(`  Saved to: ${destPath}`);
  console.log("");

  // Run platform-specific installer
  switch (process.platform) {
    case "win32":
      await installWindows(destPath);
      break;
    case "darwin":
      await installMacOS(destPath);
      break;
    case "linux":
      await installLinux(destPath);
      break;
    default:
      console.log(`Downloaded to: ${destPath}`);
      console.log("Please install manually for your platform.");
  }

  console.log("");
  console.log("Done! Launch AO from your applications menu or desktop.");
}

function cmdVersion() {
  console.log(`ao-desktop CLI v${CLI_VERSION}`);
}

function cmdHelp() {
  console.log(`
ao-desktop - CLI installer for AO Desktop AI Agent

Usage:
  npx ao-desktop [command]

Commands:
  install     Download and install AO for your platform (default)
  version     Show CLI version
  help        Show this help message

Examples:
  npx ao-desktop              Download & install the latest AO release
  npx ao-desktop install      Same as above
  npx ao-desktop version      Print version

Supported platforms:
  - Windows (x64)     - NSIS installer (.exe)
  - macOS (Intel/M1)  - Universal DMG (.dmg)
  - Linux (x64)       - Debian package (.deb)

More info: https://github.com/${GITHUB_OWNER}/${GITHUB_REPO}
`);
}

// ── Main ─────────────────────────────────────────────────────────────────

async function main() {
  const args = process.argv.slice(2);
  const command = args[0] || "install";

  try {
    switch (command) {
      case "install":
        await cmdInstall();
        break;
      case "version":
      case "--version":
      case "-v":
        cmdVersion();
        break;
      case "help":
      case "--help":
      case "-h":
        cmdHelp();
        break;
      default:
        console.error(`Unknown command: ${command}`);
        cmdHelp();
        process.exit(1);
    }
  } catch (err) {
    console.error("");
    console.error(`Error: ${err.message}`);
    if (process.env.DEBUG) {
      console.error(err.stack);
    }
    process.exit(1);
  }
}

main();
