# Pinned source CI execution scope

Read-only source review, 2026-10-20. This documents the separate manual run;
it is not dispatch approval or an executed result.

- Product: `f7c7848d25726480b253ec1ca637c3ffa5ea3c77`.
- Harness: `c79e21442143829f4611e48d9ec193089b759c67`.
- Branch: `dev/preview-source-ci-0.0.76`.
- Workflow: `.github/workflows/ci.yml`, GitHub workflow ID `231696569`.
- Workflow blob SHA-256:
  `72d101c7913da15d1fb7d1af610a85b3ababbb23427215426b5e1b740ba75cd9`.

The six checkout jobs verify the fixed product HEAD independently of the
workflow revision. Their caller-input comparison establishes revision
consistency, not review approval. The [review assessment](PREVIEW_SOURCE_CI_REVIEW.md)
records the separate immutable binding and online verification required to use
a run as evidence. This is ordinary source regression with the normal build
profile. It is not acceptance of the signed preview installer or native GUI.

## Dispatch boundary

The current instruction permits this separate dispatch only if it creates no
additional credentials and requires no new signing. The reviewed full workflow
does create disposable local cryptographic material, so dispatch is **pending
specific approval**. `ABIGAIL_CI_MODE` is not a mock credential backend. Do not
skip tests, change the product or weaken the evidence binding to avoid this
condition.

The narrow approval scope is one full pinned CI run on disposable GitHub-hosted
runners, permitting its existing synthetic local encryption/signing keys,
native Windows DPAPI fixtures and Linux runner Secret Service lookup, with
the existing cleanup plus hosted runner disposal. It involves no SSL.com
signing, purchases, provider account/credential setup, local installation or
access to Jim's real profiles or devices.

After approval, recheck the remote harness SHA, immutable workflow hash and
existing manual runs before issuing this exact command once:

```powershell
gh workflow run ci.yml --repo jbcupps/abigail --ref dev/preview-source-ci-0.0.76 -f reviewed_harness_commit=c79e21442143829f4611e48d9ec193089b759c67
```

The registration PR preserves normal push/PR/schedule CI and does not add these
tests to those events. Registration/normal CI and this additional manual run
are separate actions.

## Material and credential paths

| Existing check | Actual effect |
| --- | --- |
| Windows stability daemon integrations | Fresh random 32-byte root KEKs, encrypted `master.key`, `vault.sentinel`, `vault.kek.dpapi`, synthetic secret vaults, and Ed25519 birth keys/signatures in disposable test profiles. Startup creates two synthetic Entities. |
| Windows workspace unit tests | Real DPAPI protection/unprotection of fixed test KEKs and synthetic legacy secrets; random test identities and Argon2 salt/key material. |
| Linux identity unit fixtures | Attempt a real Secret Service lookup before their fixed raw test-key fallback. No deterministic Secret Service write was found in this bootstrap. |
| Signing-policy fixtures | Mock signer/certificate objects or verification/copying of Node's existing public signature. No new certificate-store credential or SSL.com signing operation. |

At this product's dependency features, Windows/macOS keyring-rs uses the mock
backend; Windows DPAPI remains real. Linux enables Secret Service. Its stable
service/account is `com.abigail.vault` / `abigail.hive.master-kek-v1`, not a
unique fixture namespace. Isolation therefore relies on the disposable hosted
runner, not a claim that every test uses the preview namespace.

Key source references at the immutable product:

- [Actual credential backend and first-run KEK creation](https://github.com/jbcupps/abigail/blob/f7c7848d25726480b253ec1ca637c3ffa5ea3c77/crates/abigail-core/src/vault/unlock.rs#L417).
- [Native Windows DPAPI call](https://github.com/jbcupps/abigail/blob/f7c7848d25726480b253ec1ca637c3ffa5ea3c77/crates/abigail-core/src/dpapi.rs#L25).
- [Fixed-key DPAPI unit test](https://github.com/jbcupps/abigail/blob/f7c7848d25726480b253ec1ca637c3ffa5ea3c77/crates/abigail-core/src/vault/unlock.rs#L1371).
- [Synthetic legacy-secret DPAPI fixtures](https://github.com/jbcupps/abigail/blob/f7c7848d25726480b253ec1ca637c3ffa5ea3c77/crates/abigail-core/src/secrets.rs#L333).
- [Master signing-key creation](https://github.com/jbcupps/abigail/blob/f7c7848d25726480b253ec1ca637c3ffa5ea3c77/crates/abigail-core/src/keyring.rs#L188) and [private birth-key persistence](https://github.com/jbcupps/abigail/blob/f7c7848d25726480b253ec1ca637c3ffa5ea3c77/crates/abigail-identity/src/lib.rs#L745).

## Cleanup, caches and artifacts

The daemon harness owns a `tempfile::TempDir`; it kills/reaps its Hive and
data-root-matched Entity children in `Drop`, followed by best-effort temporary
directory cleanup. The identity unit suite retains
`%TEMP%/abigail_identity_test_vault_<UUID>` until runner disposal. Startup
explicitly retains `target/manual-test/supervisor-startup-<UUID>` diagnostics.
Assertion/cleanup failures can leave other temporary fixtures. Do not claim
every individual key file is explicitly deleted.

Stability has no cache or artifact-upload step. Matrix tests do not enable
daemon integration, so startup returns before creating its target-directory
fixture. Reviewed ordinary private-key fixtures use OS temp outside the pinned
Rust cache action's Cargo/target paths. The workflow adds no cache paths and
has no artifact-upload step; Codecov names only conditional `lcov.info`.
No newly generated private fixture was traced into these cache/upload inputs.
This is source-derived scope, not proof that a future cleanup succeeds.
Public fixed test constants can remain compiled into cached test binaries.

See the [pinned cache implementation](https://github.com/Swatinem/rust-cache/blob/c19371144df3bb44fab255c43d04cbc2ab54d1c4/src/config.ts#L245),
[daemon harness cleanup](https://github.com/jbcupps/abigail/blob/f7c7848d25726480b253ec1ca637c3ffa5ea3c77/crates/daemon-test-harness/src/lib.rs#L62)
and [retained startup fixture](https://github.com/jbcupps/abigail/blob/f7c7848d25726480b253ec1ca637c3ffa5ea3c77/crates/hive-daemon/tests/startup.rs#L89).

The existing advisory audit remains separate and unresolved. This scope review
does not authorize blocked vulnerability remediation, native GUI testing or
publication.
