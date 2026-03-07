#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

function parseArgs(argv) {
  const args = {};
  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i];
    if (!token.startsWith("--")) {
      throw new Error(`Unexpected argument: ${token}`);
    }
    const key = token.slice(2);
    const value = argv[i + 1];
    if (!value || value.startsWith("--")) {
      throw new Error(`Missing value for --${key}`);
    }
    args[key] = value;
    i += 1;
  }
  return args;
}

function requireArg(args, key) {
  const value = args[key];
  if (!value) {
    throw new Error(`--${key} is required`);
  }
  return value;
}

function readSignature(assetsDir, fileName) {
  const signaturePath = path.join(assetsDir, fileName);
  if (!fs.existsSync(signaturePath)) {
    return null;
  }
  return fs.readFileSync(signaturePath, "utf8").trim();
}

function addPlatform(platforms, assetsDir, baseUrl, target, assetName, signatureName) {
  const assetPath = path.join(assetsDir, assetName);
  const signature = readSignature(assetsDir, signatureName);
  if (!fs.existsSync(assetPath) || !signature) {
    return;
  }

  platforms[target] = {
    url: `${baseUrl.replace(/\/$/, "")}/${assetName}`,
    signature,
  };
}

const args = parseArgs(process.argv.slice(2));
const version = requireArg(args, "version");
const assetsDir = requireArg(args, "assets-dir");
const baseUrl = requireArg(args, "base-url");
const outputPath = requireArg(args, "output");
const notes = args.notes ?? `Abigail ${version}`;
const pubDate = args["pub-date"] ?? new Date().toISOString();

const platforms = {};

addPlatform(
  platforms,
  assetsDir,
  baseUrl,
  "windows-x86_64-nsis",
  "Abigail-updater-windows-x64.nsis.zip",
  "Abigail-updater-windows-x64.nsis.zip.sig"
);
addPlatform(
  platforms,
  assetsDir,
  baseUrl,
  "windows-x86_64-msi",
  "Abigail-updater-windows-x64.msi.zip",
  "Abigail-updater-windows-x64.msi.zip.sig"
);

const macAsset = "Abigail-updater-macos-universal.app.tar.gz";
const macSig = "Abigail-updater-macos-universal.app.tar.gz.sig";
if (fs.existsSync(path.join(assetsDir, macAsset)) && readSignature(assetsDir, macSig)) {
  addPlatform(platforms, assetsDir, baseUrl, "darwin-x86_64-app", macAsset, macSig);
  addPlatform(platforms, assetsDir, baseUrl, "darwin-aarch64-app", macAsset, macSig);
}

addPlatform(
  platforms,
  assetsDir,
  baseUrl,
  "linux-x86_64-appimage",
  "Abigail-updater-linux-x64.AppImage.tar.gz",
  "Abigail-updater-linux-x64.AppImage.tar.gz.sig"
);

if (Object.keys(platforms).length === 0) {
  throw new Error("No updater artifacts were found to include in latest.json");
}

const manifest = {
  version,
  notes,
  pub_date: pubDate,
  platforms,
};

fs.writeFileSync(outputPath, `${JSON.stringify(manifest, null, 2)}\n`);
console.log(`Wrote ${outputPath}`);
