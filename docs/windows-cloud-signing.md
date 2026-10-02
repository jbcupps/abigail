# SSL.com cloud signing for Windows releases

The automated cloud lane signs the complete Abigail `.exe` installer with the
existing SSL.com account and certificate. It uses CodeSignTool directly on the
Windows runner; installing eSigner CKA or exporting a private key is unnecessary.
The unsigned MVP remains available. Cloud signing requires neither an updater
key nor an MSI build.

## Confirm the account and certificate

In the SSL.com portal, confirm that the intended Code Signing certificate is
issued, unexpired, and enrolled in an active eSigner signing service, and that
the signing account can use its credential. Owning a certificate alone does not
confirm active cloud enrollment. Check the publisher and the current leaf
certificate's SHA-1 thumbprint. See SSL.com's [signing service information](https://www.ssl.com/products/software-integrity/signing-service/)
and [CI/CD setup guide](https://www.ssl.com/how-to/integrating-esigner-with-ci-cd-pipelines-a-complete-setup-and-configuration-guide/).

The workflow requires these five encrypted **repository Actions secrets**:

| Secret | Required value |
| --- | --- |
| `ESIGNER_USERNAME` | SSL.com signing account username |
| `ESIGNER_PASSWORD` | That account's password |
| `ESIGNER_CREDENTIAL_ID` | The enabled signing credential for the intended certificate |
| `ESIGNER_TOTP_SECRET` | The certificate's persistent signing TOTP seed |
| `WINDOWS_CERTIFICATE_THUMBPRINT` | The current leaf certificate's 40 hexadecimal character SHA-1 thumbprint |

The TOTP seed is the persistent secret behind the certificate's signing QR code.
It generates the changing six-digit OTP. A current OTP expires and cannot serve
as the seed. A four-digit portal/signing PIN is a separate authorization value;
this workflow does not accept it. Account login 2FA is also separate: do not
assume its seed is the certificate's signing seed. SSL.com's [automation guide](https://www.ssl.com/how-to/automate-esigner-ev-code-signing/)
explains the certificate seed.

The user must complete portal login and any PIN entry, seed reveal, or enrollment
authorization manually. Enter credentials directly in the encrypted Actions
secret editor. Keep passwords, PINs, seeds, OTPs, and recovery codes out of chat,
Git, screenshots, command text, and logs. Do not reset enrollment merely to
recover a missing value without checking the existing credential first.

Credential ID and certificate thumbprint are different. CodeSignTool's
`get_credential_ids` lists account credentials; `credential_info` reports
certificate identity and expiry; `get_certs` retrieves the certificate chain.
The leaf certificate provides its thumbprint. These metadata operations still
require account authorization; run them through protected transport rather than
pasting credentials into command arguments. See the [official command guide](https://www.ssl.com/guide/esigner-codesigntool-command-guide/).

## Validate once, then enable recurring signing

Use a new version and the intended reviewed source revision. First run the
manual **Abigail Installer Release** workflow with `windows_signing=esigner` and
`publish_release=false`. This explicitly enables cloud signing for that build
without changing repository policy or publishing a release. For the initial
restoration branch, the command contains no secret values:

```powershell
gh workflow run release.yml --ref codex/restore-esigner-cloud `
  -f release_version=0.0.76 -f windows_signing=esigner -f publish_release=false
```

After integration, use `--ref beta` for an equivalent UAT candidate. The workflow
checks that all five secrets exist before the build. A failure stops the signed
lane; it does not silently publish an unsigned installer.

Review `windows-signatures.json` and the installed-product validation artifact.
Require valid signatures, the expected certificate, timestamps, matching file
hashes, all four installed product executables, and passing installed synthetic
acceptance. A successful tool exit by itself is insufficient. Download and
verify the candidate installer and test its normal Windows launch before
publishing. The synthetic contract exercises the product without requiring a
real model account; it does not validate real model inference.

After the first verified signed build, configure these **repository variables**:

| Variable | Value |
| --- | --- |
| `ABIGAIL_WINDOWS_SIGNING_MODE` | `esigner` |
| `ABIGAIL_REQUIRE_WINDOWS_SIGNING` | `true`, enabled last |

Future `beta` pushes then use cloud signing automatically and produce tagged UAT
prereleases. Promote validated beta source to `main`; publish its stable release
through the existing main/tag release path. Manual publication uses
`publish_release=true` and requires the matching branch's same-commit CI gate.
Candidate builds on feature branches remain unpublished. Use a fresh release
tag rather than replacing an existing unsigned release's assets.

## Pinned tool and signing order

`scripts/install_esigner.ps1` obtains the current Windows archive from the
[official SSL.com download endpoint](https://ssl.com/download/codesigntool-for-windows/),
linked by SSL.com's [downloads page](https://www.ssl.com/downloads/). The reviewed
CodeSignTool 1.3.3 archive SHA-256 is:

```text
317D429BE3AA12A5F2C1FFDD575EAB0CB0CE5E2408AB0056BCDCAAB29875F73D
```

The archive contains `jar/code_sign_tool-1.3.3.jar` and its bundled Java runtime.
The JAR's Maven metadata confirms 1.3.3; its `--version` text still reports
1.3.0. Use archive verification and JAR metadata to identify this reviewed
distribution. A changed download must be reviewed and pinned before use.

The official [GitHub action v1.3.2](https://github.com/SSLcom/esigner-codesign/tree/v1.3.2)
bundles CodeSignTool 1.3.0 and constructs a logged command. Abigail instead uses
its pinned direct wrapper: a short-lived, access-restricted argument file keeps
credentials off process arguments, vendor logging is disabled, and raw vendor
diagnostics are withheld. No-auth checks verified argument-file parsing,
including Windows paths and special characters; they did not sign a file or
prove the account is authorized.

The build signs and verifies Entity Runtime, `hive-daemon`, and `entity-daemon`
before embedding them. Tauri then invokes the signing hook for the final Hive
launcher, uninstaller, and NSIS installer. Tauri patches its launcher bundle
marker before signing; signing the original launcher beforehand would allow
that patch to invalidate its signature. Never change signed executable bytes.
Final checks verify the installed payload and the installer against the expected
certificate and require a timestamp. SSL.com's production timestamp service is
documented in its [SignTool integration guide](https://www.ssl.com/how-to/automate-ev-code-signing-with-signtool-or-certutil-esigner/).

For a local certificate-store or hardware-token workflow, use the separate
[Windows signing guide](WINDOWS_SSL_COM_SIGNING.md).
