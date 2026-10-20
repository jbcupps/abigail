// Shared, dependency-free descriptor policy for the page and the offline tests.
// The checked-in descriptor is the curated authority. Hashes bind exact public
// report bytes; they are not a replacement for the reported Windows checks.
import { REVIEWED_WORKFLOW_BINDINGS, REVIEWED_CANDIDATE_ORIGINS } from "./execution-bindings.mjs";
export const REPOSITORY = "jbcupps/abigail";
export const INSTALLER_NAME = "Abigail-windows-x64-setup.exe";
export const EXPECTED_CERTIFICATE_THUMBPRINT = "69BE78FD201A59A29C46F5D37097E0C6DDA04A41";
export const MAX_DESCRIPTOR_BYTES = 300 * 1024;
export const MAX_EVIDENCE_BYTES = 128 * 1024;
export const PUBLIC_STAGES = Object.freeze([
  "source-ci-gate", "tag-source", "prerelease-metadata", "published-asset-hashes",
  "authenticode-timestamp", "stable-route-unchanged",
]);
export const COMPATIBILITY_STAGES = Object.freeze([
  "separate-installer-identity", "stable-preview-coexistence", "profile-path-isolation",
  "daemon-ownership-isolation", "fresh-creation-fixture", "ongoing-entity-upgrade",
  "ongoing-entity-reopen", "full-cold-backup-restore", "newer-schema-refusal", "return-to-stable",
]);
const SHA40 = /^[a-f0-9]{40}$/;
const SHA256 = /^[a-f0-9]{64}$/;
const BETA_TAG = /^v(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)-beta\.(?:0|[1-9]\d*)$/;
const ENCODER = new TextEncoder();
export const STABLE_BASELINE = Object.freeze({
  tag: "v0.0.76",
  sourceCommit: "e3ad51181da1ea11bded24ca0c39e2dcfe5fa944",
  releaseId: 418137720,
  installerSha256: "791b97fb9c5c7ef2c776c52a5e2b41071bb7cf40fcd3226099cdf693185acbb1",
  installerSize: 86983472,
});

function requireCondition(condition, message) {
  if (!condition) throw new Error(message);
}
function object(value, label) {
  requireCondition(value && typeof value === "object" && !Array.isArray(value), `${label} must be an object.`);
}
function text(value, label, maximum = 1500) {
  requireCondition(typeof value === "string" && value.trim().length > 0 && value.length <= maximum &&
    !/[\u0000-\u0008\u000b\u000c\u000e-\u001f]/.test(value), `${label} must be bounded readable text.`);
}
function positiveInteger(value, label) {
  requireCondition(Number.isSafeInteger(value) && value > 0, `${label} must be a positive safe integer.`);
}
function textList(value, label, minimum = 0) {
  requireCondition(Array.isArray(value) && value.length >= minimum && value.length <= 20, `${label} must be a bounded list.`);
  value.forEach((entry) => text(entry, label));
}
function assertStages(stages, required, label) {
  requireCondition(Array.isArray(stages) && stages.length >= required.length && stages.length <= 40, `${label} has missing stages.`);
  const names = new Set();
  for (const stage of stages) {
    object(stage, label);
    text(stage.name, `${label} stage name`, 100);
    requireCondition(!names.has(stage.name), `${label} has duplicate stages.`);
    names.add(stage.name);
    requireCondition(stage.status === "passed", `${label} has an incomplete or failed stage.`);
    text(stage.details, `${label} stage observations`);
  }
  for (const name of required) requireCondition(names.has(name), `${label} is missing ${name}.`);
}
export async function sha256Text(value) {
  requireCondition(globalThis.crypto?.subtle, "SHA-256 verification is unavailable.");
  const bytes = await globalThis.crypto.subtle.digest("SHA-256", ENCODER.encode(value));
  return Array.from(new Uint8Array(bytes), (byte) => byte.toString(16).padStart(2, "0")).join("");
}
export function releaseAssetUrl(tag, name) {
  requireCondition(BETA_TAG.test(tag), "Preview tags must use the beta convention.");
  requireCondition([INSTALLER_NAME, "release-provenance.json", "windows-signature-evidence.json", "hosted-provenance.json"].includes(name), "Unexpected preview asset.");
  return `https://github.com/${REPOSITORY}/releases/download/${tag}/${name}`;
}
export function reviewedExecutionBinding(execution, sourceCommit, purpose) {
  object(execution, "Workflow execution");
  requireCondition(typeof execution.sourceBinding === "string" &&
    Object.hasOwn(REVIEWED_WORKFLOW_BINDINGS, execution.sourceBinding),
    "Workflow product checkout has no matching reviewed execution binding.");
  const binding = REVIEWED_WORKFLOW_BINDINGS[execution.sourceBinding];
  requireCondition(binding && binding.purpose === purpose &&
    binding.productSourceCommit === sourceCommit && execution.sourceCommit === sourceCommit &&
    execution.harnessRevision === binding.harnessRevision &&
    execution.workflowPath === binding.workflowPath && execution.event === binding.event &&
    (binding.branch === undefined || execution.branch === binding.branch),
    "Workflow product checkout has no matching reviewed execution binding.");
  return binding;
}
async function validateCandidateOrigin(origin, release) {
  object(origin, "Candidate origin");
  const binding = reviewedExecutionBinding(origin, release.sourceCommit, "candidate-origin");
  const reviewed = REVIEWED_CANDIDATE_ORIGINS[origin.sourceBinding];
  requireCondition(reviewed && ["workflowRunId", "runAttempt", "artifactId", "artifactName", "artifactSha256"]
    .every((name) => origin[name] === reviewed[name]), "Candidate origin run or artifact differs from the reviewed bytes.");
  requireCondition(release.installer.sha256 === reviewed.installerSha256 &&
    release.installer.size === reviewed.installerSize, "Candidate origin installer differs from the preview.");
  object(origin.hostedProvenance, "Original hosted provenance");
  const original = origin.hostedProvenance;
  text(original.content, "Original hosted provenance content", MAX_EVIDENCE_BYTES);
  requireCondition(original.sha256 === reviewed.provenanceSha256 &&
    ENCODER.encode(original.content).length === reviewed.provenanceSize &&
    await sha256Text(original.content) === reviewed.provenanceSha256,
    "Original hosted provenance bytes differ from the reviewed candidate.");
  let provenance;
  try { provenance = JSON.parse(original.content); } catch { throw new Error("Original hosted provenance is not JSON."); }
  requireCondition(provenance.schemaVersion === 1 && provenance.repository === REPOSITORY &&
    provenance.productSourceCommit === release.sourceCommit &&
    provenance.harnessSourceCommit === binding.harnessRevision &&
    provenance.workflowRunId === String(reviewed.workflowRunId) &&
    provenance.workflowRunAttempt === String(reviewed.runAttempt) &&
    provenance.acceptanceOutcome === "success" &&
    provenance.publicationEligible === false && provenance.realModelValidation === false &&
    Array.isArray(provenance.files), "Original candidate provenance must retain its actual execution and limited claims.");
  const installers = provenance.files.filter((file) => file.name === INSTALLER_NAME);
  requireCondition(installers.length === 1 && installers[0].sha256 === release.installer.sha256 &&
    installers[0].size === release.installer.size, "Original candidate provenance does not identify this installer.");
}
function assertReportIdentity(report, release, kind) {
  object(report, kind);
  requireCondition(report.schemaVersion === 1 && report.kind === kind, "Unexpected evidence schema or kind.");
  requireCondition(report.repository === REPOSITORY && report.channel === "beta" &&
    report.tag === release.tag && report.sourceCommit === release.sourceCommit &&
    report.releaseId === release.releaseId && report.installerSha256 === release.installer.sha256 &&
    report.certificateThumbprint === release.certificateThumbprint, "Evidence does not identify this exact preview.");
  text(report.verifiedUtc, "Evidence verification time", 50);
  requireCondition(/^\d{4}-\d\d-\d\dT/.test(report.verifiedUtc) && Number.isFinite(Date.parse(report.verifiedUtc)),
    "Evidence verification time is invalid.");
}
async function validateEvidence(envelope, release, key) {
  object(envelope, key);
  requireCondition(SHA256.test(envelope.sha256), "Evidence SHA-256 is invalid.");
  // Public verification happens after the release. Commit its report first,
  // then promote a descriptor pinned to that commit; never append release assets.
  const evidencePath = /^https:\/\/raw\.githubusercontent\.com\/jbcupps\/abigail\/[a-f0-9]{40}\/docs\/preview\/evidence\/([a-f0-9]{64})\.json$/;
  requireCondition(typeof envelope.url === "string" && evidencePath.exec(envelope.url)?.[1] === envelope.sha256,
    "Evidence must use a commit-pinned, content-addressed public report URL.");
  const url = envelope.url;
  text(envelope.content, "Evidence content", MAX_EVIDENCE_BYTES);
  requireCondition(ENCODER.encode(envelope.content).length <= MAX_EVIDENCE_BYTES, "Evidence exceeds the byte limit.");
  requireCondition(await sha256Text(envelope.content) === envelope.sha256, "Evidence content hash does not match.");
  let report;
  try { report = JSON.parse(envelope.content); } catch { throw new Error("Evidence is not JSON."); }
  assertReportIdentity(report, release, key === "publicVerification" ? "preview-public-verification" : "preview-compatibility");
  if (key === "publicVerification") {
    assertStages(report.stages, PUBLIC_STAGES, "Public verification");
    object(report.sourceCi, "Source CI evidence");
    positiveInteger(report.sourceCi.workflowRunId, "Source CI run");
    positiveInteger(report.sourceCi.runAttempt, "Source CI run attempt");
    requireCondition(report.sourceCi.sourceCommit === release.sourceCommit &&
      report.sourceCi.workflowPath === ".github/workflows/ci.yml",
      "Source CI evidence must identify the exact product and CI workflow.");
    if (report.sourceCi.sourceBinding !== undefined) {
      reviewedExecutionBinding(report.sourceCi, release.sourceCommit, "source-ci");
      text(report.sourceCi.branch, "Source CI dispatch branch", 200);
    } else {
      requireCondition(report.sourceCi.branch === "beta" && report.sourceCi.event === "push",
        "Unbound source CI evidence must identify the exact beta push.");
    }
    const knownCandidate = Object.values(REVIEWED_CANDIDATE_ORIGINS)
      .some((origin) => origin.installerSha256 === release.installer.sha256);
    if (report.candidateOrigin !== undefined || knownCandidate) {
      await validateCandidateOrigin(report.candidateOrigin, release);
    }
    // These two documents retain the existing verification scripts' schemas.
    const verification = report.verification;
    object(verification, "Published-download result");
    requireCondition(verification.verified === true && verification.repository === REPOSITORY &&
      verification.tag === release.tag && verification.sourceCommit === release.sourceCommit &&
      verification.channel === "beta" && typeof verification.installerSha256 === "string" &&
      verification.installerSha256.toLowerCase() === release.installer.sha256 &&
      verification.downloadUrl === releaseAssetUrl(release.tag, INSTALLER_NAME), "Published-download result differs from the preview.");
    const signatures = report.signatures;
    object(signatures, "Native signature evidence");
    requireCondition(signatures.schemaVersion === 1 && signatures.sourceCommit === release.sourceCommit &&
      signatures.tag === release.tag && signatures.channel === "beta" && signatures.windowsSigned === true &&
      signatures.expectedThumbprint === release.certificateThumbprint && Array.isArray(signatures.artifacts),
      "Native signature evidence differs from the preview.");
    const installers = signatures.artifacts.filter((artifact) => artifact.name === INSTALLER_NAME);
    requireCondition(installers.length === 1, "Native evidence must identify exactly one preview installer.");
    const signature = installers[0];
    requireCondition(signature.signatureStatus === "Valid" && signature.timestampType === "RFC3161" &&
      signature.certificateThumbprint === release.certificateThumbprint && signature.sha256 === release.installer.sha256,
      "Public Authenticode/timestamp evidence is invalid.");
    text(signature.publisher, "Public signature publisher", 300);
    object(report.provenance, "Release provenance");
    const provenance = report.provenance;
    requireCondition(provenance.schemaVersion === 1 && provenance.sourceCommit === release.sourceCommit &&
      provenance.tag === release.tag && provenance.channel === "beta" && provenance.windowsSigned === true &&
      provenance.expectedThumbprint === release.certificateThumbprint && Array.isArray(provenance.assets),
      "Release provenance differs from the preview.");
    const assets = provenance.assets.filter((asset) => asset.name === INSTALLER_NAME);
    requireCondition(assets.length === 1 && assets[0].sha256 === release.installer.sha256 &&
      assets[0].size === release.installer.size, "Provenance installer identity differs from the preview.");
    object(report.stableRoute, "Stable route evidence");
    for (const phase of ["before", "after"]) {
      const route = report.stableRoute[phase];
      object(route, `Stable route ${phase}`);
      requireCondition(route.verified === true &&
        Object.entries(STABLE_BASELINE).every(([name, value]) => route[name] === value) &&
        route.downloadUrl === `https://github.com/${REPOSITORY}/releases/latest/download/${INSTALLER_NAME}`,
        `Stable route ${phase} evidence differs from the reviewed baseline.`);
    }
  } else {
    assertStages(report.stages, COMPATIBILITY_STAGES, "Compatibility");
    object(report.baseline, "Compatibility baseline");
    requireCondition(Object.entries(STABLE_BASELINE).every(([name, value]) => report.baseline[name] === value),
      "Compatibility must use the reviewed stable baseline.");
    requireCondition(report.profileData === "synthetic-disposable", "Compatibility must use synthetic disposable profile data.");
    requireCondition(report.returnToStablePolicy === "preserved-stable-profile-and-restored-compatible-snapshot",
      "Return to stable must preserve the stable profile and restore a compatible snapshot, not downgrade a newer profile.");
    object(report.execution, "Compatibility execution");
    positiveInteger(report.execution.workflowRunId, "Compatibility workflow run");
    positiveInteger(report.execution.runAttempt, "Compatibility run attempt");
    requireCondition(report.execution.sourceCommit === release.sourceCommit &&
      report.execution.environment === "disposable-windows", "Compatibility execution must match the preview on disposable Windows.");
    requireCondition(SHA40.test(report.execution.harnessRevision) &&
      /^\.github\/workflows\/[A-Za-z0-9_-]+\.ya?ml$/.test(report.execution.workflowPath),
      "Compatibility evidence must identify its immutable harness and workflow.");
    if (report.execution.sourceBinding !== undefined || report.execution.harnessRevision !== release.sourceCommit) {
      reviewedExecutionBinding(report.execution, release.sourceCommit, "compatibility");
    }
    textList(report.execution.commands, "Compatibility harness commands", 1);
    // Bind the reported ongoing Entity and restored snapshot. This checks
    // recorded observations, not actual profile contents or runtime behavior.
    const continuity = report.continuity;
    object(continuity, "Entity continuity");
    object(continuity.baseline, "Entity continuity baseline");
    requireCondition(["accepted-preview", "seeded-first-preview"].includes(continuity.baseline.kind) &&
      SHA40.test(continuity.baseline.sourceCommit), "Entity continuity baseline is invalid.");
    requireCondition(SHA256.test(continuity.baseline.kind === "accepted-preview"
      ? continuity.baseline.installerSha256 : continuity.baseline.seedSha256), "Entity continuity baseline needs its exact installer or seed hash.");
    requireCondition(/^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/.test(continuity.entityIdBefore) &&
      continuity.entityIdAfter === continuity.entityIdBefore && continuity.restoredEntityId === continuity.entityIdBefore,
      "The ongoing Entity identity did not survive upgrade and restore.");
    requireCondition(SHA256.test(continuity.historySha256Before) &&
      continuity.historySha256After === continuity.historySha256Before &&
      continuity.restoredHistorySha256 === continuity.historySha256Before,
      "The ongoing Entity history did not survive upgrade and restore.");
    requireCondition(SHA256.test(continuity.settingsSha256Before) &&
      continuity.settingsSha256After === continuity.settingsSha256Before &&
      SHA256.test(continuity.coldSnapshotSha256), "Entity settings or cold snapshot identity is invalid.");
    text(continuity.creationFixtureId, "Creation fixture ID", 100);
    text(continuity.ongoingFixtureId, "Ongoing fixture ID", 100);
    requireCondition(continuity.creationFixtureId !== continuity.ongoingFixtureId,
      "Fresh creation and ongoing Entity checks must use distinct fixtures.");
  }
  return { url, sha256: envelope.sha256, report };
}

export async function validatePreviewDescriptor(descriptor) {
  object(descriptor, "Preview descriptor");
  requireCondition(ENCODER.encode(JSON.stringify(descriptor)).length <= MAX_DESCRIPTOR_BYTES, "Descriptor exceeds the byte limit.");
  requireCondition(descriptor.schemaVersion === 1, "Unknown preview descriptor schema.");
  requireCondition(descriptor.status === "unavailable" || descriptor.status === "ready", "Unknown preview status.");
  if (descriptor.status === "unavailable") {
    text(descriptor.reason, "Unavailable reason");
    requireCondition(Object.keys(descriptor).every((key) => ["schemaVersion", "status", "reason"].includes(key)),
      "Unavailable previews must not contain release or download data.");
    return { status: "unavailable", reason: descriptor.reason };
  }
  requireCondition(descriptor.updatePolicy === "manual-opt-in", "Preview updates must be manual and opt-in.");
  const release = descriptor.release;
  object(release, "Release");
  requireCondition(BETA_TAG.test(release.tag) && SHA40.test(release.sourceCommit), "Preview tag or source commit is invalid.");
  positiveInteger(release.releaseId, "Release ID");
  requireCondition(release.certificateThumbprint === EXPECTED_CERTIFICATE_THUMBPRINT, "Preview signing certificate differs from the reviewed publisher.");
  object(release.installer, "Installer");
  requireCondition(release.installer.name === INSTALLER_NAME && SHA256.test(release.installer.sha256), "Preview installer identity is invalid.");
  positiveInteger(release.installer.size, "Installer size");
  textList(descriptor.knownGaps, "Known gaps");
  textList(descriptor.checklist, "Installation checklist", 1);
  object(descriptor.evidence, "Evidence");
  const publicVerification = await validateEvidence(descriptor.evidence.publicVerification, release, "publicVerification");
  const compatibility = await validateEvidence(descriptor.evidence.compatibility, release, "compatibility");
  return {
    status: "ready", release,
    downloadUrl: releaseAssetUrl(release.tag, INSTALLER_NAME),
    notesUrl: `https://github.com/${REPOSITORY}/releases/tag/${release.tag}`,
    knownGaps: [...descriptor.knownGaps], checklist: [...descriptor.checklist],
    evidence: { publicVerification, compatibility },
  };
}

export async function loadPreviewDescriptor(descriptorUrl, pageUrl, fetcher = globalThis.fetch) {
  const page = new URL(pageUrl);
  const url = new URL(descriptorUrl, page);
  requireCondition(page.protocol === "https:" || page.hostname === "localhost" || page.hostname === "127.0.0.1",
    "Preview information requires a secure page.");
  requireCondition(url.origin === page.origin && url.protocol === page.protocol && !url.username && !url.password,
    "Preview information must remain on the same origin.");
  const response = await fetcher(url.href, {
    credentials: "omit", mode: "same-origin", redirect: "error", cache: "no-store",
    signal: AbortSignal.timeout(15000),
  });
  requireCondition(response.ok && !response.redirected, "Preview information could not be fetched.");
  const content = await response.text();
  requireCondition(ENCODER.encode(content).length <= MAX_DESCRIPTOR_BYTES, "Descriptor exceeds the byte limit.");
  return validatePreviewDescriptor(JSON.parse(content));
}
export function renderPreview(document, preview) {
  const get = (id) => document.getElementById(id);
  const ready = get("preview-ready");
  ready.hidden = true;
  for (const id of ["preview-download", "preview-notes"]) get(id).removeAttribute("href");
  get("preview-evidence").replaceChildren();
  if (preview.status !== "ready") {
    get("preview-status").textContent = preview.reason;
    return;
  }
  get("preview-status").textContent = "This curated preview has matching report bytes and release information. The browser checks those records; Windows signature and compatibility checks are recorded in the reports. Review the checklist and known gaps before installing.";
  get("preview-title").textContent = preview.release.tag;
  get("preview-source").textContent = preview.release.sourceCommit;
  get("preview-sha256").textContent = preview.release.installer.sha256;
  get("preview-size").textContent = `${preview.release.installer.size.toLocaleString("en-US")} bytes`;
  get("preview-certificate").textContent = preview.release.certificateThumbprint;
  get("preview-download").href = preview.downloadUrl;
  get("preview-notes").href = preview.notesUrl;
  for (const [id, entries] of [
    ["preview-checklist", preview.checklist],
    ["preview-gaps", preview.knownGaps.length ? preview.knownGaps : ["No additional gaps are listed in the reviewed descriptor. See the reports for the tested scope."]],
  ]) {
    get(id).replaceChildren();
    for (const entry of entries) {
      const item = document.createElement("li");
      item.textContent = entry;
      get(id).append(item);
    }
  }
  for (const [name, evidence] of Object.entries(preview.evidence)) {
    const item = document.createElement("li");
    const link = document.createElement("a");
    link.href = evidence.url;
    link.textContent = name === "publicVerification" ? "Public download verification" : "Compatibility with the stable release";
    item.append(link);
    const hash = document.createElement("code");
    hash.textContent = ` — SHA-256 ${evidence.sha256}`;
    item.append(hash);
    get("preview-evidence").append(item);
  }
  ready.hidden = false;
}
export async function startPreviewPage(document, location, fetcher = globalThis.fetch) {
  try {
    const preview = await loadPreviewDescriptor(new URL("./current.json", location.href), location.href, fetcher);
    renderPreview(document, preview);
  } catch {
    renderPreview(document, {
      status: "unavailable",
      reason: "No verified preview can be offered right now. Preview information is missing or did not pass validation. Please use the stable release.",
    });
  }
}
if (typeof document !== "undefined" && typeof location !== "undefined") {
  void startPreviewPage(document, location);
}
