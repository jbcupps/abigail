#!/usr/bin/env node
// File identity and release policy checks, not an Authenticode implementation.
// Windows verification must produce the evidence file before this utility runs.
// CLI (all options also accept RELEASE_<UPPER_SNAKE_CASE> environment variables):
//   create --assets-dir DIR --source-commit SHA --tag TAG --channel stable|beta|candidate
//          --windows-signed true|false [--expected-thumbprint SHA1]
//          [--verification-report windows-signature-evidence.json]
//   verify <same policy options> [--manifest PATH]
//   verify-metadata --manifest PATH --release RELEASE_JSON --latest-release LATEST_JSON
//                   --source-commit SHA
// create writes DIR/release-provenance.json. verify does not modify files.
// The latest-release JSON must contain null when GitHub has no latest stable release.
// Stable and beta require signing; unsigned builds must use the candidate channel.

import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import { pathToFileURL } from "node:url";

export const MANIFEST_NAME = "release-provenance.json";
export const EVIDENCE_NAME = "windows-signature-evidence.json";
const INSTALLER_NAME = "Abigail-windows-x64-setup.exe";
const MSI_NAME = "Abigail-windows-x64.msi";
const SHA256 = /^[a-f0-9]{64}$/i;
const SHA1 = /^[a-f0-9]{40}$/i;
const STABLE_TAG = /^v(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)$/;
const BETA_TAG = /^v(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)-beta\.(?:0|[1-9]\d*)$/;

function requireCondition(condition, message) {
  if (!condition) throw new Error(message);
}

function normalizedThumbprint(value, required) {
  if (!required) {
    requireCondition(value === undefined || value === null || value === "", "Unsigned policy must not claim a certificate thumbprint.");
    return null;
  }
  requireCondition(typeof value === "string" && SHA1.test(value), "Signed policy requires an exact 40-hex expected certificate thumbprint.");
  return value.toUpperCase();
}

function validatePolicy({ sourceCommit, tag, channel, windowsSigned, expectedThumbprint }) {
  requireCondition(typeof sourceCommit === "string" && SHA1.test(sourceCommit), "sourceCommit must be an exact 40-hex Git commit SHA.");
  requireCondition(["stable", "beta", "candidate"].includes(channel), "Unknown release channel.");
  requireCondition(typeof windowsSigned === "boolean", "windowsSigned must be a Boolean, not a truthy string.");
  const validTag = channel === "stable" ? STABLE_TAG.test(tag) : channel === "beta" ? BETA_TAG.test(tag) : STABLE_TAG.test(tag) || BETA_TAG.test(tag);
  requireCondition(typeof tag === "string" && validTag, "Release tag does not match its channel.");
  requireCondition(channel === "candidate" || windowsSigned, "Stable and beta releases require signed Windows installers; unsigned builds must use the candidate channel.");
  return { sourceCommit: sourceCommit.toLowerCase(), tag, channel, windowsSigned, expectedThumbprint: normalizedThumbprint(expectedThumbprint, windowsSigned) };
}

function validateName(name) {
  requireCondition(typeof name === "string" && /^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(name) && !name.endsWith("."), `Unsafe release filename: ${String(name)}`);
  requireCondition(!/^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(name), `Reserved release filename: ${name}`);
  if (/\.(exe|msi)$/i.test(name)) {
    requireCondition(name === INSTALLER_NAME || name === MSI_NAME, `Unexpected executable installer: ${name}`);
  }
}

function isWindowsInstaller(name) {
  return /\.(exe|msi)$/i.test(name);
}

function digest(bytes) {
  return crypto.createHash("sha256").update(bytes).digest("hex");
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8").replace(/^\uFEFF/, ""));
}

function readAssets(assetsDir) {
  const directory = path.resolve(assetsDir);
  const directoryStat = fs.lstatSync(directory);
  requireCondition(directoryStat.isDirectory() && !directoryStat.isSymbolicLink(), "Assets directory must be a real directory, not a link.");
  const seen = new Set();
  const records = [];
  for (const name of fs.readdirSync(directory).sort()) {
    validateName(name);
    requireCondition(!seen.has(name.toLowerCase()), `Case-colliding release filename: ${name}`);
    seen.add(name.toLowerCase());
    const absolute = path.join(directory, name);
    const stat = fs.lstatSync(absolute);
    requireCondition(stat.isFile() && !stat.isSymbolicLink(), `Release assets must be flat regular files: ${name}`);
    if (name === MANIFEST_NAME) continue;
    requireCondition(Number.isSafeInteger(stat.size) && stat.size > 0, `Release asset must be nonempty: ${name}`);
    records.push({ name, size: stat.size, sha256: digest(fs.readFileSync(absolute)) });
  }
  requireCondition(records.some((record) => record.name === INSTALLER_NAME), `Missing canonical installer: ${INSTALLER_NAME}`);
  return records;
}

function evidenceFileName(assetsDir, value = EVIDENCE_NAME) {
  requireCondition(typeof value === "string" && value.length > 0, "verificationReport must identify a staged evidence file.");
  const directory = path.resolve(assetsDir);
  const resolved = path.resolve(directory, value);
  const name = path.basename(resolved);
  requireCondition(path.dirname(resolved) === directory, "Verification report must be directly inside the assets directory.");
  validateName(name);
  requireCondition(name !== MANIFEST_NAME && name.endsWith(".json"), "Verification report must be a separate JSON asset.");
  return name;
}

function requireSamePolicy(actual, expected, label) {
  const normalized = validatePolicy(actual);
  for (const key of Object.keys(expected)) {
    requireCondition(normalized[key] === expected[key], `${label} ${key} does not match the requested release policy.`);
  }
}

function validateEvidence(report, policy, assets) {
  requireCondition(report && report.schemaVersion === 1, "Unsupported Windows verification report schema.");
  requireSamePolicy(report, policy, "Verification report");
  requireCondition(Array.isArray(report.artifacts), "Verification report artifacts must be an array.");
  const expected = new Map(assets.filter((asset) => isWindowsInstaller(asset.name)).map((asset) => [asset.name, asset]));
  const seen = new Set();
  for (const artifact of report.artifacts) {
    requireCondition(artifact && typeof artifact === "object", "Invalid verification artifact.");
    validateName(artifact.name);
    requireCondition(expected.has(artifact.name) && !seen.has(artifact.name), `Unexpected or duplicate verification artifact: ${artifact.name}`);
    seen.add(artifact.name);
    requireCondition(typeof artifact.sha256 === "string" && SHA256.test(artifact.sha256) && artifact.sha256.toLowerCase() === expected.get(artifact.name).sha256, `Verification hash differs from staged bytes: ${artifact.name}`);
    if (policy.windowsSigned) {
      requireCondition(artifact.signatureStatus === "Valid", `Signature was not verified Valid: ${artifact.name}`);
      requireCondition(typeof artifact.publisher === "string" && artifact.publisher.trim().length > 0, `Missing verified publisher: ${artifact.name}`);
      requireCondition(normalizedThumbprint(artifact.certificateThumbprint, true) === policy.expectedThumbprint, `Unexpected signing certificate: ${artifact.name}`);
      requireCondition(["RFC3161", "Authenticode"].includes(artifact.timestampType), `Missing verified timestamp: ${artifact.name}`);
    } else {
      requireCondition(artifact.signatureStatus === "NotSigned", `Unsigned release evidence must explicitly say NotSigned: ${artifact.name}`);
      requireCondition(!artifact.publisher && !artifact.certificateThumbprint && !artifact.timestampType, `Unsigned evidence must not claim signing identity or timestamp: ${artifact.name}`);
    }
  }
  requireCondition(seen.size === expected.size, "Verification evidence is missing a staged Windows installer.");
}

function validateManifest(manifest) {
  requireCondition(manifest && manifest.schemaVersion === 1, "Unsupported release provenance schema.");
  const policy = validatePolicy(manifest);
  validateName(manifest.verificationReport);
  requireCondition(manifest.verificationReport !== MANIFEST_NAME && manifest.verificationReport.endsWith(".json"), "Invalid manifest verification report filename.");
  requireCondition(Array.isArray(manifest.assets) && manifest.assets.length > 0, "Manifest assets must be a nonempty array.");
  const seen = new Set();
  for (const asset of manifest.assets) {
    requireCondition(asset && typeof asset === "object", "Invalid manifest asset.");
    validateName(asset.name);
    requireCondition(asset.name !== MANIFEST_NAME && !seen.has(asset.name.toLowerCase()), `Duplicate or recursive provenance asset: ${asset.name}`);
    seen.add(asset.name.toLowerCase());
    requireCondition(Number.isSafeInteger(asset.size) && asset.size > 0 && typeof asset.sha256 === "string" && SHA256.test(asset.sha256), `Invalid size or digest for ${asset.name}`);
  }
  requireCondition(manifest.assets.some((asset) => asset.name === INSTALLER_NAME), "Manifest is missing the canonical Windows installer.");
  requireCondition(manifest.assets.some((asset) => asset.name === manifest.verificationReport), "Manifest is missing its signature evidence asset.");
  return policy;
}

export function createProvenance(options) {
  const policy = validatePolicy(options);
  const assets = readAssets(options.assetsDir);
  const verificationReport = evidenceFileName(options.assetsDir, options.verificationReport);
  requireCondition(assets.some((asset) => asset.name === verificationReport), "Windows verification report is not staged for publication.");
  validateEvidence(readJson(path.join(options.assetsDir, verificationReport)), policy, assets);
  return { schemaVersion: 1, ...policy, verificationReport, assets };
}

export function verifyProvenance({ assetsDir, manifest, ...requestedPolicy }) {
  const policy = validatePolicy(requestedPolicy);
  validateManifest(manifest);
  requireSamePolicy(manifest, policy, "Manifest");
  const assets = readAssets(assetsDir);
  const expected = new Map(manifest.assets.map((asset) => [asset.name, asset]));
  requireCondition(assets.length === expected.size, "Staged files do not match the manifest asset set (missing or extra files).");
  for (const asset of assets) {
    const record = expected.get(asset.name);
    requireCondition(record && record.size === asset.size && record.sha256.toLowerCase() === asset.sha256, `Staged asset differs from provenance: ${asset.name}`);
  }
  validateEvidence(readJson(path.join(assetsDir, manifest.verificationReport)), policy, assets);
  return manifest;
}

export function serializeProvenance(manifest) {
  return `${JSON.stringify(manifest, null, 2)}\n`;
}

export function validatePublishedRelease({ release, latestRelease, manifest, expectedCommit }) {
  const policy = validateManifest(manifest);
  requireCondition(policy.channel !== "candidate", "Candidate artifacts cannot be published as a release.");
  requireCondition(typeof expectedCommit === "string" && SHA1.test(expectedCommit) && policy.sourceCommit === expectedCommit.toLowerCase(), "Published manifest source commit differs from the expected commit.");
  requireCondition(release && release.tag_name === policy.tag && release.draft === false && release.prerelease === (policy.channel === "beta"), "Published tag, draft, or prerelease state differs from release policy.");
  requireCondition(typeof release.target_commitish === "string" && release.target_commitish.toLowerCase() === policy.sourceCommit, "Published release target_commitish must identify the exact source commit.");
  requireCondition(latestRelease !== undefined, "Latest stable release lookup is required (use null when none exists).");
  if (policy.channel === "stable") {
    requireCondition(latestRelease && latestRelease.tag_name === policy.tag && latestRelease.draft === false && latestRelease.prerelease === false, "Latest stable download does not target this stable release.");
  } else if (latestRelease !== null) {
    requireCondition(latestRelease && STABLE_TAG.test(latestRelease.tag_name) && latestRelease.tag_name !== policy.tag && latestRelease.draft === false && latestRelease.prerelease === false, "Beta release must not replace the latest stable download.");
  }
  const manifestBytes = Buffer.from(serializeProvenance(manifest), "utf8");
  const expected = new Map([...manifest.assets, { name: MANIFEST_NAME, size: manifestBytes.length, sha256: digest(manifestBytes) }].map((asset) => [asset.name, asset]));
  requireCondition(Array.isArray(release.assets) && release.assets.length === expected.size, "Published asset set differs from the manifest.");
  const seen = new Set();
  for (const asset of release.assets) {
    requireCondition(asset && typeof asset === "object", "Invalid published asset.");
    const record = expected.get(asset.name);
    requireCondition(record && !seen.has(asset.name), `Unexpected or duplicate published asset: ${asset.name}`);
    seen.add(asset.name);
    requireCondition(asset.state === "uploaded" && asset.size === record.size && typeof asset.digest === "string" && asset.digest.toLowerCase() === `sha256:${record.sha256.toLowerCase()}`, `Published asset size, digest, or upload state differs: ${asset.name}`);
  }
  return { tag: policy.tag, channel: policy.channel, sourceCommit: policy.sourceCommit, verifiedAssets: expected.size };
}

function parseArgs(argv) {
  const allowed = new Set(["assets-dir", "source-commit", "tag", "channel", "windows-signed", "expected-thumbprint", "verification-report", "manifest", "release", "latest-release"]);
  const options = {};
  for (let index = 0; index < argv.length; index += 2) {
    const name = argv[index]?.replace(/^--/, "");
    requireCondition(argv[index]?.startsWith("--") && allowed.has(name) && !Object.hasOwn(options, name), `Unknown or duplicate option: ${argv[index]}`);
    requireCondition(argv[index + 1] !== undefined && !argv[index + 1].startsWith("--"), `Missing value for --${name}`);
    options[name] = argv[index + 1];
  }
  return (name, required = true) => {
    const value = options[name] ?? process.env[`RELEASE_${name.replaceAll("-", "_").toUpperCase()}`];
    requireCondition(!required || (value !== undefined && value !== ""), `--${name} or its RELEASE_ environment variable is required.`);
    return value;
  };
}

function main(argv) {
  const [command, ...tokens] = argv;
  requireCondition(["create", "verify", "verify-metadata"].includes(command), "Expected command: create, verify, or verify-metadata. See this script's CLI header.");
  const arg = parseArgs(tokens);
  if (command === "verify-metadata") {
    const result = validatePublishedRelease({ manifest: readJson(arg("manifest")), release: readJson(arg("release")), latestRelease: readJson(arg("latest-release")), expectedCommit: arg("source-commit") });
    console.log(`Verified published ${result.channel} ${result.tag}: ${result.verifiedAssets} assets.`);
    return;
  }
  const signed = arg("windows-signed");
  requireCondition(["true", "false"].includes(signed), "--windows-signed must be exactly true or false.");
  const options = { assetsDir: arg("assets-dir"), sourceCommit: arg("source-commit"), tag: arg("tag"), channel: arg("channel"), windowsSigned: signed === "true", expectedThumbprint: arg("expected-thumbprint", false) };
  if (command === "create") {
    const manifest = createProvenance({ ...options, verificationReport: arg("verification-report", false) });
    fs.writeFileSync(path.join(options.assetsDir, MANIFEST_NAME), serializeProvenance(manifest), { encoding: "utf8", flag: "wx" });
    console.log(`Created provenance for ${manifest.assets.length} staged files (${manifest.channel} ${manifest.tag}).`);
  } else {
    const manifest = readJson(arg("manifest", false) ?? path.join(options.assetsDir, MANIFEST_NAME));
    verifyProvenance({ ...options, manifest });
    console.log(`Verified provenance for ${manifest.assets.length} staged files (${manifest.channel} ${manifest.tag}).`);
  }
}

if (process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url) {
  try { main(process.argv.slice(2)); }
  catch (error) { console.error(`Release provenance failed: ${error.message}`); process.exitCode = 1; }
}
