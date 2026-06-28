# Security and Privacy Style Guide

Abigail is local-first family software. UI and frontend code must protect private family data, model credentials, local memory, prompts, backups, and Entity boundaries.

## Baselines

- [OWASP ASVS](https://owasp.org/www-project-application-security-verification-standard/) for application security expectations.
- [OWASP Cheat Sheet Series](https://cheatsheetseries.owasp.org/) for input validation, logging, session management, secrets, and secure coding practices.
- GDPR-style privacy principles where applicable: lawfulness, fairness, transparency, purpose limitation, data minimization, accuracy, storage limitation, integrity, confidentiality, and accountability.

## Data Classification

| Class | Examples | UI rule |
| --- | --- | --- |
| Public product data | App version, static docs | Safe to display |
| Local private data | Entity names, memories, family preferences | Display only in expected local context |
| Sensitive secrets | API keys, private keys, tokens | Never show by default, never log |
| Highly sensitive recovery data | Mentor private key, recovery bundle | One-time ceremony, explicit save/backup warnings |
| Diagnostics | Local paths, daemon errors, IDs | Hide behind advanced/details UI |

## Secure Input Handling

Rules:

- Validate on the client for usability and on the daemon/server for authority.
- Prefer allowlists for structured fields.
- Trim names and labels.
- Limit length for all user-controlled strings.
- Normalize Unicode where identity comparison matters.
- Never trust values from local storage, query strings, webviews, or daemon responses without validation.

Example:

```ts
export function validateEntityName(value: string): string | null {
  const name = value.trim();
  if (name.length === 0) return "Enter a name before creating an Entity.";
  if (name.length > 48) return "Use 48 characters or fewer.";
  if (!/^[\p{L}\p{N} .'-]+$/u.test(name)) {
    return "Use letters, numbers, spaces, apostrophes, periods, or hyphens.";
  }
  return null;
}
```

## Secrets and Credentials

UI requirements:

- Secret inputs use `type="password"` by default.
- Reveal control has text: `Show key` / `Hide key`.
- Secret values are never stored in React state longer than necessary.
- Do not put secrets in URLs, telemetry, localStorage, logs, toast text, screenshots, or error details.
- Clipboard copy for secrets requires an explicit click and a short-lived success message.

Example:

```tsx
<label htmlFor="api-key">API key</label>
<input
  id="api-key"
  type={showKey ? "text" : "password"}
  autoComplete="off"
  spellCheck={false}
  value={apiKey}
  onChange={(event) => setApiKey(event.target.value)}
/>
<button type="button" onClick={() => setShowKey((value) => !value)}>
  {showKey ? "Hide key" : "Show key"}
</button>
```

## Error Handling

Safe error pattern:

```ts
try {
  await connectProvider(input);
} catch (error) {
  console.warn("[ProviderWizard] connect failed", safeErrorCode(error));
  setError("That provider could not be connected. Check the key and try again.");
}
```

Rules:

- User messages should be helpful but not reveal implementation internals.
- Diagnostic details live behind an advanced disclosure.
- Do not echo request payloads.
- Do not distinguish secret existence unless necessary.
- Retry actions must not duplicate destructive operations.

## Logging

Log:

- Startup phase.
- Daemon health state.
- Provider connection result code, not secret.
- Security-relevant denied action.
- Backup created/restored with safe identifier.

Do not log:

- API keys, tokens, private keys.
- Prompt text or chat content unless the user explicitly exports diagnostics.
- Full local file paths in routine logs.
- Family member names in remote telemetry.
- Model raw responses containing private data.

Log shape:

```ts
console.info("[Hive] provider connection tested", {
  provider: "openai",
  result: "success",
});
```

## Privacy UX

The UI must make data movement clear:

- Local model: `This stays on this computer.`
- Cloud provider: `Messages sent with this model are processed by [Provider].`
- Backup: `This backup may contain private family memory. Store it somewhere safe.`
- Recovery key: `This is the only time Abigail will show this key.`

Do not claim absolute safety. State concrete behavior.

## Session and Authentication

Current local desktop posture:

- Prefer local OS storage and OS-level user session boundaries.
- Lock or re-authenticate before showing highly sensitive recovery material.
- Any future remote account session must use secure, httpOnly cookies or platform-native secure storage, not localStorage tokens.
- Inactive sensitive dialogs should close or obscure content after a short timeout.

## Entity Boundary Rules

- One Entity must not read another Entity's records accidentally.
- UI calls must include explicit Entity IDs only when scoped to that Entity.
- Hive-owned management screens may list Entities, but runtime chat screens should only receive the active Entity context.
- Cross-Entity actions require explicit wording.

Example:

```text
Good:
Share this memory with Ada and Heph?

Bad:
Sync all memories.
```

## Configuration Management

- Secrets come from secure stores or explicit user entry.
- Feature flags must default to safest behavior.
- Signing/updater release variables remain opt-in.
- Do not commit `.env` files, generated keys, private backups, or local databases.
- CI should fail on accidental secret patterns when a scanner is available.

## Secure UI Defaults

- Disable submit while a request is in flight.
- Use idempotency or in-flight guards for creation actions.
- Confirm irreversible deletes.
- Prefer local-only operation when cloud provider setup is incomplete.
- Sanitize user-provided URLs before opening.
- Use Tauri APIs through narrow wrappers rather than calling privileged APIs from arbitrary components.

## Recommended Libraries and Tools

Current/near-term:

- TypeScript strictness for UI type safety.
- React Testing Library for accessible behavior tests.
- `zod` or equivalent schema validation for structured frontend inputs if added.
- `eslint-plugin-jsx-a11y` for accessibility linting.
- `cargo audit`, Dependabot, and GitHub code scanning for dependency/security drift.

Future:

- Secret scanning in CI.
- SBOM generation for release artifacts.
- Explicit threat model for Hive, Entity Runtime, and Forge flows.

## Security Review Checklist

Before merging UI that handles private data:

- Inputs are validated and length-limited.
- Secrets are hidden by default.
- Errors are safe.
- Logs are scrubbed.
- Destructive actions confirm object name.
- Entity IDs are scoped correctly.
- Cloud data movement is disclosed.
- Keyboard and screen reader users can complete the flow.
- Tests cover disabled/loading/error states.
