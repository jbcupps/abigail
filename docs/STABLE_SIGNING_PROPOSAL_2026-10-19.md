# Proposed signed stable 0.0.76

Status: development-only packaging backport; no signing, merge, tag, release,
or website change authorized or performed by this proposal.

## Exact source selection

- Released stable `v0.0.75`: `4f47c16352f26201efc7ca0e2c1d4e7faf4ab540`
  (PR #290). Its public EXE is unsigned.
- Proposed base: current main `3bd68c686e88fadcddcf4780180589d6cad26279`
  (PR #292). The development branch is `dev/stable-signing-backport`.
- Main already includes SSL.com signing support and the Entity startup fix
  `4df319ac357a4c453652b72829a65e01c643578b`, plus its cold-start test follow-up
  `3e032d09f2efed759ecb4a42f44446920bee9d0c`. Compared with released stable,
  the only application source change is `crates/hive-daemon/src/supervisor.rs`:
  Entity daemons select their bound port, readiness checks validate identity,
  and startup exit/timeout/cancellation cleanup is improved. These existing
  main changes are included in the proposal; this is not identical product
  behavior to `v0.0.75`.
- The previous signed candidate `92813e36270b967cf83cff5b1e9b4776b30773fa`
  descends from beta `ea92effd31f554f74bb5e109deaa42876ec4f06f` and includes
  Entity Genesis `30282bb86e595f5fbe1a07f467bf69c9ac8306a9`. Its product
  features and acceptance evidence are not imported into this branch.

## Packaging-only delta from current main

Selectively port the stable-signing gate from
`f016fd815408c0cad38f6cdb61ae83fd641b93ee` and final-asset/provenance/public-
download checks from `92813e36270b967cf83cff5b1e9b4776b30773fa`.
Include the workflow-only queue, exact-ref, CI gate preservation and unsigned
diagnostic publication restrictions needed by those checks. Do not wholesale
cherry-pick mixed application/release commit `21f6d687dacccec02b8e864dd960d97d322f6abd`.

The delta changes only `.github/workflows`, release helper/tests under `scripts`,
and release documentation. Application trees, dependency manifests/lockfiles,
Tauri configuration, Rust toolchain, cloud signer and installed acceptance
scripts remain byte-for-byte at main. It excludes Genesis, later beta runtime
changes, installer lock behavior and the separate UI work.

It also leaves main's existing Visual C++ runtime packaging behavior unchanged;
the later static-CRT/import-check change `b7f88dc48a2b3f93af63285b54f8ca3cec16e122`
is not included. Hosted installer tests cannot prove launch on a clean PC with
no Visual C++ runtime. This proposal resolves release signing/provenance only.

## Proposed version and approval scope

Use version **0.0.76**, new stable tag **v0.0.76**, and cloud-signed NSIS asset
**Abigail-windows-x64-setup.exe**. The existing cloud lane does not produce MSI.
Leave `v0.0.75`, beta and existing release assets intact. No tag exists for
`v0.0.76` at proposal preparation; recheck before any approved execution.

After approval, record the full reviewed branch-head SHA and:

1. Run `release.yml` on this exact development revision with
   `release_version=0.0.76`, `windows_signing=esigner`, `publish_release=false`.
   Verify the actual run head SHA, all twelve installed synthetic checks,
   final signatures/timestamps/hashes, and the downloaded candidate artifact.
2. If those checks pass and main has not moved from the proposed base,
   fast-forward main to that exact reviewed commit. If it moved, stop for a
   fresh source comparison; do not force or silently broaden the scope.
3. Require a passing `CI / gate` for that exact main commit, then dispatch
   `release.yml` on main with the same version/signing settings and
   `publish_release=true`. The workflow builds/signs again with stable
   provenance; candidate provenance must never be relabeled for publication.
4. Require the published-download check to pass, including the existing
   `/releases/latest/download/Abigail-windows-x64-setup.exe` website route.
   No download-page edit is needed. Announce completion only after the public
   bytes and native signatures are verified.

Both candidate and publishing builds use the existing SSL.com configuration.
This requires authorization for signing and stable promotion/publication,
but no new credentials, enrollment, certificate purchase or trust changes.
Any failed check stops promotion. A valid signature does not promise that
Microsoft SmartScreen has already assigned reputation to the new binary.

## Validation boundary

Run the credential-free policy, provenance, native-verifier, staging and mocked
public-download tests on this branch, plus existing signing/state tests and
workflow/PowerShell syntax checks. Record exact results with the proposed SHA.
No application build or new installer has been validated by this proposal.
The earlier beta-based candidate run cannot substitute for the new candidate
because source, installed product and provenance differ.
