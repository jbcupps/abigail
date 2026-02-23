# Threat Model

This document focuses on threats introduced by the skills system, MCP integration, and self-extension. For key management, storage, and constitutional integrity see [SECURITY_NOTES.md](SECURITY_NOTES.md).

## Scope

- **In scope:** Skill supply chain, MCP server trust, UI sandbox (MCP Apps), data exfiltration via skills or MCP.
- **Out of scope:** General host security, OS hardening, physical access (covered elsewhere or by platform).

## 1. Skill supply-chain abuse

### Threat

A user installs a skill package that is malicious or compromised (e.g. exfiltrates data, escalates privileges, or abuses granted permissions). Packages may come from untrusted directories or future “skill stores.”

### Mitigations

| Control | Status |
|--------|--------|
| **Approval gating** | Implemented. If `approved_skill_ids` is set, only listed skills can run. Install adds the skill id to the list; uninstall removes it. |
| **Audit trail** | Implemented. Install, uninstall, and approve are logged to `{data_dir}/skill_audit.log` with timestamp and `skill_id`. |
| **Signed packages** | Planned. Config has `trusted_skill_signers`; format and verification (e.g. Ed25519-signed manifest + checksums) not yet implemented. |
| **Permission review** | Manifest declares permissions; UI can show them before install/approve. Execution checks sandbox before network (and optionally file/memory) access. |

### Abuse cases

- **Malicious skill runs without user consent:** Mitigated by approval list; unknown skills are not in the list and are rejected at execution.
- **Trojanized skill update:** Mitigated when signing is in place (reject if signature invalid or signer not trusted). Until then, users should re-approve only after reviewing source.
- **Privilege creep:** Sandbox and resource limits (timeout, concurrency) bound what a skill can do; capability layers must route I/O through the sandbox.

## 2. MCP server trust

### Threat

An MCP server (stdio or HTTP) is malicious or compromised, or the client talks to an unintended host (e.g. misconfiguration or DNS hijack), leading to data exfiltration or abuse of tools/resources.

### Mitigations

| Control | Status |
|--------|--------|
| **Explicit server list** | Implemented. Only servers in `AppConfig.mcp_servers` are used. |
| **Trust policy** | Implemented. `mcp_trust_policy` (e.g. `allowed_http_hosts`) restricts which HTTP hosts are allowed. |
| **Tool confirmation** | UI responsibility. Tools with `requires_confirmation` should prompt the user before invocation. |
| **No secrets to MCP by default** | MCP server env and URLs are configured explicitly; secrets are not automatically passed unless the user configures them. |

### Abuse cases

- **Server sends user data to attacker:** Mitigated by configuring only trusted servers and, for HTTP, using an allowlist so that only intended hosts are contacted.
- **Tool runs destructive action without consent:** Mitigated by UI gating for `requires_confirmation`; backend does not enforce confirmation.

## 3. UI sandbox escape (MCP Apps)

### Threat

MCP Apps render `ui://` resources in the desktop client. A malicious or compromised app could try to escape the iframe, access host APIs, or escalate to full app privileges.

### Mitigations

| Control | Status |
|--------|--------|
| **Sandboxed iframe** | Implemented. Content is rendered via `srcDoc` in an iframe with restricted `sandbox` (e.g. `allow-scripts allow-same-origin`). No `allow-same-origin` would break many apps; document that same-origin is required and that app content must be from a trusted MCP server. |
| **CSP** | Tauri/WebView CSP is set in `tauri.conf.json`; limits script/style/connect sources. |
| **No elevated bridge** | MCP App content is fetched over the existing MCP connection; no separate privileged channel is exposed to the iframe beyond what the frontend already has. |

### Abuse cases

- **Script in iframe accesses parent or Tauri APIs:** Mitigated by CSP and iframe sandbox; postMessage bridge should be minimal and not expose sensitive commands.
- **Malicious server returns HTML that phishes or runs script:** Mitigated by treating MCP Apps as same trust as the MCP server; allowlist servers and use confirmation for sensitive tools.

## 4. Data exfiltration

### Threat

A skill or MCP tool sends user or system data to an external party without consent (e.g. via network permission or by abusing another capability).

### Mitigations

| Control | Status |
|--------|--------|
| **Network permission** | Implemented. Tools that need network declare it; sandbox allows only granted domains (Full, LocalOnly, or Domains list). |
| **Local LLM SSRF** | Implemented. Local LLM URL is validated to localhost/127.0.0.1/::1 only. |
| **Audit log** | Implemented for install/uninstall/approve; execution audit is in sandbox (e.g. network requests) for future analysis. |
| **Resource limits** | Implemented. Timeout and concurrency limit long-running or runaway tool use. |

### Abuse cases

- **Skill with network permission exfiltrates memory or files:** Mitigated by granting only the permissions needed and, where possible, restricting to specific domains; memory/storage caps (when enforced) limit scope.
- **MCP server forwards prompts/responses to third party:** Mitigated by configuring only trusted MCP servers and using HTTP allowlist so that connections go only to intended hosts.

## 5. Hive/Entity daemon threats

### Threat

The Hive and Entity daemons communicate over HTTP on localhost. An attacker with local access could impersonate one daemon to the other, intercept provider config (including API keys), or send malicious requests.

### Mitigations

| Control | Status |
|--------|--------|
| **Localhost binding** | Implemented. Both daemons bind to `127.0.0.1` only — not exposed to LAN or internet. |
| **No raw secret exposure** | Implemented. Hive's `/v1/entities/:id/provider-config` returns resolved provider config. Entity never accesses the raw `SecretsVault`. |
| **CORS** | Implemented. Both daemons use `tower-http` CORS layer. Currently permissive (`Any`) for development; should be restricted in production. |
| **Auth tokens** | Planned. Hive should issue a short-lived token to Entity at startup for subsequent API calls. Not yet implemented. |

### Abuse cases

- **Local process reads Hive API keys:** Mitigated by localhost-only binding; any local process with network access can reach the daemon. Future: add bearer token auth.
- **Entity daemon spoofing:** A malicious process could impersonate entity-daemon to hive-daemon. Mitigated when auth tokens are implemented.

## Document maintenance

- Update this document when adding new extension points (e.g. WASM runtime, new MCP transports, daemon auth) or when changing approval, signing, or sandbox behavior.
- See [SECURITY_NOTES.md](SECURITY_NOTES.md) for key management, DPAPI, constitutional integrity, and Hive/Entity security boundaries.
