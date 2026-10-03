# SSL.com Windows signing for Abigail

For GitHub Actions using the existing eSigner cloud certificate, follow the
[automated cloud signing guide](windows-cloud-signing.md). The certificate-store
instructions below apply to local hardware-token/CKA builds.

The supported local signing entry point is `scripts/build-signed-installer.ps1`.
It builds `hive-app` plus Entity Runtime and both daemons, signs the three internal
resources before packaging, uses Tauri's signing hook for Hive and the installers,
and verifies Authenticode, the expected certificate, a timestamp, and file hashes.
Verification failure stops the release. `build-release-windows.ps1` still targets
the retired `tauri-app`; do not use it for the family-facing product.

Signing is opt-in. `build-split-installer.ps1`, `release-fast.yml`, and checked-in
app configs remain suitable for unsigned development. Updater `.sig` files use a
different key and do not fix Windows publisher warnings.

## Current handoff

On September 8, 2026, the inspected Windows account exposed only the self-signed
`CN=Abigail Dev Local` code-signing certificate. The SSL.com certificate was not
visible in CurrentUser/My or LocalMachine/My. eSigner CKA was not found at the
standard Program Files locations. Windows SDK x64 SignTool was present at:

`C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\signtool.exe`

This does not establish the status of the existing SSL.com order. The desktop
agent must inspect that account, connect the real key provider, and perform the
first actual signing operation. No SSL.com login, production signing, GitHub
settings change, or release publication was performed during repository setup.

## Connect the existing certificate

Use the existing SSL.com order and confirm it is an issued, unexpired **Code
Signing** certificate with the intended publisher and an enabled signing
credential. A TLS certificate for a website is unsuitable. Inspect the order to
determine where its private key lives; a downloaded public certificate alone
cannot sign an executable.

- **eSigner cloud key:** confirm enrollment, then install/configure SSL.com
  eSigner CKA in **Production** under the Windows account doing the build.
  Complete login and the required signing authorization in the provider UI.
  Manual mode is suitable for the first supervised local build. For an unattended
  runner, configure automated signing under its account, with the provider's
  master key and credentials outside the checkout. See [SSL.com's CKA setup
  guide](https://www.ssl.com/how-to/how-to-install-ssl-com-esigner-cloud-key-adapter-cka/).
- **Hardware token:** connect the existing token and configure its supported
  middleware and PIN access under the build account. Use the same certificate
  store signing path. See [SSL.com's certificate usage
  guide](https://www.ssl.com/how-to/using-your-code-signing-certificate/).

If SSL.com's malware blocker refuses a file, inspect the reported finding and
resolve it before retrying. Do not turn off the blocker to force signing. See
[SSL.com's SignTool integration guide](https://www.ssl.com/how-to/automate-ev-code-signing-with-signtool-or-certutil-esigner/).

List public certificate metadata in full Windows PowerShell or PowerShell 7:

```powershell
Get-ChildItem Cert:\CurrentUser\My,Cert:\LocalMachine\My -CodeSigningCert |
  Select-Object Subject, Issuer, Thumbprint, NotBefore, NotAfter, HasPrivateKey, PSParentPath
```

Select the SSL.com-issued publisher certificate, not `Abigail Dev Local`.
`HasPrivateKey=True` confirms a key association; only successful signing proves
that the provider is accessible and authorized. Do not export the private key or
put account passwords, PINs, OTP seeds, recovery codes, or master keys in Git,
chat, screenshots, or build logs. The certificate thumbprint is public metadata
and differs from the eSigner credential ID.

## Build and verify locally

Use the intended beta source revision and a clean, reviewed working tree. Keep
any other desktop agent's edits intact. Supply the intended release version;
the example below is illustrative, not a reserved release number.

```powershell
$env:WINDOWS_CERTIFICATE_THUMBPRINT = 'REPLACE_WITH_THE_REAL_40_HEX_CHARACTER_THUMBPRINT'
./scripts/build-signed-installer.ps1 -Version 0.0.76 -DryRun
./scripts/build-signed-installer.ps1 -Version 0.0.76
```

The script requires Rust, Node/npm, Tauri CLI v2 (`cargo tauri --version`), and
Windows SDK Signing Tools. Tauri needs its normal Windows build/NSIS/WiX tooling.
The default RFC 3161 timestamp endpoint is `http://ts.ssl.com`, with SHA-256 for
both file and timestamp digests, as documented by SSL.com. Use `-TimestampUrl`
to supply an alternative supported endpoint if necessary.

The build uses a temporary Tauri config overlay and leaves checked-in configs
unchanged. Outputs are isolated under
`release-assets/windows/<version>/<run-id>/`:

- `Abigail-windows-x64-setup.exe`
- `Abigail-windows-x64.msi`
- `windows-signatures.json` with final installer SHA-256 hashes and publishers
- `build-signatures.json` with the Hive/runtime/daemon and installer evidence

`-NoNotice` suppresses the script's introductory dialog; it does not bypass
provider authorization. It is appropriate only when CKA/token authorization is
already configured. The local script does not publish or create updater files.

After installation in a test location, verify the actual installed Hive,
Entity Runtime, daemons, and uninstaller where present, using their discovered
paths. Verify the downloaded installer again after publication:

```powershell
./scripts/verify_windows_signatures.ps1 `
  -Files @('C:\path\to\Abigail-windows-x64-setup.exe') `
  -ReportPath artifacts/downloaded-windows-signatures.json
```

Every expected file must exist, verify successfully, display the intended
publisher, and have a timestamp. Match the downloaded installer's SHA-256 against
`windows-signatures.json`. Test one browser download on a clean Windows machine
so Windows evaluates its normal internet download trust signals.

## Enable recurring signed beta releases

The existing `.github/workflows/release.yml` now runs certificate preflight,
signs internal resources before bundling, uses the verified Tauri signing hook,
and verifies all four executables plus NSIS/MSI before upload. It uploads and
publishes `windows-signatures.json` when signing is enabled.

After a successful local signed build, configure the intended signing runner and
these repository settings. Do not enable signing on a runner that cannot access
the key, since this deliberately makes the release fail instead of going unsigned.

| Kind | Name | Value |
| --- | --- | --- |
| Variable | `ABIGAIL_WINDOWS_RUNNER` | Unique label of the Windows runner with the configured provider |
| Variable | `ABIGAIL_WINDOWS_SIGNING_MODE` | `store` |
| Variable | `ABIGAIL_WINDOWS_TIMESTAMP_TSP` | `true` |
| Secret | `WINDOWS_CERTIFICATE_THUMBPRINT` | Real certificate's 40-character thumbprint |
| Secret | `WINDOWS_TIMESTAMP_URL` | `http://ts.ssl.com` |
| Variable | `ABIGAIL_REQUIRE_WINDOWS_SIGNING` | `true`, enabled last |

SSL.com account credentials stay in the configured key provider; these scripts
do not need GitHub username/password/TOTP secrets. `WINDOWS_SIGNING_CERT_PEM` is
optional public material and never substitutes for the key provider. The legacy
PFX mode remains supported for genuinely exportable certificates; do not try to
export or replace an existing SSL.com cloud/token key to use that mode.

Use a dedicated trusted release runner; its Windows login/session must have
provider access. A runner running as a service under a different account will
not see the desktop user's CurrentUser certificate. Configure the correct account
and unattended signing before dispatching a job. Pull-request CI uses mocked
signature tests and does not require this runner or its signing key.

Integrate the repository changes through `beta`; run the normal beta installer
workflow and validate its `vX.Y.Z-beta.N` prerelease before promoting to `main`.
Do not replace an already published installer under an existing release tag.

## Interpret the Windows notice correctly

A valid publicly trusted Authenticode signature establishes the publisher and
file integrity. It does not guarantee an immediate SmartScreen reputation pass.
Microsoft states that new, valid OV/EV-signed downloads can still be unrecognized,
and EV certificates no longer bypass SmartScreen automatically. Record whether
the observed notice is Unknown publisher, SmartScreen, Smart App Control, or a
specific malware detection; each points to a different remaining problem. See
[Microsoft's SmartScreen guidance](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation).

If verification fails, fix the signature, chain, timestamp, publisher, or changed
bytes first. If all signatures pass and only SmartScreen reputation remains,
report that outcome accurately; do not disable Windows protection or promise the
warning has disappeared. Keep a consistent signing identity and repeatable
distribution. See [Microsoft SignTool
reference](https://learn.microsoft.com/en-us/windows/win32/seccrypto/signtool).

## Repository checks

```powershell
./scripts/test_windows_signing.ps1
node scripts/check_unsigned_stabilization.mjs
```

The regression tests use simulated certificates and signatures. They test invalid
identity, missing keys/EKU, expiry, native verification failures/warnings, missing
timestamps, changed files, wrong publisher, and certificate-store selection.
They do not constitute a successful production signing test.
