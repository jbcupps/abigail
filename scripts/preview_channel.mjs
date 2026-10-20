#!/usr/bin/env node
// Read-only readiness validation. Never publishes, installs, signs, or edits a
// pointer. Ready descriptors must match the actual public report bytes; unit
// tests inject a synthetic fetcher and do not make network requests.
import fs from "node:fs";
import { pathToFileURL } from "node:url";
import {
  validatePreviewDescriptor, MAX_DESCRIPTOR_BYTES, MAX_EVIDENCE_BYTES, sha256Text,
  REPOSITORY, STABLE_BASELINE, releaseAssetUrl,
} from "../docs/preview/preview.mjs";
import { validatePublishedRelease } from "./release_provenance.mjs";

async function publicText(url, fetcher, limit = MAX_EVIDENCE_BYTES) {
  const response = await fetcher(url, {
    credentials: "omit", redirect: "follow", signal: AbortSignal.timeout(30000),
    headers: { Accept: "application/json" },
  });
  if (!response.ok) throw new Error(`Public preview metadata/evidence unavailable (HTTP ${response.status}).`);
  const bytes = new Uint8Array(await response.arrayBuffer());
  if (bytes.byteLength > limit) throw new Error("Public metadata/evidence exceeds its byte limit.");
  // Reject invalid UTF-8 and preserve BOM bytes rather than silently normalizing
  // downloaded evidence before comparing its content-addressed digest.
  return new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes);
}
function requireCondition(condition, message) {
  if (!condition) throw new Error(message);
}

export async function verifyPublicPreviewEvidence(descriptor, fetcher = globalThis.fetch) {
  const preview = await validatePreviewDescriptor(descriptor);
  if (preview.status !== "ready") return preview;
  for (const [key, evidence] of Object.entries(preview.evidence)) {
    const content = await publicText(evidence.url, fetcher);
    if (content !== descriptor.evidence[key].content || await sha256Text(content) !== evidence.sha256) {
      throw new Error(`Public ${key} evidence bytes differ from the curated descriptor.`);
    }
  }
  const api = `https://api.github.com/repos/${REPOSITORY}`;
  const readJson = async (url) => JSON.parse(await publicText(url, fetcher, 1024 * 1024));
  const release = await readJson(`${api}/releases/${preview.release.releaseId}`);
  const latest = await readJson(`${api}/releases/latest`);
  requireCondition(release.id === preview.release.releaseId, "Public release ID differs from the descriptor.");
  requireCondition(latest.tag_name === STABLE_BASELINE.tag && latest.id === STABLE_BASELINE.releaseId &&
    latest.target_commitish === STABLE_BASELINE.sourceCommit && latest.draft === false && latest.prerelease === false,
    "Latest stable identity changed; refresh compatibility evidence before promotion.");
  const baselineAssets = Array.isArray(latest.assets) ? latest.assets.filter((asset) => asset.name === "Abigail-windows-x64-setup.exe") : [];
  requireCondition(baselineAssets.length === 1 && baselineAssets[0].state === "uploaded" &&
    baselineAssets[0].size === STABLE_BASELINE.installerSize &&
    baselineAssets[0].digest === `sha256:${STABLE_BASELINE.installerSha256}`, "Latest stable installer changed.");
  async function verifyTag(tag, sourceCommit) {
    let reference = (await readJson(`${api}/git/ref/tags/${tag}`)).object;
    // Resolve annotated tags without trusting external object URLs.
    for (let depth = 0; reference?.type === "tag" && depth < 4; depth++) {
      requireCondition(/^[a-f0-9]{40}$/.test(reference.sha), "Invalid annotated tag identity.");
      reference = (await readJson(`${api}/git/tags/${reference.sha}`)).object;
    }
    requireCondition(reference?.type === "commit" && reference.sha === sourceCommit,
      "Public tag does not resolve to its tested source.");
  }
  await verifyTag(preview.release.tag, preview.release.sourceCommit);
  await verifyTag(STABLE_BASELINE.tag, STABLE_BASELINE.sourceCommit);
  async function verifyRun(execution, requireConclusion) {
    const run = await readJson(`${api}/actions/runs/${execution.workflowRunId}`);
    requireCondition(run.id === execution.workflowRunId && run.repository?.full_name === REPOSITORY &&
      run.head_repository?.full_name === REPOSITORY && run.head_sha === preview.release.sourceCommit &&
      run.path === execution.workflowPath && run.run_attempt === execution.runAttempt && run.status === "completed",
      "Evidence workflow execution does not match its recorded source, attempt or workflow.");
    if (requireConclusion) requireCondition(run.conclusion === "success", "Compatibility workflow did not succeed.");
    return run;
  }
  const publicReport = preview.evidence.publicVerification.report;
  const ci = publicReport.sourceCi;
  const ciRun = await verifyRun(ci, false);
  requireCondition(ciRun.event === "push" && ciRun.head_branch === "beta", "Source CI must be a beta push.");
  const jobs = await readJson(`${api}/actions/runs/${ci.workflowRunId}/attempts/${ci.runAttempt}/jobs?per_page=100`);
  requireCondition(Array.isArray(jobs.jobs) && jobs.total_count === jobs.jobs.length, "Source CI job metadata is incomplete.");
  const gates = jobs.jobs.filter((job) => job.name === "gate");
  requireCondition(gates.length === 1 && gates[0].status === "completed" && gates[0].conclusion === "success",
    "The exact source CI gate did not succeed.");
  await verifyRun(preview.evidence.compatibility.report.execution, true);
  const manifestContent = await publicText(releaseAssetUrl(preview.release.tag, "release-provenance.json"), fetcher);
  const manifest = JSON.parse(manifestContent);
  requireCondition(JSON.stringify(manifest) === JSON.stringify(preview.evidence.publicVerification.report.provenance),
    "Public provenance differs from the recorded verification.");
  validatePublishedRelease({ release, latestRelease: latest, manifest, expectedCommit: preview.release.sourceCommit });
  const manifestAsset = release.assets.find((asset) => asset.name === "release-provenance.json");
  requireCondition(manifestAsset.digest === `sha256:${await sha256Text(manifestContent)}` &&
    manifestAsset.size === Buffer.byteLength(manifestContent, "utf8"), "Public manifest bytes differ from release metadata.");
  const signaturesContent = await publicText(releaseAssetUrl(preview.release.tag, "windows-signature-evidence.json"), fetcher);
  const signaturesAsset = release.assets.find((asset) => asset.name === "windows-signature-evidence.json");
  requireCondition(signaturesAsset?.digest === `sha256:${await sha256Text(signaturesContent)}` &&
    signaturesAsset.size === Buffer.byteLength(signaturesContent, "utf8"), "Public signature evidence differs from release metadata.");
  requireCondition(JSON.stringify(JSON.parse(signaturesContent)) === JSON.stringify(preview.evidence.publicVerification.report.signatures),
    "Public signature evidence differs from the recorded native verification.");
  return preview;
}

export async function main(args) {
  if (args.length !== 2 || args[0] !== "validate") {
    throw new Error("Usage: node scripts/preview_channel.mjs validate docs/preview/current.json");
  }
  const stat = fs.lstatSync(args[1]);
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size > MAX_DESCRIPTOR_BYTES) {
    throw new Error("Descriptor must be a bounded regular file.");
  }
  const descriptor = JSON.parse(fs.readFileSync(args[1], "utf8"));
  const preview = await verifyPublicPreviewEvidence(descriptor);
  console.log(preview.status === "ready"
    ? `Verified curated preview ${preview.release.tag} at ${preview.release.sourceCommit}; remote identity, asset digests and report bytes match. Native Windows checks remain recorded evidence.`
    : "Preview is unavailable; no installer link will be offered.");
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
