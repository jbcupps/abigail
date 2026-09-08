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
hidden prompts and sends values on stdin to `gh secret set`; it asks the operator
to confirm the target repository. Supply the non-secret username, credential ID,
and thumbprint as parameters. The operator must control the existing signing
authenticator. Never reset it or change certificate access merely to make CI work.
Use a dedicated signer account with only the required certificate access where
available. Repository administrators and trusted workflows can use repository secrets.

## Build an artifact without publishing

On a reviewed branch, run **Abigail Installer Release** with
`signed_artifact_only=true` and a numeric `release_version`, for example `0.0.75`.
It calls **Signed Windows artifact** from that same commit, with read-only contents
permission. This creates an Actions artifact containing the installer, coordinator
executable, and signature report; it creates no git tag or public release.

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
./scripts/configure_esigner_credentials.ps1 -Destination Local -Username <account> -CredentialId <credential-id> -Thumbprint <fingerprint> -Version 0.0.75
```

Commit tracked source changes before Prepare. The preparation phase runs npm/cargo
without signing secrets and records source commit, version, executable hashes, and
all offline payload hashes. The signing phase rejects changed inputs. After a
partially completed signing attempt, run Prepare again before retrying.

The SSL.com CodeSignTool 1.3.3 Windows archive is SHA-256 pinned. The mutable vendor
download fails closed if it changes. Review and update the pin deliberately. The
vendor archive contains its own Java runtime. The tool uses production SSL.com
endpoints and its configured RFC 3161 timestamp service. Password/seed arguments
are passed through a restricted temporary Java argument file, removed in `finally`;
vendor logs are never uploaded. No private signing key is downloaded from the HSM.

## Verification and limits

Each first-party PE is checked with Windows Authenticode and
`signtool verify /pa /all /v /tw`. Verification requires the expected publisher
certificate, code-signing EKU, a trusted chain, SHA-256, and an RFC 3161/SHA-256
timestamp. The installer is extracted and all four executable hashes/signatures
are compared with the signed source payload. Every offline runtime/model/license
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
