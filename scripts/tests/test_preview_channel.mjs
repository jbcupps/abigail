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
  reviewedExecutionBinding,
} from "../../docs/preview/preview.mjs";
import { REVIEWED_WORKFLOW_BINDINGS, REVIEWED_CANDIDATE_ORIGINS } from "../../docs/preview/execution-bindings.mjs";
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
function fixture({ source = SOURCE, installerHash = INSTALLER_HASH, installerSize = 123456 } = {}) {
  const signature = {
    name: INSTALLER_NAME, sha256: installerHash, signatureStatus: "Valid",
    publisher: "Synthetic fixture publisher", certificateThumbprint: EXPECTED_CERTIFICATE_THUMBPRINT,
    timestampType: "RFC3161",
  };
  const signatures = {
    schemaVersion: 1, sourceCommit: source, tag: TAG, channel: "beta",
    windowsSigned: true, expectedThumbprint: EXPECTED_CERTIFICATE_THUMBPRINT, artifacts: [signature],
  };
  const signaturesContent = json(signatures);
  const provenance = {
    schemaVersion: 1, sourceCommit: source, tag: TAG, channel: "beta",
    windowsSigned: true, expectedThumbprint: EXPECTED_CERTIFICATE_THUMBPRINT,
    verificationReport: "windows-signature-evidence.json",
    assets: [
      { name: INSTALLER_NAME, size: installerSize, sha256: installerHash },
      { name: "windows-signature-evidence.json", size: Buffer.byteLength(signaturesContent), sha256: hash(signaturesContent) },
    ],
  };
  const common = {
    schemaVersion: 1, repository: REPOSITORY, channel: "beta", tag: TAG,
    sourceCommit: source, releaseId: 123, installerSha256: installerHash,
    certificateThumbprint: EXPECTED_CERTIFICATE_THUMBPRINT, verifiedUtc: "2026-10-20T12:00:00Z",
  };
  const descriptor = {
    schemaVersion: 1, status: "ready", updatePolicy: "manual-opt-in",
    release: {
      tag: TAG, sourceCommit: source, releaseId: 123,
      certificateThumbprint: EXPECTED_CERTIFICATE_THUMBPRINT,
      installer: { name: INSTALLER_NAME, size: installerSize, sha256: installerHash },
    },
    knownGaps: ["Synthetic fixture only; never select for publication."],
    checklist: ["Use a disposable test profile.", "Close the preview before installing another build."],
    evidence: {
      publicVerification: envelope({
        ...common, kind: "preview-public-verification", stages: stages(PUBLIC_STAGES),
        sourceCi: { workflowRunId: 1233, runAttempt: 1, sourceCommit: source, workflowPath: ".github/workflows/ci.yml", branch: "beta", event: "push" },
        verification: {
          verified: true, repository: REPOSITORY, tag: TAG, sourceCommit: source, channel: "beta",
          installerSha256: installerHash, downloadUrl: releaseAssetUrl(TAG, INSTALLER_NAME), verifiedUtc: common.verifiedUtc,
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
          workflowRunId: 1234, runAttempt: 1, sourceCommit: source, environment: "disposable-windows",
          harnessRevision: source, workflowPath: ".github/workflows/preview-compatibility.yml",
          commands: ["Synthetic fixture: run compatibility harness"],
        },
        continuity: {
          baseline: { kind: "seeded-first-preview", sourceCommit: source, seedSha256: "1".repeat(64) },
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
    id: 123, tag_name: TAG, target_commitish: source, draft: false, prerelease: true,
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
    [`${api}/git/ref/tags/${TAG}`, json({ object: { type: "commit", sha: source } })],
    [`${api}/git/ref/tags/${STABLE_BASELINE.tag}`, json({ object: { type: "commit", sha: STABLE_BASELINE.sourceCommit } })],
    [releaseAssetUrl(TAG, "release-provenance.json"), provenanceContent],
    [releaseAssetUrl(TAG, "windows-signature-evidence.json"), signaturesContent],
  ]);
  for (const [id, workflow] of [[1233, "ci"], [1234, "preview-compatibility"]]) {
    responses.set(`${api}/actions/runs/${id}`, json({
      id, repository: { full_name: REPOSITORY }, head_repository: { full_name: REPOSITORY },
      head_sha: source, run_attempt: 1, status: "completed", conclusion: "success",
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

function candidateOriginFixture() {
  // Real public source/hash pins exercise proof verification. All release,
  // source-CI and full compatibility observations are still synthetic fixtures.
  const bindingId = "preview-candidate-39468905473";
  const binding = REVIEWED_WORKFLOW_BINDINGS[bindingId];
  const originPin = REVIEWED_CANDIDATE_ORIGINS[bindingId];
  const f = fixture({
    source: binding.productSourceCommit,
    installerHash: originPin.installerSha256,
    installerSize: originPin.installerSize,
  });
  const content = fs.readFileSync(new URL("./fixtures/preview-bindings/hosted-provenance-39468905473.json", import.meta.url), "utf8");
  const workflow = fs.readFileSync(new URL("./fixtures/preview-bindings/preview-candidate-dfeb.yml", import.meta.url), "utf8");
  const origin = {
    sourceBinding: bindingId, sourceCommit: binding.productSourceCommit,
    harnessRevision: binding.harnessRevision, workflowPath: binding.workflowPath, event: binding.event,
    ...Object.fromEntries(["workflowRunId", "runAttempt", "artifactId", "artifactName", "artifactSha256"]
      .map((name) => [name, originPin[name]])),
    hostedProvenance: { sha256: originPin.provenanceSha256, content },
  };
  editReport(f.descriptor, "publicVerification", (report) => {
    report.candidateOrigin = origin;
    report.provenance.assets.push({ name: "hosted-provenance.json", size: Buffer.byteLength(content), sha256: hash(content) });
  });
  const report = JSON.parse(f.descriptor.evidence.publicVerification.content);
  const manifestContent = json(report.provenance);
  f.responses.set(f.descriptor.evidence.publicVerification.url, f.descriptor.evidence.publicVerification.content);
  f.responses.set(releaseAssetUrl(TAG, "release-provenance.json"), manifestContent);
  f.responses.set(releaseAssetUrl(TAG, "hosted-provenance.json"), content);
  f.release.assets = [
    ...report.provenance.assets.map((asset) => ({ ...asset, digest: `sha256:${asset.sha256}`, state: "uploaded" })),
    { name: "release-provenance.json", size: Buffer.byteLength(manifestContent), digest: `sha256:${hash(manifestContent)}`, state: "uploaded" },
  ];
  f.responses.set(`${f.api}/releases/123`, json(f.release));
  f.responses.set(`${f.api}/actions/runs/${origin.workflowRunId}`, json({
    id: origin.workflowRunId, repository: { full_name: REPOSITORY }, head_repository: { full_name: REPOSITORY },
    head_sha: binding.harnessRevision, run_attempt: origin.runAttempt, status: "completed", conclusion: "success",
    path: binding.workflowPath, event: binding.event, head_branch: "dev/preview-combined-0.0.76",
  }));
  f.workflowUrl = `https://raw.githubusercontent.com/${REPOSITORY}/${binding.harnessRevision}/${binding.workflowPath}`;
  f.responses.set(f.workflowUrl, workflow);
  return { ...f, binding, origin, originPin };
}

function sourceCiFixture(bindingId = "preview-source-ci-c79e214") {
  const sourceBinding = REVIEWED_WORKFLOW_BINDINGS[bindingId];
  // Never relabel the historical candidate's actual artifact/provenance bytes.
  // The newer product uses wholly synthetic release/compatibility observations.
  const f = bindingId === "preview-source-ci-c79e214"
    ? candidateOriginFixture()
    : fixture({ source: sourceBinding.productSourceCommit });
  editReport(f.descriptor, "publicVerification", (report) => {
    Object.assign(report.sourceCi, {
      sourceBinding: bindingId, sourceCommit: sourceBinding.productSourceCommit,
      harnessRevision: sourceBinding.harnessRevision, workflowPath: sourceBinding.workflowPath,
      branch: sourceBinding.branch, event: sourceBinding.event,
    });
  });
  f.responses.set(f.descriptor.evidence.publicVerification.url, f.descriptor.evidence.publicVerification.content);
  const sourceRunUrl = `${f.api}/actions/runs/1233`;
  const run = JSON.parse(f.responses.get(sourceRunUrl));
  Object.assign(run, { head_sha: sourceBinding.harnessRevision, head_branch: sourceBinding.branch, event: sourceBinding.event });
  f.responses.set(sourceRunUrl, json(run));
  const sourceWorkflowUrl = `https://raw.githubusercontent.com/${REPOSITORY}/${sourceBinding.harnessRevision}/${sourceBinding.workflowPath}`;
  f.responses.set(sourceWorkflowUrl,
    fs.readFileSync(new URL(bindingId === "preview-source-ci-c79e214"
      ? "./fixtures/preview-bindings/source-ci-c79e214.yml"
      : "./fixtures/preview-bindings/source-ci-genesis.yml", import.meta.url), "utf8"));
  return { ...f, sourceBinding, sourceRunUrl, sourceWorkflowUrl };
}

for (const sourceBindingId of ["preview-source-ci-c79e214", "genesis-source-ci-a18249a"]) {
for (const [name, mutate, localError, onlineError] of [
  ["exact reviewed execution record", () => {}, null, null],
  ["self-asserted arbitrary harness and matching caller input", (ci, run) => {
    ci.harnessRevision = SOURCE;
    run.head_sha = SOURCE;
  }, /no matching reviewed execution binding/, /no matching reviewed execution binding/],
  ["claimed c79 with another actual harness", (_ci, run) => {
    run.head_sha = SOURCE;
  }, null, /Evidence workflow execution does not match/],
  ["stripped binding and beta-push relabel of another harness", (ci, run) => {
    delete ci.sourceBinding;
    ci.branch = "beta";
    ci.event = "push";
    run.head_sha = SOURCE;
  }, null, /Evidence workflow execution does not match/],
  ["stripped binding and beta-push relabel of a manual product-head run", (ci, run) => {
    delete ci.sourceBinding;
    ci.branch = "beta";
    ci.event = "push";
    run.head_sha = ci.sourceCommit;
    run.head_branch = "beta"; // Even matching legacy source/branch cannot turn dispatch into push.
  }, null, /Unbound source CI must be a beta push/],
]) {
  test(`${sourceBindingId}: source CI acceptance distinguishes ${name}`, async () => {
    const f = sourceCiFixture(sourceBindingId);
    const run = JSON.parse(f.responses.get(f.sourceRunUrl));
    editReport(f.descriptor, "publicVerification", (report) => {
      mutate(report.sourceCi, run);
      // These are caller claims, not GitHub API proof or reviewed authority.
      // Keep the claimed product fixed and the caller input consistent with
      // the actual run head even when that head is an unapproved harness.
      report.sourceCi.checkoutProof = { sourceCommit: report.sourceCi.sourceCommit, verified: true };
      report.sourceCi.dispatchInputs = { reviewed_harness_commit: run.head_sha };
      assert.equal(report.sourceCi.sourceCommit, f.sourceBinding.productSourceCommit);
    });
    f.responses.set(f.descriptor.evidence.publicVerification.url, f.descriptor.evidence.publicVerification.content);
    f.responses.set(f.sourceRunUrl, json(run));
    assert.equal(run.conclusion, "success");
    const jobs = JSON.parse(f.responses.get(`${f.api}/actions/runs/1233/attempts/1/jobs?per_page=100`));
    assert.equal(jobs.jobs[0].conclusion, "success");

    // The browser checks curated claims; actual execution is checked online.
    if (localError) await assert.rejects(validatePreviewDescriptor(f.descriptor), localError);
    else assert.equal((await validatePreviewDescriptor(f.descriptor)).status, "ready");
    if (onlineError) await assert.rejects(verifyPublicPreviewEvidence(f.descriptor, f.fetcher), onlineError);
    else assert.equal((await verifyPublicPreviewEvidence(f.descriptor, f.fetcher)).status, "ready");
  });
}

test(`${sourceBindingId}: reviewed source CI retains the existing required gate and advisory failure policy`, async () => {
  const f = sourceCiFixture(sourceBindingId);
  const run = JSON.parse(f.responses.get(f.sourceRunUrl));
  run.conclusion = "failure"; // Synthetic advisory audit failure; gate remains successful.
  f.responses.set(f.sourceRunUrl, json(run));
  assert.equal((await verifyPublicPreviewEvidence(f.descriptor, f.fetcher)).status, "ready");
});

for (const [name, mutate] of [
  ["unreviewed binding", (ci) => { ci.sourceBinding = "caller-invented"; }],
  ["candidate-origin authority substituted", (ci) => { ci.sourceBinding = "preview-candidate-39468905473"; }],
  ["missing binding", (ci) => { delete ci.sourceBinding; }],
  ["product substitution", (ci) => { ci.sourceCommit = SOURCE; }],
  ["workflow substitution", (ci) => { ci.workflowPath = ".github/workflows/preview-candidate.yml"; }],
  ["branch substitution", (ci) => { ci.branch = "dev/other-source-ci"; }],
  ["PR event substituted", (ci) => { ci.event = "pull_request"; }],
  ["push event substituted", (ci) => { ci.event = "push"; }],
]) {
  test(`${sourceBindingId}: reviewed source CI rejects ${name}`, async () => {
    const f = sourceCiFixture(sourceBindingId);
    editReport(f.descriptor, "publicVerification", (report) => mutate(report.sourceCi));
    await assert.rejects(validatePreviewDescriptor(f.descriptor));
  });
}

for (const [name, mutate] of [
  ["changed immutable workflow", (f) => { f.responses.set(f.sourceWorkflowUrl, `${f.responses.get(f.sourceWorkflowUrl)}\n# changed`); }],
  ["product incorrectly reported as run head", (f, run) => { run.head_sha = f.sourceBinding.productSourceCommit; }],
  ["another branch", (_f, run) => { run.head_branch = "dev/other-source-ci"; }],
  ["another workflow", (_f, run) => { run.path = ".github/workflows/preview-candidate.yml"; }],
  ["PR merge event", (_f, run) => { run.event = "pull_request"; }],
  ["different run attempt", (_f, run) => { run.run_attempt++; }],
  ["foreign repository", (_f, run) => { run.head_repository.full_name = "other/project"; }],
  ["unfinished run", (_f, run) => { run.status = "in_progress"; }],
  ["failed required gate", (f) => {
    f.responses.set(`${f.api}/actions/runs/1233/attempts/1/jobs?per_page=100`,
      json({ total_count: 1, jobs: [{ name: "gate", status: "completed", conclusion: "failure" }] }));
  }],
  ["skipped required gate", (f) => {
    f.responses.set(`${f.api}/actions/runs/1233/attempts/1/jobs?per_page=100`,
      json({ total_count: 1, jobs: [{ name: "gate", status: "completed", conclusion: "skipped" }] }));
  }],
]) {
  test(`${sourceBindingId}: online reviewed source CI rejects ${name}`, async () => {
    const f = sourceCiFixture(sourceBindingId);
    const run = JSON.parse(f.responses.get(f.sourceRunUrl));
    mutate(f, run);
    f.responses.set(f.sourceRunUrl, json(run));
    await assert.rejects(verifyPublicPreviewEvidence(f.descriptor, f.fetcher));
  });
}

test(`${sourceBindingId}: reviewed source CI cannot replace candidate origin or full compatibility acceptance`, async () => {
  const f = sourceCiFixture(sourceBindingId);
  const ci = JSON.parse(f.descriptor.evidence.publicVerification.content).sourceCi;
  for (const purpose of ["candidate-origin", "compatibility"]) {
    assert.throws(() => reviewedExecutionBinding(ci, f.sourceBinding.productSourceCommit, purpose), /no matching reviewed/);
  }
  editReport(f.descriptor, "compatibility", (report) => {
    report.execution = { ...ci, environment: "disposable-windows", commands: ["Synthetic source regression only"] };
  });
  await assert.rejects(validatePreviewDescriptor(f.descriptor), /no matching reviewed/);
});
}

test("Genesis source CI cannot reuse the historical product binding", async () => {
  const f = sourceCiFixture("genesis-source-ci-a18249a");
  editReport(f.descriptor, "publicVerification", report => {
    report.sourceCi.sourceBinding = "preview-source-ci-c79e214";
  });
  await assert.rejects(validatePreviewDescriptor(f.descriptor), /no matching reviewed execution binding/);
});

for (const [bindingId, filename] of [
  ["genesis-source-ci-a18249a", "source-ci-genesis.yml"],
  ["genesis-candidate-a18249a", "preview-candidate-genesis.yml"],
]) {
  test(`${bindingId}: exact Git workflow bytes bind the reviewed product and harness`, () => {
    const binding = REVIEWED_WORKFLOW_BINDINGS[bindingId];
    const bytes = fs.readFileSync(new URL(`./fixtures/preview-bindings/${filename}`, import.meta.url));
    assert.equal(hash(bytes), binding.workflowSha256);
    assert.ok(bytes.toString("utf8").includes(binding.productSourceCommit));
    assert.equal(binding.harnessRevision, "0bb0f4fd0cd181250cc62ed472699c5ea21c79a2");
    assert.equal(binding.branch, "dev/genesis-preview-harness");
    assert.equal(binding.event, "workflow_dispatch");
  });
}

test("Genesis candidate workflow binding alone cannot invent run or artifact acceptance", async () => {
  const bindingId = "genesis-candidate-a18249a";
  const binding = REVIEWED_WORKFLOW_BINDINGS[bindingId];
  assert.equal(Object.hasOwn(REVIEWED_CANDIDATE_ORIGINS, bindingId), false);
  const f = sourceCiFixture("genesis-source-ci-a18249a");
  const origin = {
    ...candidateOriginFixture().origin, sourceBinding: bindingId,
    sourceCommit: binding.productSourceCommit, harnessRevision: binding.harnessRevision,
    workflowPath: binding.workflowPath, branch: binding.branch, event: binding.event,
  };
  assert.equal(reviewedExecutionBinding(origin, binding.productSourceCommit, "candidate-origin"), binding);
  for (const purpose of ["source-ci", "compatibility"]) {
    assert.throws(() => reviewedExecutionBinding(origin, binding.productSourceCommit, purpose), /no matching reviewed/);
  }
  editReport(f.descriptor, "publicVerification", report => { report.candidateOrigin = origin; });
  await assert.rejects(validatePreviewDescriptor(f.descriptor), /Candidate origin run or artifact differs/);
  await assert.rejects(verifyPublicPreviewEvidence(f.descriptor, f.fetcher), /Candidate origin run or artifact differs/);
});

test("reviewed candidate-origin proof binds separate product/harness without replacing compatibility", async () => {
  const f = candidateOriginFixture();
  assert.notEqual(f.binding.productSourceCommit, f.binding.harnessRevision);
  assert.equal((await verifyPublicPreviewEvidence(f.descriptor, f.fetcher)).status, "ready");
});
for (const [name, mutate] of [
  ["omitted required candidate origin", (r) => { delete r.candidateOrigin; }],
  ["unreviewed binding", (r) => { r.candidateOrigin.sourceBinding = "caller-invented"; }],
  ["non-string binding", (r) => { r.candidateOrigin.sourceBinding = ["preview-candidate-39468905473"]; }],
  ["inherited binding", (r) => { r.candidateOrigin.sourceBinding = "__proto__"; }],
  ["candidate product substitution", (r) => { r.candidateOrigin.sourceCommit = SOURCE; }],
  ["candidate harness substitution", (r) => { r.candidateOrigin.harnessRevision = SOURCE; }],
  ["candidate workflow substitution", (r) => { r.candidateOrigin.workflowPath = ".github/workflows/ci.yml"; }],
  ["candidate event substitution", (r) => { r.candidateOrigin.event = "pull_request"; }],
  ["candidate run substitution", (r) => { r.candidateOrigin.workflowRunId++; }],
  ["candidate attempt substitution", (r) => { r.candidateOrigin.runAttempt++; }],
  ["candidate artifact substitution", (r) => { r.candidateOrigin.artifactId++; }],
  ["candidate artifact name substitution", (r) => { r.candidateOrigin.artifactName += "-other"; }],
  ["candidate artifact digest substitution", (r) => { r.candidateOrigin.artifactSha256 = "d".repeat(64); }],
  ["candidate original provenance rewritten", (r) => { r.candidateOrigin.hostedProvenance.content += "\n"; }],
  ["candidate original provenance relabelled as accepted", (r) => {
    const original = JSON.parse(r.candidateOrigin.hostedProvenance.content);
    original.publicationEligible = true;
    r.candidateOrigin.hostedProvenance.content = json(original);
    r.candidateOrigin.hostedProvenance.sha256 = hash(json(original));
  }],
]) {
  test(`candidate-origin binding rejects ${name}`, async () => {
    const f = candidateOriginFixture();
    editReport(f.descriptor, "publicVerification", mutate);
    await assert.rejects(validatePreviewDescriptor(f.descriptor));
  });
}
for (const [name, mutate] of [
  ["changed immutable workflow source", (f) => { f.responses.set(f.workflowUrl, `${f.responses.get(f.workflowUrl)}\n# changed`); }],
  ["candidate remote head incorrectly labelled as product", (f) => {
    const url = `${f.api}/actions/runs/${f.origin.workflowRunId}`;
    const run = JSON.parse(f.responses.get(url)); run.head_sha = f.binding.productSourceCommit;
    f.responses.set(url, json(run));
  }],
  ["candidate remote dispatch mismatch", (f) => {
    const url = `${f.api}/actions/runs/${f.origin.workflowRunId}`;
    const run = JSON.parse(f.responses.get(url)); run.event = "push";
    f.responses.set(url, json(run));
  }],
  ["published original provenance differs", (f) => { f.responses.set(releaseAssetUrl(TAG, "hosted-provenance.json"), "{}"); }],
  ["missing published original provenance", (f) => {
    f.release.assets = f.release.assets.filter((asset) => asset.name !== "hosted-provenance.json");
    f.responses.set(`${f.api}/releases/123`, json(f.release));
  }],
]) {
  test(`online candidate-origin verification rejects ${name}`, async () => {
    const f = candidateOriginFixture();
    mutate(f);
    await assert.rejects(verifyPublicPreviewEvidence(f.descriptor, f.fetcher));
  });
}
test("reviewed candidate-origin proof cannot serve as source CI or full compatibility", () => {
  const f = candidateOriginFixture();
  for (const purpose of ["source-ci", "compatibility"]) {
    assert.throws(() => reviewedExecutionBinding(f.origin, f.binding.productSourceCommit, purpose), /no matching reviewed/);
  }
});
test("candidate origin does not supply missing continuity or acceptance stages", async () => {
  for (const change of [
    (report) => { delete report.continuity; },
    (report) => { report.stages = []; },
    (report) => { report.execution = { ...candidateOriginFixture().origin, environment: "disposable-windows", commands: ["Synthetic only"] }; },
  ]) {
    const f = candidateOriginFixture();
    editReport(f.descriptor, "compatibility", change);
    await assert.rejects(validatePreviewDescriptor(f.descriptor));
  }
});
test("an arbitrary separate compatibility harness cannot assert its own checkout binding", async () => {
  const f = fixture();
  editReport(f.descriptor, "compatibility", (report) => {
    report.execution.harnessRevision = "d".repeat(40);
    report.execution.sourceBinding = "unreviewed-caller-proof";
    report.execution.event = "workflow_dispatch";
  });
  await assert.rejects(validatePreviewDescriptor(f.descriptor), /no matching reviewed/);
});
test("a PR merge run cannot be substituted for exact-product CI", async () => {
  const f = fixture();
  editReport(f.descriptor, "publicVerification", (report) => {
    report.sourceCi.event = "pull_request";
    report.sourceCi.branch = "dev/preview-combined-0.0.76";
  });
  await assert.rejects(validatePreviewDescriptor(f.descriptor), /exact beta push/);
});

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
