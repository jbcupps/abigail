# Windows signing with SSL.com eSigner

The signed lane builds the current split Abigail product, including the offline
Ollama/Qwen bundle. It signs the coordinator, Entity Runtime, both daemons, the
NSIS uninstaller, and the NSIS installer. It preserves third-party resource bytes.
The unsigned stabilization workflow remains available without signing credentials.

## Credentials

Use the issued **code-signing** certificate enrolled in production eSigner.
An SSL/TLS certificate, public PEM alone, or a development certificate cannot sign
a trusted Windows release. Select the cloud credential and its matching certificate
thumbprint from the SSL.com order; a hardware-token certificate can have a different
thumbprint even when the publisher name matches.

Configure encrypted repository Actions secrets:

| Secret | Value |
| --- | --- |
| `ESIGNER_USERNAME` | SSL.com signing account |
| `ESIGNER_PASSWORD` | Existing account password |
| `ESIGNER_CREDENTIAL_ID` | Enabled production eSigner credential |
| `ESIGNER_TOTP_SECRET` | Existing signing authenticator seed, not a six-digit OTP |
| `WINDOWS_CERTIFICATE_THUMBPRINT` | Full SHA-1 fingerprint of that certificate |

Do not put secret values into chat, commands, committed files, workflow variables,
or logs. `scripts/configure_esigner_credentials.ps1 -Destination GitHub` provides
hidden prompts and sends values on stdin to `gh secret set`. It identifies the
target repository before requesting private inputs. Supply the non-secret username,
credential ID and thumbprint as parameters, or in an ignored
`.cache/signing/account.json` containing `username`, `credentialId` and `thumbprint`.
The operator must control the existing signing
authenticator. Never reset it or change certificate access merely to make CI work.
Use a dedicated signer account with only the required certificate access where
available. Repository administrators and trusted workflows can use repository secrets.

The existing eSigner **secret code** can be revealed by the account owner on the
certificate order's details page using the existing four-digit enrollment PIN and
**Show QR Code**. It is neither that PIN nor the changing six-digit authenticator
code. Reveal/copy it privately; do not share a screenshot or paste it into chat.
If the existing PIN/seed is unavailable, use supervised local signing below.
Do not reset an authenticator to make automation work.

## Build an artifact without publishing

On a reviewed branch, run **Abigail Installer Release** with
`signed_artifact_only=true` and a numeric `release_version`, for example `0.0.75`.
It calls **SSL.com signed Windows** from that same commit, with read-only contents
permission. This creates an Actions artifact containing the installer, coordinator
executable, and signature report; it creates no git tag or public release.

The configuration check runs before checkout or tool installation and lists every
missing secret in the Actions run summary. Add missing values under repository
**Settings → Secrets and variables → Actions**, then rerun the failed job. Until
this workflow is integrated into the default branch, select the reviewed branch
in **Abigail Installer Release** and enable **signed_artifact_only**.

The coordinator executable requires the bundled resources installed alongside it.
Use `Abigail-windows-x64-setup.exe` for the complete product.

After the artifact is validated and the change is integrated, release signing can
be selected with repository variables `ABIGAIL_WINDOWS_SIGNING_MODE=esigner` and
`ABIGAIL_REQUIRE_WINDOWS_SIGNING=true`. Leave updater signing disabled for this NSIS
lane. Missing credentials, wrong certificates, failed signing, and invalid packaged
signatures fail the job before upload/publish. The normal beta tag/promotion policy
still applies. Selecting signed artifact mode never enables public publishing.

## Local build

Use PowerShell 7, Rust, Node/npm, Windows SDK SignTool, and 7-Zip. Install the pinned
CLI with `npm install -g @tauri-apps/cli@2.11.4`, then:

```powershell
./scripts/install_esigner.ps1
./scripts/build_signed_installer.ps1 -Phase Prepare -Version 0.0.75
./scripts/build_signed_installer.ps1 -Phase Check -Version 0.0.75
./scripts/configure_esigner_credentials.ps1 -Destination Local -Username <account> -CredentialId <credential-id> -Thumbprint <fingerprint> -Version 0.0.75
```

Commit tracked source changes before Prepare. The preparation phase runs npm/cargo
without signing secrets and records source commit, version, executable hashes, and
all offline payload hashes. The signing phase rejects changed inputs. After a
partially completed signing attempt, run Prepare again before retrying.

`Check` performs no signing and needs no signing password/seed. It validates the
exact commit/version, generated bundle config, ordered first-party paths/hashes,
complete offline file set, and verification tooling. Added files and links are
rejected, as are missing or modified files. A relative `CARGO_TARGET_DIR` is
resolved against the repository root and made absolute for all child builds.

For a first local signature using existing six-digit authenticator codes:

```powershell
./scripts/configure_esigner_credentials.ps1 -Destination Local -Authentication Manual -UseDialog
```

This command uses the non-secret account profile described above. Run it in
PowerShell, including via its absolute path when the terminal is in another
checkout. A masked window requests the account password. Each signing operation
then opens **Abigail SSL.com signing code** when CodeSignTool is waiting for a
fresh code. Codes go to the vendor process on private stdin; passwords use the
restricted argument file. Cancellation or rejection stops the build; no automatic
retry, authenticator reset, or CKA installation is needed. Stay available through
NSIS compression for the final installer approval. This supervised mode cannot
run unattended on a GitHub-hosted runner.

To configure GitHub privately and start only its artifact build from the reviewed
branch, use `-Destination GitHub -UseDialog -StartBuild` instead. This requires the
existing secret code and uses the same account profile. Neither path publishes.

The main executable is signed inside Tauri's callback after its NSIS metadata
patch and before bundling. Because Tauri restores its unsigned build output when
bundling finishes, the callback saves a separate signed copy and compares it with
the extracted executable. The other three Abigail programs are signed first.

The SSL.com CodeSignTool 1.3.3 Windows archive is SHA-256 pinned. The mutable vendor
download fails closed if it changes. Review and update the pin deliberately. The
vendor archive contains its own Java runtime. The tool uses production SSL.com
endpoints and its configured RFC 3161 timestamp service. Password/seed arguments
are passed through a restricted temporary Java argument file, removed in `finally`;
vendor file logging is explicitly disabled, and vendor console output is kept
private. No private signing key is downloaded from the HSM.

Successful outputs go to `target/signed-release/artifacts/<run-id>/` so earlier
outputs cannot be mistaken for the current attempt. `latest-success.json` points
to the last successful signature verification. GitHub uploads only the directory
returned by its current signing step, after packaged startup acceptance passes.

## Entrypoint consolidation

Use `build_signed_installer.ps1` for Prepare, Check and Sign. The hyphenated
`build-signed-installer.ps1` is only an explicit-phase forwarding name.
`build-release-windows.ps1` now fails immediately because its retired `tauri-app`
build omitted the current offline product. Signed release mode must be `esigner`;
the unverified legacy store/PFX release path is rejected before building.

The earlier uncommitted CKA implementation in another checkout is preserved.
Its certificate normalization and failure-test ideas were reviewed and adapted;
its builder, resource list, and verifier were not copied because they lack the
offline payload, Tauri signed-main capture, and actual installer extraction checks.
An existing CKA/store installation may support SignTool independently, but it is
not a substitute for the canonical verified packaging path.

## Verification and limits

Each first-party PE is checked with Windows Authenticode and
`signtool verify /pa /all /v /tw`. Verification requires the expected publisher
certificate, code-signing EKU, a trusted chain, SHA-256, and an RFC 3161/SHA-256
timestamp. The installer is extracted and all four executable hashes/signatures
are compared with the signed source payload; the extracted NSIS uninstaller is
also verified. Every offline runtime/model/license
file must match its pre-signing hash. The JSON report includes source commit,
version, hashes, signer, timestamp signer, and verbose SignTool evidence.

An Actions artifact or successful extraction is not clean-machine UAT. Test the
installer and first-run local reply on a disconnected Windows VM before promotion.
Authenticode verification does not promise that Windows SmartScreen will display
no reputation warning. The local inference acceptance probe uses dead proxies;
it does not disable the operating system's networking.

References: [SSL.com CodeSignTool](https://www.ssl.com/guide/esigner-codesigntool-command-guide/),
[SSL.com CI/CD setup](https://www.ssl.com/how-to/integrating-esigner-with-ci-cd-pipelines-a-complete-setup-and-configuration-guide/),
[Tauri Windows signing](https://v2.tauri.app/distribute/sign/windows/),
[Microsoft SignTool](https://learn.microsoft.com/en-us/windows/win32/seccrypto/signtool).
