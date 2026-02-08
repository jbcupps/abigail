#!/usr/bin/env node

// Post-install script for abigail-desktop npm package.
//
// Detects the current platform and architecture, downloads the correct
// Abigail binary from the matching GitHub Release, and places it where
// the bin/run.js launcher can find it.

const fs = require("fs");
const path = require("path");
const https = require("https");
const { execSync } = require("child_process");

const pkg = require("../package.json");
const version = pkg.version;

// ---------------------------------------------------------------------------
// Platform / arch mapping
// ---------------------------------------------------------------------------

const PLATFORM_MAP = {
  win32: { ext: "msi", archMap: { x64: "x64", arm64: "arm64" } },
  darwin: { ext: "dmg", archMap: { x64: "universal", arm64: "universal" } },
  linux: { ext: "AppImage", archMap: { x64: "x64", arm64: "arm64" } },
};

function getPlatformInfo() {
  const platform = process.platform;
  const arch = process.arch;

  const info = PLATFORM_MAP[platform];
  if (!info) {
    console.error(`Unsupported platform: ${platform}`);
    process.exit(1);
  }

  const mappedArch = info.archMap[arch];
  if (!mappedArch) {
    console.error(`Unsupported architecture: ${arch} on ${platform}`);
    process.exit(1);
  }

  return {
    platform,
    arch: mappedArch,
    ext: info.ext,
  };
}

// ---------------------------------------------------------------------------
// Download helper (follows redirects)
// ---------------------------------------------------------------------------

function download(url, destPath, redirectCount = 0) {
  return new Promise((resolve, reject) => {
    if (redirectCount > 5) {
      return reject(new Error("Too many redirects"));
    }

    https
      .get(url, { headers: { "User-Agent": "abigail-desktop-npm" } }, (res) => {
        // Handle redirects (GitHub releases redirect to S3)
        if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
          return download(res.headers.location, destPath, redirectCount + 1).then(
            resolve,
            reject
          );
        }

        if (res.statusCode !== 200) {
          return reject(
            new Error(`Download failed with status ${res.statusCode}: ${url}`)
          );
        }

        const file = fs.createWriteStream(destPath);
        res.pipe(file);
        file.on("finish", () => {
          file.close(resolve);
        });
        file.on("error", (err) => {
          fs.unlink(destPath, () => {});
          reject(err);
        });
      })
      .on("error", reject);
  });
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main() {
  const { platform, arch, ext } = getPlatformInfo();

  const filename = `abigail_${version}_${arch}.${ext}`;
  const url = `https://github.com/jbcupps/abigail/releases/download/v${version}/${filename}`;

  // Determine install directory: place the binary next to this package
  const binDir = path.join(__dirname, "..", "bin");
  const destPath = path.join(binDir, filename);

  // Ensure bin directory exists
  if (!fs.existsSync(binDir)) {
    fs.mkdirSync(binDir, { recursive: true });
  }

  console.log(`Abigail v${version} - Installing for ${platform} (${arch})`);
  console.log(`Downloading: ${url}`);

  try {
    await download(url, destPath);
    console.log(`Downloaded to: ${destPath}`);

    // Make executable on Unix platforms
    if (platform !== "win32") {
      fs.chmodSync(destPath, 0o755);
    }

    // Write a metadata file so the launcher knows which file to run
    const metaPath = path.join(binDir, ".abigail-meta.json");
    fs.writeFileSync(
      metaPath,
      JSON.stringify(
        {
          version,
          platform,
          arch,
          filename,
          installedAt: new Date().toISOString(),
        },
        null,
        2
      ) + "\n"
    );

    console.log("Abigail installed successfully.");
  } catch (err) {
    console.error(`Failed to download Abigail binary: ${err.message}`);
    console.error(
      "You can download it manually from: " +
        `https://github.com/jbcupps/abigail/releases/tag/v${version}`
    );
    // Don't fail the npm install -- the user can still download manually
    process.exit(0);
  }
}

main();
