# Windows release signing diagnosis — 2026-10-19

## Result

The public stable Windows installer is unsigned. The newer beta installer has a valid SSL.com Authenticode signature and timestamp. The download page uses GitHub's stable `releases/latest` route, so a signed beta does not change what that page downloads.

The exact warning and file used by the reporter have not been identified. An unknown publisher warning on the stable file is explained by its missing signature. A SmartScreen "unrecognized app" warning on the verified beta can instead reflect reputation. Do not infer the warning type solely from the word "signing".

## Exact released artifacts

Repository: `https://github.com/jbcupps/abigail`. Both files were downloaded from their release asset URLs, hashed, and inspected with Windows PowerShell `Get-AuthenticodeSignature` and Windows SDK SignTool 10.0.26100. No installer was launched, installed, or unblocked during this diagnosis.

| Fact | Latest stable | Latest beta |
| --- | --- | --- |
| Release | [v0.0.75](https://github.com/jbcupps/abigail/releases/tag/v0.0.75) | [v0.0.76-beta.131](https://github.com/jbcupps/abigail/releases/tag/v0.0.76-beta.131) |
| Published UTC | 2026-10-02 20:48:30 | 2026-10-18 00:11:27 |
| Tag commit | `4f47c16352f26201efc7ca0e2c1d4e7faf4ab540` | `ea92effd31f554f74bb5e109deaa42876ec4f06f` |
| Asset | `Abigail-windows-x64-setup.exe` | `Abigail-windows-x64-setup.exe` |
| Asset ID | `606463672` | `651969198` |
| Size, bytes | 65,082,112 | 89,192,984 |
| Authenticode | `NotSigned`, no signer or timestamp | `Valid`, embedded Authenticode |
| SignTool `/pa /all /v /tw` | Exit 1: No signature found | Exit 0: 1 signature verified, 0 warnings, 0 errors |

Whole-file SHA-256 (both matched the GitHub API asset `digest`):

```text
v0.0.75
d02eedba0d110be780ed416834f15b8ad65d0af8d4315fccdcee1afc241c1c8d

v0.0.76-beta.131
403731c0e1d09f0b9ed29340879f3f96731362d497e86016bd8eadb3fc36d61e
```

The beta certificate facts from the downloaded installer:

- Publisher: `James B Cupps`.
- Issuer: `SSL.com Code Signing Intermediate CA RSA R1`.
- Certificate thumbprint: `69BE78FD201A59A29C46F5D37097E0C6DDA04A41`.
- Validity UTC: 2026-03-09 17:13:36 through 2027-06-10 17:13:36.
- Code-signing EKU: `1.3.6.1.5.5.7.3.3`.
- Chain root: `SSL.com Root Certification Authority RSA`.
- Timestamp UTC: 2026-10-17 23:43:11, verified by `SSL.com Timestamping Unit 2025 E1`.
- Timestamp certificate thumbprint: `26BF9B09469C6976748F04A0DDEAA07AE7C81569`.

The published beta `windows-signatures.json` has SHA-256 `184309859310d0ca3d89a1844c202a99dc834bf206b936a48b5bd8ff6e2440b8`, matching GitHub's digest. It identifies the same source commit, installer hash, publisher and certificate, and records SHA-256/RFC 3161 timestamp verification for the installer, application, internal binaries and uninstaller. Its installed-contract result is release-run evidence, not a claim that this diagnosis executed the installer.

SignTool's displayed Authenticode content hash (`857DF16A041E293F90D2651FD2B88FFBFE22063E8709CA63035B8CB55E3FC86B` for the beta) is distinct from the whole-file download SHA-256 above. Use `Get-FileHash` for download identity.

## Release configuration and job evidence

- [Stable release run 37057597530](https://github.com/jbcupps/abigail/actions/runs/37057597530/job/111006026639): signing-tool lookup, certificate preparation, internal signing, signature verification and signature evidence upload were skipped. Unsigned payload verification and publication succeeded.
- [Beta release run 39079182127](https://github.com/jbcupps/abigail/actions/runs/39079182127/job/117532469167): SSL.com tool installation, account/certificate preflight, signing, installed-product verification and signature evidence upload succeeded. The separate local certificate-store verification step was skipped because the cloud signing lane was active.
- [Release workflow at the beta commit](https://github.com/jbcupps/abigail/blob/ea92effd31f554f74bb5e109deaa42876ec4f06f/.github/workflows/release.yml) selects the cloud/store/unsigned lanes and publishes their outputs. The cloud lane uses SSL.com CodeSignTool 1.3.3 with a pinned archive hash.
- [Windows signing verification](https://github.com/jbcupps/abigail/blob/ea92effd31f554f74bb5e109deaa42876ec4f06f/scripts/windows-signing.psm1#L222) requires a valid trusted signature, expected thumbprint, code-signing EKU, SHA-256 and a verified RFC 3161/SHA-256 timestamp, then runs SignTool. The [installer script](https://github.com/jbcupps/abigail/blob/ea92effd31f554f74bb5e109deaa42876ec4f06f/scripts/build_signed_installer.ps1#L387) verifies installed application/internal binaries and uninstaller before success.
- [Committed download page](https://github.com/jbcupps/abigail/blob/ea92effd31f554f74bb5e109deaa42876ec4f06f/docs/index.html#L30) points both Windows links to `/releases/latest/download/Abigail-windows-x64-setup.exe`. GitHub Pages is configured for `main:/docs`; the latest-release API resolved to `v0.0.75` at inspection time.

## Minimal change and remaining action

The accompanying workflow change rejects an unsigned **stable publication** before any tag or release side effects. It relies on the existing signed-build verification gates; it does not claim to implement signature verification itself. Unsigned candidate artifacts (`publish_release=false`), beta stabilization and the manual artifact-only workflow remain available. This guard cannot retroactively repair the existing stable asset.

The public download requires a separately authorized, tested, signed stable promotion. Preserve the historical artifact; do not silently replace it, retag it, or represent a beta as stable. Until then, anyone deliberately testing the beta can use the explicit beta release above and compare its exact hash and publisher.

No new certificate, signing provider, enrollment or purchase is indicated by the verified beta. Existing SSL.com account entitlements and future billing cannot be inferred from an artifact. Do not change credentials, private keys, certificate trust or SmartScreen settings to address reputation warnings.

To finish identifying the reported warning, record its exact wording, displayed publisher, source URL and the user's local file hash. A normal Windows elevation consent prompt showing a verified publisher is also different from an unknown publisher or SmartScreen reputation warning. These checks do not require launching the installer:

```powershell
Get-FileHash -LiteralPath '<downloaded-installer.exe>' -Algorithm SHA256
Get-AuthenticodeSignature -LiteralPath '<downloaded-installer.exe>'
signtool verify /pa /all /v /tw '<downloaded-installer.exe>'
```

## Primary guidance

- [Microsoft: SmartScreen reputation for developers](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation): valid signing and file/publisher reputation are separate. Current guidance does not promise warning-free downloads; EV no longer grants immediate reputation.
- [Microsoft: current distribution behavior](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/distribution-feature-status): EV and Artifact Signing still accumulate reputation.
- [Microsoft: SignTool](https://learn.microsoft.com/en-us/windows/win32/seccrypto/signtool): `/pa` selects Authenticode policy, `/all` verifies every signature and `/tw` warns about missing timestamps.
- [Microsoft: Get-AuthenticodeSignature](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.security/get-authenticodesignature): file signature inspection without execution.
- [SSL.com: CI signing configuration](https://www.ssl.com/how-to/integrating-esigner-with-ci-cd-pipelines-a-complete-setup-and-configuration-guide/): production and TEST/sandbox are distinct; sandbox signatures are not publicly trusted. The verified beta has a production trusted chain.
- [Tauri: Windows signing](https://v2.tauri.app/distribute/sign/windows/) and [updater signing](https://v2.tauri.app/plugin/updater/#signing-updates): updater `.sig` files are separate from Windows Authenticode publisher signatures.

This is a dated evidence record. Recheck release selection and artifact hashes before applying these findings to later downloads.
