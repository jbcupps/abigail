# Release process

## Release posture

- `release-fast.yml` is for validation builds and optional signed Windows pre-releases.
- `release.yml` is for official published releases.
- Official releases are expected to ship signed updater metadata, Windows code signing, and macOS Developer ID signing/notarization.

## Required release secrets

Official published releases require all of the following:

- `TAURI_SIGNING_PRIVATE_KEY`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
- `TAURI_UPDATER_PUBKEY`
- `WINDOWS_SIGNING_CERT_BASE64`
- `WINDOWS_SIGNING_CERT_PASSWORD`
- `WINDOWS_CERTIFICATE_THUMBPRINT`
- `WINDOWS_TIMESTAMP_URL`
- `APPLE_CERTIFICATE`
- `APPLE_CERTIFICATE_PASSWORD`
- `APPLE_SIGNING_IDENTITY`
- `APPLE_ID`
- `APPLE_PASSWORD`
- `APPLE_TEAM_ID`

`scripts/enforce_release_prereqs.sh` blocks the build if required inputs are missing.

## Build-time release preparation

- `scripts/prepare_tauri_bundle_config.mjs` injects the updater verification public key and signing fields into `tauri-app/tauri.conf.json`.
- `scripts/validate_tauri_signing_key.sh` normalizes and validates the updater signing secret key before bundling.
- `scripts/generate_tauri_latest_manifest.mjs` produces `latest.json` from the signed updater artifacts that are actually attached to the release.

## Published release assets

Official releases publish:

- Desktop installers (`.exe`, `.msi`, `.dmg`, `.deb`)
- Signed updater payloads (`.nsis.zip`, `.msi.zip`, `.app.tar.gz`) when available
- Matching `.sig` files for updater payloads
- `latest.json`

## Workflow behavior

### Release (`.github/workflows/release.yml`)

- Requires the full signing set above.
- Hard-fails on partial matrix failure instead of publishing a partial official release.
- Uploads installers plus updater artifacts.
- Generates `latest.json` from stable asset names before creating the GitHub Release.
- Publishes npm after the GitHub Release is created.

### Release Fast (`.github/workflows/release-fast.yml`)

- Validation builds can still run without published release output.
- If `create_github_release=true`, the Windows fast-release path requires updater signing and Windows code signing inputs.
- Fast pre-releases generate Windows updater metadata only.

## Versioning

- Stable tags use `vX.Y.Z`.
- Fast validation tags use `vX.Y.Z-fast.<run_number>`.

## Minimal release checklist

1. Confirm the repo is on the intended commit.
2. Ensure required signing secrets are configured in GitHub Actions.
3. Run the appropriate workflow.
4. Confirm installers, updater artifacts, `.sig` files, and `latest.json` are attached to the release.
5. Confirm the release notes and security docs reflect the shipped behavior.
