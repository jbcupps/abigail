// Runs only parsed workflow policy checks and the extracted source guard with
// mocked git output. No product, build, network, signing or credential actions.
// Requires Python + PyYAML and Bash (Git Bash on Windows).
// Run: node --test scripts/tests/test_preview_source_ci.mjs
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import test from "node:test";

const source = readFileSync(
  fileURLToPath(new URL("../../.github/workflows/ci.yml", import.meta.url)),
  "utf8",
);
const python = process.env.PREVIEW_CI_TEST_PYTHON ||
  (process.platform === "win32" ? "python" : "python3");
const parsed = spawnSync(python, ["-c", [
  "import json,sys,yaml",
  // BaseLoader preserves the Actions key `on` and scalar strings; YAML 1.1's
  // implicit Boolean resolver otherwise turns `on` into True.
  "json.dump(yaml.load(sys.stdin.read(), Loader=yaml.BaseLoader), sys.stdout)",
].join("; ")], { input: source, encoding: "utf8" });
assert.equal(parsed.status, 0, `Parse workflow with PyYAML: ${parsed.stderr}`);
const workflow = JSON.parse(parsed.stdout);
const bash = process.env.PREVIEW_CI_TEST_BASH ||
  (process.platform === "win32"
    ? join(process.env.ProgramFiles || "C:\\Program Files", "Git", "bin", "bash.exe")
    : "bash");
const product = "f7c7848d25726480b253ec1ca637c3ffa5ea3c77";
const checkoutJobs = ["stability", "lint", "test", "frontend", "audit", "codeql"];
const guardName = "Verify exact checked-out source and reviewed manual scope";
const sourceExpression =
  "${{ github.event_name == 'workflow_dispatch' && '" + product + "' || github.sha }}";

function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map(key => [key, canonical(value[key])]));
  }
  return value;
}

function assertPolicy(candidate) {
  assert.deepEqual(candidate.on.workflow_dispatch.inputs, {
    reviewed_harness_commit: {
      description: "Reviewed dev workflow commit; manual CI checks fixed preview product f7c7848 without signing",
      required: "true",
      type: "string",
    },
  });
  assert.equal(candidate.env.ABIGAIL_SOURCE_COMMIT, sourceExpression);
  assert.equal(candidate.env.REVIEWED_HARNESS_COMMIT, "${{ inputs.reviewed_harness_commit }}");
  const original = structuredClone(candidate);
  delete original.on.workflow_dispatch;
  delete original.env.ABIGAIL_SOURCE_COMMIT;
  delete original.env.REVIEWED_HARNESS_COMMIT;
  for (const name of checkoutJobs) {
    const steps = candidate.jobs[name].steps;
    assert.match(steps[0].uses, /^actions\/checkout@[a-f0-9]{40}$/);
    assert.deepEqual(steps[0].with, {
      ref: "${{ env.ABIGAIL_SOURCE_COMMIT }}",
      "persist-credentials": "false",
    });
    assert.deepEqual(Object.keys(steps[1]).sort(), ["name", "run", "shell"]);
    assert.equal(steps[1].name, guardName);
    assert.equal(steps[1].shell, "bash");
    assert.equal(steps[1].run, candidate.jobs.stability.steps[1].run);
    assert.ok(steps[1].run.startsWith("set -euo pipefail\n"));
    delete original.jobs[name].steps[0].with;
    original.jobs[name].steps.splice(1, 1);
  }
  // Entire preexisting parsed workflow at 02d5048ff01a296c376da103ae8b6087265f0bc9.
  // This checks default triggers, all old commands/exclusions, permissions,
  // concurrency, required gates, and advisory audit/CodeQL without reproducing
  // a weaker second copy. Updating this digest requires reviewing the CI delta.
  const digest = createHash("sha256")
    .update(JSON.stringify(canonical(original)))
    .digest("hex");
  assert.equal(digest, "df724677d70bdec534c904ae77617419c14f6e068508eb5563c221b59ace4627");
}

test("parsed manual scope preserves all preexisting CI behavior", () => {
  assertPolicy(workflow);
});

const mutations = {
  "manual source made mutable": candidate => {
    candidate.env.ABIGAIL_SOURCE_COMMIT = "${{ github.sha }}";
  },
  "normal source incorrectly fixed to preview": candidate => {
    candidate.env.ABIGAIL_SOURCE_COMMIT = product;
  },
  "manual review input optional": candidate => {
    candidate.on.workflow_dispatch.inputs.reviewed_harness_commit.required = "false";
  },
  "ordinary trigger broadened": candidate => {
    candidate.on.push.branches.push("dev/**");
  },
  "repository write permission": candidate => {
    candidate.permissions.contents = "write";
  },
  "signing secret added": candidate => {
    candidate.env.ESIGNER_PASSWORD = "${{ secrets.ESIGNER_PASSWORD }}";
  },
  "required gate weakened": candidate => {
    candidate.jobs.gate.steps[0].run =
      candidate.jobs.gate.steps[0].run.replace("stability lint test frontend", "lint frontend");
  },
  "test exclusion added": candidate => {
    const step = candidate.jobs.test.steps.find(step => step.name === "Run tests");
    step.run += " --exclude abigail-identity";
  },
  "advisory audit removed": candidate => {
    delete candidate.jobs.audit;
  },
  "new release command": candidate => {
    candidate.jobs.frontend.steps.push({ run: "gh release create unreviewed" });
  },
};
for (const name of checkoutJobs) {
  mutations[`${name} checkout follows branch`] = candidate => {
    candidate.jobs[name].steps[0].with.ref = "main";
  };
  mutations[`${name} checkout persists credentials`] = candidate => {
    candidate.jobs[name].steps[0].with["persist-credentials"] = "true";
  };
  mutations[`${name} source guard omitted`] = candidate => {
    candidate.jobs[name].steps.splice(1, 1);
  };
  mutations[`${name} workload precedes source guard`] = candidate => {
    const steps = candidate.jobs[name].steps;
    [steps[1], steps[2]] = [steps[2], steps[1]];
  };
}
for (const [label, mutate] of Object.entries(mutations)) {
  test(`policy rejects ${label}`, () => {
    const candidate = structuredClone(workflow);
    mutate(candidate);
    assert.notDeepEqual(candidate, workflow);
    assert.throws(() => assertPolicy(candidate));
  });
}

const harness = "a".repeat(40);
const fixture = {
  ABIGAIL_SOURCE_COMMIT: product,
  GITHUB_EVENT_NAME: "workflow_dispatch",
  GITHUB_REPOSITORY: "jbcupps/abigail",
  GITHUB_REF: "refs/heads/dev/reviewed-preview-ci",
  REVIEWED_HARNESS_COMMIT: harness,
  GITHUB_SHA: harness,
  MOCK_CHECKOUT_SHA: product,
};
const fakeGit = [
  "git() {",
  '  [[ "$*" == "rev-parse HEAD" ]] || return 97',
  '  printf "%s\\n" "$MOCK_CHECKOUT_SHA"',
  "}",
].join("\n");
function executeGuard(guard, overrides) {
  return spawnSync(bash, ["--noprofile", "--norc", "-c", `${fakeGit}\n${guard}`], {
    encoding: "utf8",
    env: { ...process.env, ...fixture, ...overrides },
    timeout: 10_000,
  });
}

for (const name of checkoutJobs) {
  const guard = workflow.jobs[name].steps[1].run;
  test(`${name}: actual guard accepts distinct reviewed workflow/product pins`, () => {
    const result = executeGuard(guard, {});
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, new RegExp(`Verified product source: ${product}; workflow revision: ${harness}`));
  });
  const invalid = {
    "wrong checkout": { MOCK_CHECKOUT_SHA: "b".repeat(40) },
    "different repository": { GITHUB_REPOSITORY: "other/abigail" },
    "main dispatch": { GITHUB_REF: "refs/heads/main" },
    "beta dispatch": { GITHUB_REF: "refs/heads/beta" },
    "tag dispatch": { GITHUB_REF: "refs/tags/dev/preview" },
    "empty review pin": { REVIEWED_HARNESS_COMMIT: "" },
    "short review pin": { REVIEWED_HARNESS_COMMIT: "a".repeat(7) },
    "different review pin": { REVIEWED_HARNESS_COMMIT: "b".repeat(40) },
    "nonhex matched pins": { REVIEWED_HARNESS_COMMIT: "z".repeat(40), GITHUB_SHA: "z".repeat(40) },
    "different checked out product": { ABIGAIL_SOURCE_COMMIT: "c".repeat(40), MOCK_CHECKOUT_SHA: "c".repeat(40) },
  };
  for (const [label, overrides] of Object.entries(invalid)) {
    test(`${name}: actual guard refuses ${label}`, () => {
      const result = executeGuard(guard, overrides);
      assert.equal(result.error, undefined, result.error?.message);
      // Shell launch failures are not successful negative guard evidence.
      assert.equal(result.status, 1, result.stderr);
      assert.doesNotMatch(result.stdout, /Verified product source:/);
    });
  }
  for (const event of ["push", "pull_request", "schedule"]) {
    test(`${name}: ${event} retains arbitrary github.sha checkout`, () => {
      const sha = "d".repeat(40);
      const result = executeGuard(guard, {
        GITHUB_EVENT_NAME: event,
        GITHUB_REF: event === "pull_request" ? "refs/pull/123/merge" : "refs/heads/main",
        GITHUB_SHA: sha,
        ABIGAIL_SOURCE_COMMIT: sha,
        MOCK_CHECKOUT_SHA: sha,
        REVIEWED_HARNESS_COMMIT: "",
      });
      assert.equal(result.status, 0, result.stderr);
      assert.match(result.stdout, new RegExp(`Verified product source: ${sha}`));
    });
  }
}
