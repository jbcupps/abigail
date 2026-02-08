# Security Notes

## Key Management

### External Signing Keypair (Ed25519)
- **Generated at first run** by the user's Abigail instance
- **Private key is shown ONCE** during initial setup - user MUST save it securely
- **Private key is NEVER stored** by Abigail - only the public key is retained
- **Public key location:** `{data_dir}/external_pubkey.bin` (auto-detected)
- **Purpose:** Signs constitutional documents (soul.md, ethics.md, instincts.md)

### Internal Keyring (Mentor Keypair)
- **Generated automatically** at first run
- **DPAPI-protected** on Windows (user scope), plaintext stub on other platforms (dev only)
- **Purpose:** Internal operations (signing memories, etc.)

## Storage Security

- **DPAPI:** Keyring and email passwords use Windows DPAPI when available
- **Non-Windows:** Plaintext stub with warning logged at startup (for development only). Do not use for production on macOS/Linux until a cross-platform secret store is integrated.
- **Keys file:** `{data_dir}/keys.bin` (DPAPI-encrypted)

## Secrets Handling

- **No secrets in repo:** API keys, passwords never committed
- **Environment:** Use `example.env` as template; `.env` is gitignored
- **Email passwords:** Encrypted via DPAPI before storage in config
- **Secret key namespace:** `store_secret` / `check_secret` / `remove_secret` accept only (1) reserved provider names: `openai`, `anthropic`, `xai`, `google`, `tavily`, or (2) secret names declared in a skill’s `skill.toml` (under `secrets[].name`). Other keys are rejected to avoid overwriting provider keys or polluting the vault.
- **Logging:** Logs must not contain API keys, passwords, or other secrets. User-controlled paths (e.g. backup destination) are not logged in full. HTTP clients used for API key validation do not log request URL or body.

## Constitutional Document Integrity

- **Signed at first run:** soul.md, ethics.md, instincts.md are signed when keypair is generated
- **Verified at every boot:** Signatures checked against the stored public key
- **Immutable:** Abigail refuses requests to modify constitutional docs
- **Recovery:** If user loses private key, they cannot re-sign after reinstall

## First Run Security Flow

1. User clicks "Start" in boot sequence
2. Abigail generates Ed25519 keypair
3. Constitutional documents are signed with the private key
4. **CRITICAL:** Private key is displayed with security warnings
5. User must acknowledge they've saved the key before proceeding
6. Private key is cleared from memory (never stored)
7. Only the public key remains for future verification

## Local LLM URL (SSRF Mitigation)

- **Validation:** The local LLM base URL is validated whenever it is set (UI, birth flow, or from `LOCAL_LLM_BASE_URL` env) and when loaded from config. Only **http** or **https** URLs are allowed. The host must be **localhost**, **127.0.0.1**, or **::1**. Private IP ranges (e.g. 169.254.x.x, 10.x, 192.168.x) and other hosts are rejected to prevent SSRF (e.g. cloud metadata, internal services).
- **Defense in depth:** The HTTP provider re-validates the URL before each heartbeat and completion request. If config was tampered with, the first request will fail.

## Dependency and CI Security

- **CI:** `.github/workflows/security-audit.yml` runs `cargo audit` and `npm audit --audit-level=high` on push to main and on pull requests. The build fails on high/critical advisories (with an option to document exceptions if needed).
- **Dependabot:** `.github/dependabot.yml` is configured for Cargo, npm (tauri-app/src-ui), and GitHub Actions with weekly checks and PRs for updates.

## Content Security Policy (CSP)

- A strict CSP is set in `tauri.conf.json` under `app.security.csp`: `default-src 'self'`, `script-src 'self'`, `style-src 'self' 'unsafe-inline'` (Tailwind/inline styles), `connect-src 'self'` and localhost for dev/LLM, `img-src`/`font-src` as needed. This reduces XSS and content-injection risk. If you add new script or style sources, document them here.

## Path Validation

- **Backup:** The SQLite backup destination path is validated before write. Allowed bases: the app data directory, and (on Windows) `%USERPROFILE%\Documents`, (on Unix) `$HOME/Documents` and `$HOME`. Path traversal (e.g. `..`) is rejected; the resolved parent of the destination must be under one of these bases. Prefer using the native Save dialog (Data Archives → Backup), which lets the user pick a path; the backend re-validates before copying.

## Skill Sandbox

- **Network:** The executor checks the sandbox for network permission before running a tool that declares a network permission (domain allowlist). Other resource access (file, memory) uses the same sandbox logic but must be invoked by the code path that performs the I/O (e.g. a capability layer). Skill code that performs raw file or network I/O should go through a layer that calls the sandbox.
- **Resource limits:** Timeouts and concurrency are enforced at runtime: each tool call is bounded by `ResourceLimits::max_cpu_ms` (default 30s), and global concurrency by `max_concurrency` (default 10). Memory and storage caps are intended for capability layers and/or a future WASM runtime (see `crates/abigail-skills/src/runtime/wasm.rs`).

## Skill Packaging and Approval

- **Approval gating:** If `approved_skill_ids` is non-empty in config, only skills in that list may execute tools. Install and approve flows update this list and persist it to `config.json`.
- **Audit log:** Install, uninstall, and approve actions are appended to `{data_dir}/skill_audit.log` with timestamp and detail (e.g. `skill_id=...`) for traceability.
- **Signing (path):** Config supports `trusted_skill_signers` for a future signed-package format. Currently, install copies a directory with a valid `skill.toml` into `{data_dir}/skills/<id>/`; signature verification of packages is not yet implemented.

## MCP Trust

- **Server definitions:** MCP servers are configured in `AppConfig.mcp_servers` (id, name, transport, command or URL, env). Only explicitly configured servers are used.
- **Trust policy:** `mcp_trust_policy` (e.g. `allow_list_only`, `allowed_http_hosts`) restricts which HTTP hosts are allowed for stdio/HTTP MCP. Use allowlists to avoid data exfiltration to untrusted hosts.
- **Tool confirmation:** Tools that declare `requires_confirmation` should be gated in the UI before invocation; the backend does not enforce confirmation (UI responsibility).

## Threat Model Summary

| Threat | Mitigation |
|--------|------------|
| Tampered constitutional docs | Signature verification at boot |
| Lost private key | Clear warnings during setup; user responsibility |
| Compromised private key | User can detect via failed verification |
| Man-in-the-middle on download | Installer signatures (future: code signing) |
| Local privilege escalation | DPAPI uses user scope, not machine scope |
| Skill supply-chain abuse | Approval list; audit log; (future) signed packages + trusted signers |
| MCP server exfiltration | Per-server config; HTTP allowlist in trust policy |
| UI sandbox escape (MCP Apps) | Sandboxed iframe + CSP; no elevated privileges to host |
