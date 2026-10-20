// Source-controlled review records, not descriptor-supplied authority. Adding a
// record requires reviewing the immutable workflow and its product checkout.
// A candidate-origin record establishes where bytes came from; it does not
// establish full compatibility or permit use as a source-CI/acceptance gate.
export const REVIEWED_WORKFLOW_BINDINGS = Object.freeze({
  "genesis-source-ci-a18249a": Object.freeze({
    purpose: "source-ci",
    productSourceCommit: "a18249a3b622e8b34d0f20ec1f0e24fc0dc7183d",
    harnessRevision: "0bb0f4fd0cd181250cc62ed472699c5ea21c79a2",
    workflowPath: ".github/workflows/ci.yml",
    workflowSha256: "4114e030cab0eda10995884e746a0bffeed73eba373eeb34cbf02345aa171022",
    event: "workflow_dispatch",
    branch: "dev/genesis-preview-harness",
    productCheckoutPath: ".",
    // Reviewed source relationship only; no hosted run or execution permission.
    // Six guards bind the actual checkout and caller revision consistently.
    // Acceptance additionally verifies the actual run and immutable workflow.
  }),
  "genesis-candidate-a18249a": Object.freeze({
    purpose: "candidate-origin",
    productSourceCommit: "a18249a3b622e8b34d0f20ec1f0e24fc0dc7183d",
    harnessRevision: "0bb0f4fd0cd181250cc62ed472699c5ea21c79a2",
    workflowPath: ".github/workflows/preview-candidate.yml",
    workflowSha256: "d9ccd63dc36ded79b34cced601c9eb2a12b2f9df0249c8cc89b69ff349ee0886",
    event: "workflow_dispatch",
    branch: "dev/genesis-preview-harness",
    productCheckoutPath: "product",
    // Literal immutable product checkout; separate harness checkout.
    // Deliberately no REVIEWED_CANDIDATE_ORIGINS entry until real run/artifact
    // bytes exist and are reviewed. This binding alone cannot accept bytes.
  }),
  "preview-source-ci-c79e214": Object.freeze({
    purpose: "source-ci",
    productSourceCommit: "f7c7848d25726480b253ec1ca637c3ffa5ea3c77",
    harnessRevision: "c79e21442143829f4611e48d9ec193089b759c67",
    workflowPath: ".github/workflows/ci.yml",
    workflowSha256: "72d101c7913da15d1fb7d1af610a85b3ababbb23427215426b5e1b740ba75cd9",
    event: "workflow_dispatch",
    branch: "dev/preview-source-ci-0.0.76",
    productCheckoutPath: ".",
    // Six checkout guards verify the fixed product HEAD and caller-declared
    // dispatch revision consistency. They do not establish harness approval;
    // this reviewed record and online run/workflow verification bind acceptance.
    // Source regression uses the default build profile and establishes no
    // preview or signed-installer acceptance.
  }),
  "preview-candidate-39468905473": Object.freeze({
    purpose: "candidate-origin",
    productSourceCommit: "f7c7848d25726480b253ec1ca637c3ffa5ea3c77",
    harnessRevision: "dfebfb8b589bbf3f8275eff5547e1e7c8d7b91c4",
    workflowPath: ".github/workflows/preview-candidate.yml",
    workflowSha256: "4eb81663febd42fcbd5f37f43f183dff4fdbb4f5945d79194b30e511b66989c2",
    event: "workflow_dispatch",
    productCheckoutPath: "product",
    // The workflow has an explicit literal f7c7848 checkout, verifies its HEAD,
    // builds under product/, and reports separate product and harness commits.
  }),
});

export const REVIEWED_CANDIDATE_ORIGINS = Object.freeze({
  "preview-candidate-39468905473": Object.freeze({
    workflowRunId: 39468905473,
    runAttempt: 1,
    artifactId: 12287683353,
    artifactName: "abigail-preview-candidate-39468905473-1",
    artifactSha256: "8bf9865341e78cb2582ee1d3b261e75e46ad8e452c2bb74d211bd0923b72f6dc",
    provenanceSha256: "46d3f2f4ed7b4fc74c2f53363348b9a5f71e321a2d32bc08428f8bb7b6ead2b0",
    provenanceSize: 1988,
    installerSha256: "a59580764b7cf072cf7c6eeccf040f5c934c5b0c5c164cfe5338e2ccfe5199d1",
    installerSize: 87408680,
  }),
});
