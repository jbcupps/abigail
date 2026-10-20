#!/usr/bin/env node
// All ready reports and remote replies below are synthetic fixtures. No signing,
// installer execution, profile access, credentials, or network calls occur.
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import crypto from "node:crypto";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import {
  validatePreviewDescriptor, loadPreviewDescriptor, renderPreview, startPreviewPage,
  REPOSITORY, INSTALLER_NAME, EXPECTED_CERTIFICATE_THUMBPRINT, PUBLIC_STAGES,
  COMPATIBILITY_STAGES, STABLE_BASELINE, releaseAssetUrl, MAX_DESCRIPTOR_BYTES,
} from "../../docs/preview/preview.mjs";
import { verifyPublicPreviewEvidence } from "../preview_channel.mjs";

const SOURCE = "a".repeat(40);
const EVIDENCE_COMMIT = "b".repeat(40);
const TAG = "v0.0.77-beta.123";
const INSTALLER_HASH = "c".repeat(64);
const PAGE = "https://jbcupps.github.io/abigail/preview/";
const hash = (content) => crypto.createHash("sha256").update(content).digest("hex");
const json = (value) => `${JSON.stringify(value, null, 2)}\n`;
const clone = (value) => structuredClone(value);
const stages = (names) => names.map((name) => ({ name, status: "passed", details: `Synthetic fixture observation for ${name}; not real acceptance evidence.` }));
function envelope(report) {
  const content = json(report);
  const sha256 = hash(content);
  return {
    url: `https://raw.githubusercontent.com/${REPOSITORY}/${EVIDENCE_COMMIT}/docs/preview/evidence/${sha256}.json`,
    sha256, content,
  };
}
function editReport(descriptor, key, change) {
  const report = JSON.parse(descriptor.evidence[key].content);
  change(report);
  descriptor.evidence[key] = envelope(report);
}
function fixture() {
  const signature = {
    name: INSTALLER_NAME, sha256: INSTALLER_HASH, signatureStatus: "Valid",
    publisher: "Synthetic fixture publisher", certificateThumbprint: EXPECTED_CERTIFICATE_THUMBPRINT,
    timestampType: "RFC3161",
  };
  const signatures = {
    schemaVersion: 1, sourceCommit: SOURCE, tag: TAG, channel: "beta",
    windowsSigned: true, expectedThumbprint: EXPECTED_CERTIFICATE_THUMBPRINT, artifacts: [signature],
  };
  const signaturesContent = json(signatures);
  const provenance = {
    schemaVersion: 1, sourceCommit: SOURCE, tag: TAG, channel: "beta",
    windowsSigned: true, expectedThumbprint: EXPECTED_CERTIFICATE_THUMBPRINT,
    verificationReport: "windows-signature-evidence.json",
    assets: [
      { name: INSTALLER_NAME, size: 123456, sha256: INSTALLER_HASH },
      { name: "windows-signature-evidence.json", size: Buffer.byteLength(signaturesContent), sha256: hash(signaturesContent) },
    ],
  };
  const common = {
    schemaVersion: 1, repository: REPOSITORY, channel: "beta", tag: TAG,
    sourceCommit: SOURCE, releaseId: 123, installerSha256: INSTALLER_HASH,
    certificateThumbprint: EXPECTED_CERTIFICATE_THUMBPRINT, verifiedUtc: "2026-10-20T12:00:00Z",
  };
  const descriptor = {
    schemaVersion: 1, status: "ready", updatePolicy: "manual-opt-in",
    release: {
      tag: TAG, sourceCommit: SOURCE, releaseId: 123,
      certificateThumbprint: EXPECTED_CERTIFICATE_THUMBPRINT,
      installer: { name: INSTALLER_NAME, size: 123456, sha256: INSTALLER_HASH },
    },
    knownGaps: ["Synthetic fixture only; never select for publication."],
    checklist: ["Use a disposable test profile.", "Close the preview before installing another build."],
    evidence: {
      publicVerification: envelope({
        ...common, kind: "preview-public-verification", stages: stages(PUBLIC_STAGES),
        sourceCi: { workflowRunId: 1233, runAttempt: 1, sourceCommit: SOURCE, workflowPath: ".github/workflows/ci.yml", branch: "beta", event: "push" },
        verification: {
          verified: true, repository: REPOSITORY, tag: TAG, sourceCommit: SOURCE, channel: "beta",
          installerSha256: INSTALLER_HASH, downloadUrl: releaseAssetUrl(TAG, INSTALLER_NAME), verifiedUtc: common.verifiedUtc,
        },
        signatures, provenance,
        stableRoute: Object.fromEntries(["before", "after"].map((phase) => [phase, {
          ...STABLE_BASELINE, verified: true,
          downloadUrl: `https://github.com/${REPOSITORY}/releases/latest/download/${INSTALLER_NAME}`,
        }])),
      }),
      compatibility: envelope({
        ...common, kind: "preview-compatibility", stages: stages(COMPATIBILITY_STAGES),
        baseline: { ...STABLE_BASELINE }, profileData: "synthetic-disposable",
        returnToStablePolicy: "preserved-stable-profile-and-restored-compatible-snapshot",
        execution: {
          workflowRunId: 1234, runAttempt: 1, sourceCommit: SOURCE, environment: "disposable-windows",
          harnessRevision: SOURCE, workflowPath: ".github/workflows/preview-compatibility.yml",
          commands: ["Synthetic fixture: run compatibility harness"],
        },
        continuity: {
          baseline: { kind: "seeded-first-preview", sourceCommit: SOURCE, seedSha256: "1".repeat(64) },
          entityIdBefore: "11111111-2222-3333-4444-555555555555",
          entityIdAfter: "11111111-2222-3333-4444-555555555555",
          restoredEntityId: "11111111-2222-3333-4444-555555555555",
          historySha256Before: "2".repeat(64), historySha256After: "2".repeat(64),
          restoredHistorySha256: "2".repeat(64),
          settingsSha256Before: "3".repeat(64), settingsSha256After: "3".repeat(64),
          coldSnapshotSha256: "4".repeat(64), creationFixtureId: "fresh-entity", ongoingFixtureId: "continuing-entity",
        },
      }),
    },
  };
  const provenanceContent = json(provenance);
  const release = {
    id: 123, tag_name: TAG, target_commitish: SOURCE, draft: false, prerelease: true,
    assets: [
      ...provenance.assets.map((asset) => ({ name: asset.name, size: asset.size, digest: `sha256:${asset.sha256}`, state: "uploaded" })),
      { name: "release-provenance.json", size: Buffer.byteLength(provenanceContent), digest: `sha256:${hash(provenanceContent)}`, state: "uploaded" },
    ],
  };
  const api = `https://api.github.com/repos/${REPOSITORY}`;
  const responses = new Map([
    [descriptor.evidence.publicVerification.url, descriptor.evidence.publicVerification.content],
    [descriptor.evidence.compatibility.url, descriptor.evidence.compatibility.content],
    [`${api}/releases/123`, json(release)],
    [`${api}/releases/latest`, json({
      tag_name: STABLE_BASELINE.tag, id: STABLE_BASELINE.releaseId,
      target_commitish: STABLE_BASELINE.sourceCommit, draft: false, prerelease: false,
      assets: [{ name: INSTALLER_NAME, size: STABLE_BASELINE.installerSize, digest: `sha256:${STABLE_BASELINE.installerSha256}`, state: "uploaded" }],
    })],
    [`${api}/git/ref/tags/${TAG}`, json({ object: { type: "commit", sha: SOURCE } })],
    [`${api}/git/ref/tags/${STABLE_BASELINE.tag}`, json({ object: { type: "commit", sha: STABLE_BASELINE.sourceCommit } })],
    [releaseAssetUrl(TAG, "release-provenance.json"), provenanceContent],
    [releaseAssetUrl(TAG, "windows-signature-evidence.json"), signaturesContent],
  ]);
  for (const [id, workflow] of [[1233, "ci"], [1234, "preview-compatibility"]]) {
    responses.set(`${api}/actions/runs/${id}`, json({
      id, repository: { full_name: REPOSITORY }, head_repository: { full_name: REPOSITORY },
      head_sha: SOURCE, run_attempt: 1, status: "completed", conclusion: "success",
      path: `.github/workflows/${workflow}.yml`, event: "push", head_branch: "beta",
    }));
  }
  responses.set(`${api}/actions/runs/1233/attempts/1/jobs?per_page=100`,
    json({ total_count: 1, jobs: [{ name: "gate", status: "completed", conclusion: "success" }] }));
  const fetcher = async (url, options) => {
    assert.equal(options.credentials, "omit");
    assert.equal(options.headers?.Authorization, undefined);
    assert.ok(responses.has(url), `Unexpected network request: ${url}`);
    return new Response(responses.get(url), { status: 200 });
  };
  return { descriptor, responses, fetcher, release, api };
}

test("checked-in descriptor satisfies the local policy for either allowed status", async () => {
  const descriptor = JSON.parse(fs.readFileSync(new URL("../../docs/preview/current.json", import.meta.url), "utf8"));
  assert.ok(["ready", "unavailable"].includes((await validatePreviewDescriptor(descriptor)).status));
});
test("an unavailable descriptor makes no public requests", async () => {
  const descriptor = { schemaVersion: 1, status: "unavailable", reason: "Synthetic pending checks." };
  const result = await verifyPublicPreviewEvidence(descriptor, () => assert.fail("Unavailable must not fetch."));
  assert.equal(result.downloadUrl, undefined);
});
test("valid synthetic evidence yields only exact tagged download and notes URLs", async () => {
  const { descriptor } = fixture();
  const result = await validatePreviewDescriptor(descriptor);
  assert.equal(result.downloadUrl, `https://github.com/${REPOSITORY}/releases/download/${TAG}/${INSTALLER_NAME}`);
  assert.equal(result.notesUrl, `https://github.com/${REPOSITORY}/releases/tag/${TAG}`);
});
for (const [name, mutate] of [
  ["unsupported schema", (d) => { d.schemaVersion = 2; }],
  ["stable tag", (d) => { d.release.tag = "v0.0.77"; }],
  ["moving source ref", (d) => { d.release.sourceCommit = "beta"; }],
  ["wrong publisher pin", (d) => { d.release.certificateThumbprint = "D".repeat(40); }],
  ["automatic updates", (d) => { d.updatePolicy = "automatic"; }],
  ["substituted installer", (d) => { d.release.installer.name = "Other.exe"; }],
  ["changed evidence bytes", (d) => { d.evidence.compatibility.content += " "; }],
  ["moving evidence URL", (d) => { d.evidence.compatibility.url = d.evidence.compatibility.url.replace(EVIDENCE_COMMIT, "main"); }],
  ["evidence from another repository", (d) => { d.evidence.compatibility.url = d.evidence.compatibility.url.replace(REPOSITORY, "other/project"); }],
  ["missing compatibility report", (d) => { delete d.evidence.compatibility; }],
  ["source-mismatched compatibility", (d) => editReport(d, "compatibility", (r) => { r.sourceCommit = "d".repeat(40); })],
  ["execution source mismatch", (d) => editReport(d, "compatibility", (r) => { r.execution.sourceCommit = "d".repeat(40); })],
  ["wrong baseline", (d) => editReport(d, "compatibility", (r) => { r.baseline.installerSha256 = "d".repeat(64); })],
  ["missing source CI identity", (d) => editReport(d, "publicVerification", (r) => { delete r.sourceCi; })],
  ["wrong stable before evidence", (d) => editReport(d, "publicVerification", (r) => { r.stableRoute.before.installerSha256 = "d".repeat(64); })],
  ["wrong stable after route", (d) => editReport(d, "publicVerification", (r) => { r.stableRoute.after.downloadUrl = releaseAssetUrl(TAG, INSTALLER_NAME); })],
  ["unreviewable compatibility harness", (d) => editReport(d, "compatibility", (r) => { r.execution.harnessRevision = "beta"; })],
  ["missing compatibility commands", (d) => editReport(d, "compatibility", (r) => { r.execution.commands = []; })],
  ["real profile evidence", (d) => editReport(d, "compatibility", (r) => { r.profileData = "personal"; })],
  ["binary downgrade claim", (d) => editReport(d, "compatibility", (r) => { r.returnToStablePolicy = "launch-stable-on-upgraded-profile"; })],
  ["failed native signature", (d) => editReport(d, "publicVerification", (r) => { r.signatures.artifacts[0].signatureStatus = "NotSigned"; })],
  ["missing timestamp", (d) => editReport(d, "publicVerification", (r) => { r.signatures.artifacts[0].timestampType = null; })],
  ["failed published download", (d) => editReport(d, "publicVerification", (r) => { r.verification.verified = false; })],
  ["provenance installer mismatch", (d) => editReport(d, "publicVerification", (r) => { r.provenance.assets[0].sha256 = "d".repeat(64); })],
  ["aggregate pass with no stage evidence", (d) => editReport(d, "compatibility", (r) => { r.passed = true; r.stages = []; })],
  ["duplicate stages", (d) => editReport(d, "compatibility", (r) => { r.stages.push(clone(r.stages[0])); })],
  ["relabelled fresh fixture", (d) => editReport(d, "compatibility", (r) => { r.continuity.ongoingFixtureId = r.continuity.creationFixtureId; })],
  ["changed continuing Entity", (d) => editReport(d, "compatibility", (r) => { r.continuity.entityIdAfter = "dddddddd-dddd-dddd-dddd-dddddddddddd"; })],
  ["lost continuing history", (d) => editReport(d, "compatibility", (r) => { r.continuity.historySha256After = "d".repeat(64); })],
  ["lost continuing settings", (d) => editReport(d, "compatibility", (r) => { r.continuity.settingsSha256After = "d".repeat(64); })],
  ["wrong restored Entity", (d) => editReport(d, "compatibility", (r) => { r.continuity.restoredEntityId = "dddddddd-dddd-dddd-dddd-dddddddddddd"; })],
  ["wrong restored history", (d) => editReport(d, "compatibility", (r) => { r.continuity.restoredHistorySha256 = "d".repeat(64); })],
  ["missing cold snapshot", (d) => editReport(d, "compatibility", (r) => { delete r.continuity.coldSnapshotSha256; })],
]) {
  test(`rejects ${name}`, async () => {
    const { descriptor } = fixture();
    mutate(descriptor);
    await assert.rejects(validatePreviewDescriptor(descriptor));
  });
}
test("each required compatibility stage is necessary and must pass", async () => {
  for (const name of COMPATIBILITY_STAGES) {
    const { descriptor } = fixture();
    editReport(descriptor, "compatibility", (r) => { r.stages = r.stages.filter((s) => s.name !== name); });
    await assert.rejects(validatePreviewDescriptor(descriptor), /missing/);
    const failed = fixture().descriptor;
    editReport(failed, "compatibility", (r) => { r.stages.find((s) => s.name === name).status = "skipped"; });
    await assert.rejects(validatePreviewDescriptor(failed), /incomplete or failed/);
  }
});
test("unavailable descriptor cannot smuggle a download", async () => {
  await assert.rejects(validatePreviewDescriptor({ schemaVersion: 1, status: "unavailable", reason: "Pending", release: fixture().descriptor.release }));
});
test("CLI validates live identity, tag target, asset digests and public report bytes", async () => {
  const { descriptor, fetcher } = fixture();
  assert.equal((await verifyPublicPreviewEvidence(descriptor, fetcher)).status, "ready");
});
test("public evidence cannot hide invalid UTF-8 behind replacement characters", async () => {
  const { descriptor } = fixture();
  await assert.rejects(verifyPublicPreviewEvidence(descriptor, async () => new Response(new Uint8Array([0xff]))));
});
for (const [name, mutate] of [
  ["wrong tag target", (f) => f.responses.set(`${f.api}/git/ref/tags/${TAG}`, json({ object: { type: "commit", sha: "d".repeat(40) } }))],
  ["different release ID", (f) => { f.release.id = 456; f.responses.set(`${f.api}/releases/123`, json(f.release)); }],
  ["stable release substituted", (f) => { f.release.prerelease = false; f.responses.set(`${f.api}/releases/123`, json(f.release)); }],
  ["mutated installer digest", (f) => { f.release.assets[0].digest = `sha256:${"d".repeat(64)}`; f.responses.set(`${f.api}/releases/123`, json(f.release)); }],
  ["missing asset", (f) => { f.release.assets.pop(); f.responses.set(`${f.api}/releases/123`, json(f.release)); }],
  ["changed public evidence", (f) => f.responses.set(f.descriptor.evidence.compatibility.url, "{}")],
  ["latest stable changed", (f) => f.responses.set(`${f.api}/releases/latest`, json({ tag_name: "v0.0.78", draft: false, prerelease: false }))],
  ["same stable tag new release ID", (f) => {
    const data = JSON.parse(f.responses.get(`${f.api}/releases/latest`)); data.id++;
    f.responses.set(`${f.api}/releases/latest`, json(data));
  }],
  ["same stable tag new digest", (f) => {
    const data = JSON.parse(f.responses.get(`${f.api}/releases/latest`)); data.assets[0].digest = `sha256:${"d".repeat(64)}`;
    f.responses.set(`${f.api}/releases/latest`, json(data));
  }],
  ["moved stable source", (f) => f.responses.set(`${f.api}/git/ref/tags/${STABLE_BASELINE.tag}`, json({ object: { type: "commit", sha: SOURCE } }))],
  ["failed source gate", (f) => f.responses.set(`${f.api}/actions/runs/1233/attempts/1/jobs?per_page=100`,
    json({ total_count: 1, jobs: [{ name: "gate", status: "completed", conclusion: "failure" }] }))],
  ["incomplete source job metadata", (f) => f.responses.set(`${f.api}/actions/runs/1233/attempts/1/jobs?per_page=100`,
    json({ total_count: 2, jobs: [{ name: "gate", status: "completed", conclusion: "success" }] }))],
  ["source CI commit mismatch", (f) => {
    const data = JSON.parse(f.responses.get(`${f.api}/actions/runs/1233`)); data.head_sha = "d".repeat(40);
    f.responses.set(`${f.api}/actions/runs/1233`, json(data));
  }],
  ["failed compatibility run", (f) => {
    const data = JSON.parse(f.responses.get(`${f.api}/actions/runs/1234`)); data.conclusion = "failure";
    f.responses.set(`${f.api}/actions/runs/1234`, json(data));
  }],
  ["compatibility attempt mismatch", (f) => {
    const data = JSON.parse(f.responses.get(`${f.api}/actions/runs/1234`)); data.run_attempt++;
    f.responses.set(`${f.api}/actions/runs/1234`, json(data));
  }],
]) {
  test(`public validation rejects ${name}`, async () => {
    const f = fixture();
    mutate(f);
    await assert.rejects(verifyPublicPreviewEvidence(f.descriptor, f.fetcher));
  });
}
test("annotated tags resolve to the same exact source", async () => {
  const f = fixture();
  f.responses.set(`${f.api}/git/ref/tags/${TAG}`, json({ object: { type: "tag", sha: EVIDENCE_COMMIT } }));
  f.responses.set(`${f.api}/git/tags/${EVIDENCE_COMMIT}`, json({ object: { type: "commit", sha: SOURCE } }));
  assert.equal((await verifyPublicPreviewEvidence(f.descriptor, f.fetcher)).status, "ready");
});
test("same-origin descriptor fetch omits credentials and rejects redirects", async () => {
  const unavailable = { schemaVersion: 1, status: "unavailable", reason: "Pending." };
  await loadPreviewDescriptor("./current.json", PAGE, async (url, options) => {
    assert.equal(url, `${PAGE}current.json`);
    assert.equal(options.credentials, "omit");
    assert.equal(options.redirect, "error");
    assert.equal(options.mode, "same-origin");
    return new Response(json(unavailable));
  });
  await assert.rejects(loadPreviewDescriptor("https://example.com/current.json", PAGE, () => assert.fail("Must not fetch.")));
  await assert.rejects(loadPreviewDescriptor("./current.json", PAGE, async () => ({ ok: true, redirected: true })));
  await assert.rejects(loadPreviewDescriptor("./current.json", PAGE, async () => new Response("x".repeat(MAX_DESCRIPTOR_BYTES + 1))));
});
function fakeDocument() {
  class Element {
    children = []; hidden = false; textContent = "";
    append(child) { this.children.push(child); }
    replaceChildren() { this.children = []; }
    removeAttribute(name) { delete this[name]; }
    set innerHTML(_) { assert.fail("HTML injection is forbidden."); }
  }
  const nodes = new Map();
  return {
    getElementById(id) { if (!nodes.has(id)) nodes.set(id, new Element()); return nodes.get(id); },
    createElement() { return new Element(); },
  };
}
test("rendering uses text and removes links on subsequent unavailable/error states", async () => {
  const doc = fakeDocument();
  const { descriptor } = fixture();
  descriptor.knownGaps = ["<img src=x onerror=alert(1)>"];
  renderPreview(doc, await validatePreviewDescriptor(descriptor));
  assert.equal(doc.getElementById("preview-gaps").children[0].textContent, descriptor.knownGaps[0]);
  assert.equal(doc.getElementById("preview-ready").hidden, false);
  await startPreviewPage(doc, { href: PAGE }, async () => new Response("{invalid"));
  assert.equal(doc.getElementById("preview-ready").hidden, true);
  assert.equal(doc.getElementById("preview-download").href, undefined);
  assert.equal(doc.getElementById("preview-notes").href, undefined);
  assert.equal(doc.getElementById("preview-evidence").children.length, 0);
});
test("page has no download link until validation and unavailable CLI succeeds offline", (t) => {
  const html = fs.readFileSync(new URL("../../docs/preview/index.html", import.meta.url), "utf8");
  assert.match(html, /id="preview-ready" hidden/);
  assert.match(html, /<a id="preview-download" class="download">/);
  assert.doesNotMatch(html, /http-equiv=["']refresh|download\s*=/i);
  const script = fileURLToPath(new URL("../preview_channel.mjs", import.meta.url));
  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), "abigail-preview-fixture-"));
  t.after(() => fs.rmSync(temporary, { recursive: true, force: true }));
  const descriptor = path.join(temporary, "current.json");
  fs.writeFileSync(descriptor, json({ schemaVersion: 1, status: "unavailable", reason: "Synthetic pending checks." }));
  const run = spawnSync(process.execPath, [script, "validate", descriptor], { encoding: "utf8" });
  assert.equal(run.status, 0, run.stderr);
  assert.match(run.stdout, /unavailable; no installer link/);
});
