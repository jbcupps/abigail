# Security Policy

## Supported versions

| Version | Supported |
| --- | --- |
| 0.0.x | Yes |

## Reporting a vulnerability

Do not open a public GitHub issue for a security vulnerability.

Use GitHub private vulnerability reporting and include:

1. A clear description of the issue.
2. Reproduction steps.
3. Expected impact.
4. Any suggested mitigation.

## Scope

The following are in scope:

- Abigail desktop application and daemons
- Vault unlock and sealed local storage
- Hive identity signing and constitutional verification
- Portable archive and recovery export handling
- Signed skill allowlist enforcement
- Updater signing, release-signing, or `latest.json` generation issues
- Skill sandbox escape or trust-policy bypass
- Local-only HTTP boundary issues, including SSRF validation regressions

## Security practices

- Abigail enforces a fail-closed trust chain: stable KEK -> Hive master key -> Hive signature over `external_pubkey.bin` -> constitutional document signatures.
- Local vault storage uses AES-256-GCM with atomic writes.
- Passphrase fallback uses Argon2id with per-install metadata for new vaults.
- Portable archive v2 uses X25519 + HKDF-SHA256 + AES-256-GCM with authenticated headers.
- Signed skill allowlist entries are verified against `trusted_skill_signers` and fail closed on malformed or untrusted input.
- Official release workflows require updater signing inputs, Windows code signing inputs, and macOS signing/notarization inputs.
