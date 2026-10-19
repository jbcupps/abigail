#!/usr/bin/env node
// Synthetic bytes/evidence exercise policy and provenance only. These tests do
// not sign, install, execute, or establish actual Windows Authenticode validity.
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import crypto from "node:crypto";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { createProvenance, verifyProvenance, validatePublishedRelease, serializeProvenance, MANIFEST_NAME, EVIDENCE_NAME } from "../release_provenance.mjs";

const COMMIT = "a".repeat(40);
const THUMBPRINT = "B".repeat(40);
const INSTALLER = "Abigail-windows-x64-setup.exe";
const MSI = "Abigail-windows-x64.msi";
const script = fileURLToPath(new URL("../release_provenance.mjs", import.meta.url));
const sha256 = (bytes) => crypto.createHash("sha256").update(bytes).digest("hex");
const clone = (value) => structuredClone(value);

function fixture(t, overrides = {}) {
  const assetsDir = fs.mkdtempSync(path.join(os.tmpdir(), "abigail-provenance-test-"));
  t.after(() => fs.rmSync(assetsDir, { recursive: true, force: true }));
  const policy = { sourceCommit: COMMIT, tag: "v0.0.76", channel: "stable", windowsSigned: true, expectedThumbprint: THUMBPRINT, ...overrides };
  const bytes = Buffer.from("Synthetic installer bytes; never executable.");
  fs.writeFileSync(path.join(assetsDir, INSTALLER), bytes);
  const report = {
    schemaVersion: 1,
    ...policy,
    artifacts: [{ name: INSTALLER, sha256: sha256(bytes), signatureStatus: policy.windowsSigned ? "Valid" : "NotSigned", publisher: policy.windowsSigned ? "CN=Example Test Publisher" : null, certificateThumbprint: policy.windowsSigned ? THUMBPRINT : null, timestampType: policy.windowsSigned ? "RFC3161" : null }],
  };
  const writeReport = () => fs.writeFileSync(path.join(assetsDir, EVIDENCE_NAME), JSON.stringify(report));
  writeReport();
  return { assetsDir, policy, report, writeReport, create: () => createProvenance({ assetsDir, ...policy }) };
}

function published(manifest) {
  const bytes = Buffer.from(serializeProvenance(manifest));
  const release = {
    tag_name: manifest.tag,
    target_commitish: manifest.sourceCommit,
    draft: false,
    prerelease: manifest.channel === "beta",
    assets: [...manifest.assets, { name: MANIFEST_NAME, size: bytes.length, sha256: sha256(bytes) }].map((asset) => ({ name: asset.name, size: asset.size, digest: `sha256:${asset.sha256}`, state: "uploaded" })),
  };
  return { release, latestRelease: manifest.channel === "stable" ? clone(release) : { tag_name: "v0.0.75", prerelease: false, draft: false }, manifest, expectedCommit: COMMIT };
}

test("synthetic provenance: records all final files, optional MSI, updater and signature evidence", (t) => {
  const f = fixture(t);
  const bytes = Buffer.from("Synthetic MSI bytes");
  fs.writeFileSync(path.join(f.assetsDir, MSI), bytes);
  f.report.artifacts.push({ ...f.report.artifacts[0], name: MSI, sha256: sha256(bytes), timestampType: "Authenticode" });
  f.writeReport();
  for (const name of ["Abigail-updater-windows-x64.nsis.zip", "Abigail-updater-windows-x64.nsis.zip.sig", "latest.json"]) fs.writeFileSync(path.join(f.assetsDir, name), "Synthetic support file");
  const manifest = f.create();
  assert.equal(manifest.schemaVersion, 1);
  assert.equal(manifest.assets.length, 6);
  assert.ok(manifest.assets.some((asset) => asset.name === EVIDENCE_NAME));
  fs.writeFileSync(path.join(f.assetsDir, MANIFEST_NAME), serializeProvenance(manifest));
  assert.equal(verifyProvenance({ assetsDir: f.assetsDir, manifest, ...f.policy }), manifest);
  assert.equal(validatePublishedRelease(published(manifest)).verifiedAssets, 7);
});

test("unsigned beta and unsigned candidate are explicit; neither may claim signing", (t) => {
  for (const channel of ["beta", "candidate"]) {
    const f = fixture(t, { channel, tag: "v0.0.76-beta.132", windowsSigned: false, expectedThumbprint: null });
    const manifest = f.create();
    assert.equal(manifest.windowsSigned, false);
    verifyProvenance({ assetsDir: f.assetsDir, manifest, ...f.policy });
    f.report.artifacts[0].publisher = "False publisher claim";
    f.writeReport();
    assert.throws(f.create, /must not claim signing identity/);
  }
});

test("candidate may use a planned stable tag but cannot be published", (t) => {
  const f = fixture(t, { channel: "candidate", windowsSigned: false, expectedThumbprint: null });
  const manifest = f.create();
  assert.throws(() => validatePublishedRelease(published(manifest)), /Candidate artifacts cannot be published/);
});

test("stable signing cannot be disabled and Boolean strings are rejected", (t) => {
  const f = fixture(t);
  assert.throws(() => createProvenance({ assetsDir: f.assetsDir, ...f.policy, windowsSigned: false, expectedThumbprint: null }), /Stable releases require signed/);
  assert.throws(() => createProvenance({ assetsDir: f.assetsDir, ...f.policy, windowsSigned: "true" }), /must be a Boolean/);
  assert.throws(() => createProvenance({ assetsDir: f.assetsDir, ...f.policy, expectedThumbprint: "" }), /exact 40-hex/);
});

test("source/tag/channel must match exact requested policy", (t) => {
  const f = fixture(t);
  const manifest = f.create();
  for (const changed of [{ sourceCommit: "c".repeat(40) }, { tag: "v0.0.77" }, { channel: "beta", tag: "v0.0.76-beta.132" }, { channel: "candidate", windowsSigned: false, expectedThumbprint: null }]) {
    assert.throws(() => verifyProvenance({ assetsDir: f.assetsDir, manifest, ...f.policy, ...changed }), /Manifest .*does not match/);
  }
  for (const changed of [{ sourceCommit: "main" }, { tag: "v0.0.76-beta.132" }, { channel: "beta" }, { tag: "../v0.0.76" }, { channel: "unknown" }]) {
    assert.throws(() => createProvenance({ assetsDir: f.assetsDir, ...f.policy, ...changed }));
  }
});

test("evidence must match source/tag/channel/signing policy", (t) => {
  for (const changed of [{ sourceCommit: "c".repeat(40) }, { tag: "v0.0.77" }, { channel: "candidate" }, { expectedThumbprint: "D".repeat(40) }, { schemaVersion: 2 }]) {
    const f = fixture(t);
    Object.assign(f.report, changed);
    f.writeReport();
    assert.throws(f.create, /Verification report|Unsupported/);
  }
});

test("a mismatched evidence hash, identity, status or timestamp blocks publication", (t) => {
  for (const changed of [{ sha256: "0".repeat(64) }, { certificateThumbprint: "D".repeat(40) }, { publisher: " " }, { signatureStatus: "UnknownError" }, { timestampType: null }, { timestampType: "none" }]) {
    const f = fixture(t);
    Object.assign(f.report.artifacts[0], changed);
    f.writeReport();
    assert.throws(f.create);
  }
});

test("verification evidence must cover every installer exactly once", (t) => {
  for (const mode of ["missing", "duplicate", "unexpected", "msi-unverified"]) {
    const f = fixture(t);
    if (mode === "missing") f.report.artifacts = [];
    if (mode === "duplicate") f.report.artifacts.push(clone(f.report.artifacts[0]));
    if (mode === "unexpected") f.report.artifacts[0].name = "another.exe";
    if (mode === "msi-unverified") fs.writeFileSync(path.join(f.assetsDir, MSI), "unverified");
    f.writeReport();
    assert.throws(f.create);
  }
});

test("missing, extra and tampered assets are rejected after manifest creation", (t) => {
  for (const mode of ["missing", "extra", "tampered", "evidence-tampered"]) {
    const f = fixture(t);
    const manifest = f.create();
    if (mode === "missing") fs.unlinkSync(path.join(f.assetsDir, EVIDENCE_NAME));
    if (mode === "extra") fs.writeFileSync(path.join(f.assetsDir, "unexpected.json"), "extra");
    if (mode === "tampered") {
      const bytes = fs.readFileSync(path.join(f.assetsDir, INSTALLER));
      bytes[0] ^= 1; // Same size, different SHA-256: size checks alone cannot pass.
      fs.writeFileSync(path.join(f.assetsDir, INSTALLER), bytes);
    }
    if (mode === "evidence-tampered") fs.appendFileSync(path.join(f.assetsDir, EVIDENCE_NAME), " ");
    assert.throws(() => verifyProvenance({ assetsDir: f.assetsDir, manifest, ...f.policy }), /asset set|differs from provenance/);
  }
});

test("missing canonical installer, renamed executable and subdirectories fail", (t) => {
  for (const mode of ["missing", "extra-exe", "extra-msi", "directory"]) {
    const f = fixture(t);
    if (mode === "missing") fs.unlinkSync(path.join(f.assetsDir, INSTALLER));
    if (mode === "extra-exe") fs.writeFileSync(path.join(f.assetsDir, "other.exe"), "unexpected");
    if (mode === "extra-msi") fs.writeFileSync(path.join(f.assetsDir, "other.msi"), "unexpected");
    if (mode === "directory") fs.mkdirSync(path.join(f.assetsDir, "nested"));
    assert.throws(f.create, /Missing canonical|Unexpected executable|flat regular/);
  }
});

test("symlink asset and symlink assets directory are rejected without reading targets", (t) => {
  const f = fixture(t);
  // Junction creation can still be unavailable in restricted Windows sandboxes.
  const target = path.join(f.assetsDir, "target");
  fs.mkdirSync(target);
  const link = path.join(f.assetsDir, "linked");
  try {
    fs.symlinkSync(target, link, process.platform === "win32" ? "junction" : "dir");
  } catch (error) {
    if (process.platform === "win32" && ["EPERM", "EACCES"].includes(error.code)) {
      t.skip(`Windows sandbox cannot create the test junction (${error.code}); run this case on Linux CI.`);
      return;
    }
    throw error;
  }
  assert.throws(f.create, /flat regular/);
  assert.throws(() => createProvenance({ assetsDir: link, ...f.policy }), /real directory/);
});

test("manifest path traversal, duplicate names, recursive manifest and bad digests fail", (t) => {
  const f = fixture(t);
  const original = f.create();
  for (const mode of ["path", "duplicate", "recursive", "bad-size", "bad-hash", "report-path"]) {
    const manifest = clone(original);
    if (mode === "path") manifest.assets[0].name = "../outside.exe";
    if (mode === "duplicate") manifest.assets.push(clone(manifest.assets[0]));
    if (mode === "recursive") manifest.assets[0].name = MANIFEST_NAME;
    if (mode === "bad-size") manifest.assets[0].size = -1;
    if (mode === "bad-hash") manifest.assets[0].sha256 = "not a digest";
    if (mode === "report-path") manifest.verificationReport = "../outside.json";
    assert.throws(() => verifyProvenance({ assetsDir: f.assetsDir, manifest, ...f.policy }));
  }
  assert.throws(() => createProvenance({ assetsDir: f.assetsDir, ...f.policy, verificationReport: "../outside.json" }), /directly inside/);
});

test("published metadata must identify exact source, tag and public channel", (t) => {
  const f = fixture(t);
  const original = published(f.create());
  for (const changed of [{ tag_name: "v0.0.75" }, { target_commitish: "main" }, { target_commitish: "c".repeat(40) }, { draft: true }, { prerelease: true }]) {
    const data = clone(original);
    Object.assign(data.release, changed);
    assert.throws(() => validatePublishedRelease(data));
  }
  assert.throws(() => validatePublishedRelease({ ...original, expectedCommit: "c".repeat(40) }), /source commit differs/);
});

test("stable latest download must identify the exact release; beta cannot become latest", (t) => {
  const stable = published(fixture(t).create());
  for (const latestRelease of [undefined, null, { tag_name: "v0.0.75", draft: false, prerelease: false }, { tag_name: stable.release.tag_name, draft: false, prerelease: true }]) {
    assert.throws(() => validatePublishedRelease({ ...stable, latestRelease }), /Latest stable|latest stable/);
  }
  const beta = published(fixture(t, { tag: "v0.0.76-beta.132", channel: "beta" }).create());
  validatePublishedRelease(beta);
  validatePublishedRelease({ ...beta, latestRelease: null });
  assert.throws(() => validatePublishedRelease({ ...beta, latestRelease: beta.release }), /Beta release must not replace/);
});

test("published metadata rejects missing, extra, duplicate, unuploaded or modified files", (t) => {
  const f = fixture(t);
  const original = published(f.create());
  for (const mode of ["missing", "extra", "duplicate", "size", "digest", "missing-digest", "state", "manifest-bytes"]) {
    const data = clone(original);
    if (mode === "missing") data.release.assets.pop();
    if (mode === "extra") data.release.assets.push({ name: "extra.json", size: 1, digest: `sha256:${"0".repeat(64)}`, state: "uploaded" });
    if (mode === "duplicate") data.release.assets[1] = clone(data.release.assets[0]);
    if (mode === "size") data.release.assets[0].size += 1;
    if (mode === "digest") data.release.assets[0].digest = `sha256:${"0".repeat(64)}`;
    if (mode === "missing-digest") delete data.release.assets[0].digest;
    if (mode === "state") data.release.assets[0].state = "starter";
    if (mode === "manifest-bytes") data.release.assets.find((asset) => asset.name === MANIFEST_NAME).digest = `sha256:${"0".repeat(64)}`;
    assert.throws(() => validatePublishedRelease(data));
  }
});

test("CLI uses explicit policy environment, verifies metadata, rejects typos and never overwrites manifests", (t) => {
  const f = fixture(t);
  const env = { ...process.env, RELEASE_ASSETS_DIR: f.assetsDir, RELEASE_SOURCE_COMMIT: COMMIT, RELEASE_TAG: f.policy.tag, RELEASE_CHANNEL: f.policy.channel, RELEASE_WINDOWS_SIGNED: "true", RELEASE_EXPECTED_THUMBPRINT: THUMBPRINT };
  const run = (...args) => spawnSync(process.execPath, [script, ...args], { env, encoding: "utf8" });
  assert.equal(run("create").status, 0);
  assert.notEqual(run("create").status, 0);
  assert.equal(run("verify").status, 0);
  assert.notEqual(run("verify", "--windows-signed", "yes").status, 0);
  assert.notEqual(run("verify", "--typo", "value").status, 0);
  const manifest = JSON.parse(fs.readFileSync(path.join(f.assetsDir, MANIFEST_NAME), "utf8"));
  const data = published(manifest);
  const metadataDirectory = fs.mkdtempSync(path.join(os.tmpdir(), "abigail-metadata-test-"));
  t.after(() => fs.rmSync(metadataDirectory, { recursive: true, force: true }));
  const releaseFile = path.join(metadataDirectory, "release.json");
  const latestFile = path.join(metadataDirectory, "latest.json");
  fs.writeFileSync(releaseFile, JSON.stringify(data.release));
  fs.writeFileSync(latestFile, JSON.stringify(data.latestRelease));
  const result = run("verify-metadata", "--manifest", path.join(f.assetsDir, MANIFEST_NAME), "--release", releaseFile, "--latest-release", latestFile);
  assert.equal(result.status, 0, result.stderr);
});
