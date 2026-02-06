# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.0.x   | Yes                |

## Reporting a Vulnerability

If you discover a security vulnerability in AO, please report it responsibly.

**Do NOT open a public GitHub issue for security vulnerabilities.**

### How to Report

1. **Email**: Send a detailed report to the repository owner via GitHub private vulnerability reporting (Settings > Security > Advisories > "Report a vulnerability").
2. **Include**:
   - Description of the vulnerability
   - Steps to reproduce
   - Potential impact
   - Suggested fix (if any)

### What to Expect

- **Acknowledgment**: Within 48 hours of your report.
- **Assessment**: We will evaluate the severity and impact within 7 days.
- **Fix timeline**: Critical vulnerabilities will be patched as soon as possible. Non-critical issues will be addressed in the next scheduled release.
- **Disclosure**: We will coordinate disclosure timing with you. We ask that you do not publicly disclose the vulnerability until a fix is available.

### Scope

The following are in scope for security reports:

- AO desktop application (Tauri backend and React frontend)
- Ed25519 signature verification and key management
- DPAPI secrets handling (Windows)
- Skill sandbox escape or permission bypass
- Local LLM endpoint SSRF or injection

### Out of Scope

- Vulnerabilities in third-party dependencies (report these upstream; we monitor via Dependabot and `cargo audit`)
- Social engineering
- Denial of service on local-only interfaces

## Security Practices

- All constitutional documents are signed with Ed25519 and verified at every boot
- Secrets are encrypted via Windows DPAPI (user scope) and never stored in plaintext
- Skills run in a permission-checked sandbox with declared manifests
- Local LLM URLs are validated to prevent SSRF (localhost/loopback only)
- GitHub Actions use pinned commit SHAs to prevent supply chain attacks
- Dependency audits run automatically via `cargo audit` and `npm audit` in CI
