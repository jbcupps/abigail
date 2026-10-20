# Abigail Release Runbook

This repo has one family-facing installer release lane and one artifact-only diagnostic build lane. Follow the [Windows release checklist](WINDOWS_RELEASE_CHECKLIST.md) to review a candidate, publish an approved revision, and verify the public download.

Rust is pinned to the tested `1.97.0` compiler in `rust-toolchain.toml` and all
CI/release setup steps. The repository toolchain uses the minimal profile with
`rustfmt` and `clippy`; CodeQL autobuild and local Cargo commands also inherit
the repository pin. This avoids compiler drift between local validation and
hosted builds: floating `stable` advanced to Rust 1.99 and introduced Clippy
errors in `async_trait` generated code. Upgrade the pin only with coordinated
workspace tests, formatting, Clippy, and installer validation.

## Branch Channels

- `beta` is the permanent iteration and UAT branch. Merge implementation PRs there first.
- Every push to `beta` requests the one-step Windows installer workflow; successful signing and release gates permit a GitHub prerelease tagged `vX.Y.Z-beta.N`.
- `main` is the promoted stable branch. Stable releases use clean `vX.Y.Z` tags and are not prereleases.
- The fixed [preview page](https://jbcupps.github.io/abigail/preview/) selects one explicitly verified beta build. A new prerelease never advances that pointer automatically. See the [preview contract and promotion procedure](PREVIEW_CHANNEL.md) for profile isolation, continuing-Entity acceptance, complete backups, and manual opt-in updates.

## Full Installer Release

Workflow: `Abigail Installer Release` (`.github/workflows/release.yml`)

Current active platforms:

- Windows x64 NSIS installer: `Abigail-windows-x64-setup.exe`
- Windows x64 MSI installer: `Abigail-windows-x64.msi` (local signing and unsigned lanes; the cloud lane publishes NSIS only)

The Windows installer is the family-facing release lane. It installs one `Abigail` app icon and bundles the internal split runtime pieces (`Abigail Hive`, `Abigail Entity Runtime`, `hive-daemon`, and `entity-daemon`) so users do not download separate binaries.

Run a beta UAT release from the permanent beta branch:

```bash
git push origin HEAD:beta
```

The workflow tags the build as `v<next-stable-version>-beta.<run-number>` and publishes it as a prerelease.

Installer runs share a signing concurrency group with `queue: max`, which retains
up to 100 pending runs while using the certificate serially. Push CI runs use
separate commit groups so a newer beta push does not cancel the gate needed by
an earlier installer. PR updates still supersede their older CI runs. See
[GitHub Actions concurrency](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-workflow-concurrency).

Linux one-step packaging is planned after the Windows lane is stable. macOS/Apple builds remain paused until the Apple Developer agreement/signing issue is resolved.

Run a specific release:

```bash
gh workflow run release.yml --ref main -f release_version=<unused-version> -f windows_signing=esigner -F publish_release=true
```

Run the next patch release automatically:

```bash
gh workflow run release.yml --ref main
```

Watch the newest run:

```bash
gh run watch --repo jbcupps/abigail
```

Verify the release:

```bash
gh release view v0.0.76 --json tagName,url,publishedAt,assets
```

The workflow builds and tags the triggering commit. Both desktop shells embed their production frontend assets and the requested installer version. It refuses an existing tag that identifies another commit. Every publishing event validates the release ref; pushed tags must match `vX.Y.Z` or `vX.Y.Z-beta.N` exactly.

Before uploading an unsigned candidate artifact, the workflow installs it in a fresh test directory, checks all four payload hashes and Windows daemon loader checks, then runs the synthetic daemon contract against those installed binaries. The contract uses synthetic roots, exercises live streaming and durable history across restarts, and does not require a model account or API key. It does not prove all Windows KnownFolder paths are isolated, GUI/installer coexistence, migration/restore, or real-model inference. The installed verifier refuses to replace an existing Abigail installation outside the repository's test directory. Validation evidence is uploaded as `abigail-installer-validation-windows-latest`.

Publication also requires the `gate` job from the `CI` workflow to pass for the exact release commit on `beta` or `main`. The wait is bounded to one hour. Advisory audit or CodeQL results do not override a successful gate. `beta` builds are prereleases; promoted `main` and clean `vX.Y.Z` tag builds are stable releases. Stable and beta publication require a successfully verified Windows signing lane before creating a tag or release. Unsigned candidate builds (`publish_release=false`) and artifact-only stabilization remain available; updater signing remains opt-in. Selection for the preview page additionally requires the compatibility evidence in `PREVIEW_CHANNEL.md`.

Before upload, the workflow stages the canonical public filenames once and verifies their final Windows signatures, expected publisher certificate, timestamps, and SHA-256 hashes. The single `abigail-release-assets-windows` artifact contains those exact bytes, `windows-signature-evidence.json`, and `release-provenance.json`. The publish job verifies them again on Windows, rejects an existing release, and uploads without renaming or rebuilding files. Stable releases explicitly become latest; beta releases do not.

Publication is complete only after `verify-published-download` passes. It resolves the remote tag to the exact source commit, compares GitHub asset metadata and every downloaded asset with the original build artifact, and verifies the downloaded installer signatures on Windows. Stable verification also downloads the exact `/releases/latest/download/Abigail-windows-x64-setup.exe` route used by the website and checks that its bytes match. The `published-download-verification` Actions artifact preserves the result and metadata. A failure after publication can leave a public release; use the checklist's recovery procedure.

Windows Authenticode validation and Microsoft SmartScreen reputation are separate. A valid publisher signature and timestamp do not guarantee the absence of an "unrecognized app" warning. The public `/releases/latest` download selects stable releases, so publishing a signed beta does not repair an older unsigned stable download. See [Microsoft SmartScreen reputation](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation) and the [stable source proposal](STABLE_SIGNING_PROPOSAL_2026-10-19.md).

For `beta` releases, the workflow also downloads the published Windows installer back from the GitHub prerelease, verifies the expected internal split binaries are present, and uploads a `beta-uat-installer-verification` artifact containing the installer tag/version and inspected payload list.

For current installed MVP acceptance, follow [Windows MVP validation](MVP_WINDOWS.md). Run the synthetic contract against the installed directory, then run the real-model harness with a model connection or an authenticated installed CLI. For example, an existing Codex account needs no API key:

```powershell
pwsh ./scripts/tests/run-mvp-acceptance.ps1 -BinaryDir <installed-directory>
node ./scripts/tests/mvp-real-model.mjs --binary-dir <installed-directory> --cli-provider codex-cli
```

The older `scripts/uat/run-uat.ps1` is a legacy Claude/OpenAI diagnostic and does not cover the current birth and private runtime bootstrap flow. It is not the release acceptance gate.

## Repository Switches

For existing SSL.com certificates, local signed builds, runner setup, and signature
verification, follow [Windows SSL.com signing](WINDOWS_SSL_COM_SIGNING.md).
The [Codex desktop handoff](CODEX_SSL_COM_HANDOFF.md) covers the account/provider steps.

The repeatable artifact-only stabilization path keeps signing and updater artifacts opt-in. Public stable and beta releases require Windows signing; the publication gate rejects unsigned builds.

- `ABIGAIL_REQUIRE_WINDOWS_SIGNING=true` enables Windows signing checks and signing config.
- `ABIGAIL_WINDOWS_SIGNING_MODE=store` expects a certificate available on the Windows runner.
- `ABIGAIL_WINDOWS_RUNNER` can route Windows builds to a self-hosted runner label.
- `ABIGAIL_REQUIRE_UPDATER_SIGNING=true` enables Tauri updater artifacts and `latest.json`.

Current expected repeat-build posture:

- Leave `ABIGAIL_REQUIRE_WINDOWS_SIGNING` unset for unsigned candidates/stabilization only. Stable and beta publication require the supported SSL.com cloud or local signing lane; see the [cloud signing guide](windows-cloud-signing.md) for `windows_signing=esigner` and repository policy.
- Leave `ABIGAIL_REQUIRE_UPDATER_SIGNING` unset until the updater signing lane is intentionally restored.
- Keep Apple/macOS out of the build matrix until the Apple Developer account agreement/signing problem is fixed.
- Keep Linux out of the full installer matrix until the one-step Linux package is intentionally added.

## Unsigned Stabilization Build

Workflow: `Stabilization Build (Unsigned)` (`.github/workflows/release-fast.yml`)

Use it for quick Windows/Linux binary checks without installers, updater artifacts, signing, or Apple notarization:

```bash
gh workflow run release-fast.yml --ref beta
```

Download the `abigail-stabilization-windows-latest` and
`abigail-stabilization-ubuntu-22.04` artifacts from the completed Actions run.
The workflow uses the triggering commit and checked-in dependency lock. It has
read-only repository access and cannot create tags or GitHub releases.

This lane builds diagnostic binaries; it does not embed the current desktop frontends or preserve the internal executable names used by the installed app. Use the full installer workflow above for beta UAT and stable releases.

Each platform artifact contains four side-by-side binaries:

- Abigail Hive app
- Abigail Entity Runtime app
- hive-daemon
- entity-daemon

This lane is for diagnostics and portable validation. Family-facing releases should use the full Windows installer lane so users install and launch one `Abigail` app.
