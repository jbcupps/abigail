# Installed tools and model readiness

Base: `a18249a3b622e8b34d0f20ec1f0e24fc0dc7183d`. The running hands-on preview and its profile are unchanged.

## Regression contract

- Discovery is separate from connection. Presence alone never means authenticated, selected, or able to answer. No discovery request sends an inference prompt or starts login.
- Isolated previews may report executable presence using the existing native executable resolver, without executing installed tools or inspecting account files. Existing preview restrictions on CLI account access remain enforced at selection and construction.
- Ordinary profiles retain existing supported Claude Code, Codex and Grok Build integrations. Unknown status is distinct from signed out. Gemini CLI is explicitly unsupported in onboarding; Google API remains available.
- Discovery failures remain visible and retryable. API and Ollama/LM Studio setup remain available throughout.
- A user selects a CLI and sees the existing response-check action and its usage implications before initiating it. Only a successful backend response reports a connection. Login stays in the vendor's own tool; no imported tokens, automatic grants, or persistent access changes.
- Hive service availability is separate from model selection. Show the actual resolved selection without claiming a fresh live model check. Missing-provider conversation attempts offer setup directly and never rewrite the saved Entity, identity, draft, or selected session.

## Vendor routes checked 2026-10-21

- [Claude Code authentication](https://code.claude.com/docs/en/authentication) and [CLI reference](https://code.claude.com/docs/en/cli-reference): vendor-owned login and `claude auth status`. [Product integration conditions](https://code.claude.com/docs/en/legal-and-compliance) remain relevant to distribution; no agreements are accepted here.
- [Codex app-server](https://learn.chatgpt.com/docs/app-server): local/open-source integration, managed ChatGPT sign-in and account status. Commercial/hosted use has separate restrictions; no new auth integration is introduced here.
- [Grok Build ACP](https://docs.x.ai/build/cli/headless-scripting) and [CLI reference](https://docs.x.ai/build/cli/reference): vendor sign-in through `grok login`; ACP connection uses that account. No documented passive login-status command is assumed.
- [Gemini CLI authentication](https://geminicli.com/docs/get-started/authentication/): interactive Google sign-in or API/Vertex credentials. This does not establish an Abigail onboarding adapter; do not invent `gemini auth login/status` support.

Consumer desktop apps and browser sessions are not callable model endpoints. No credentials, subscriptions, or real model responses are accessed during this source work. Tests use fixtures. Enabling CLI account use inside an isolated preview remains a separate adapter-isolation task, not a UI toggle.

## Family setup hierarchy and automatic-routing boundary

The first missing-model startup offers a brief, postponable invitation. Its display preference is saved per persistent Hive identity in app storage; it is not credential or routing consent. Unfinished or unreadable creation journals suppress the automatic invitation and remain intact. Settings → Model connections stays available. The Entity home remains primary, with a compact saved-model indicator once a connection exists. Account names lead the options; implementation names remain secondary.

**Not implemented in this slice:** a multi-provider allowed set, dynamic provider selection or automatic account access. No checkbox claims to enable these. Inspection found that `HiveConfig`/`ProviderConfig` carry one primary provider and optional local URL, `IdEgoRouter::execute_with_fallback` only switches between those two paths, and Entity provider configuration inherits Hive defaults only when its own selection is absent. Genesis also resolves provider configuration separately. A frontend allowlist alone would not constrain these execution paths or live provider refresh.

The smallest next routing stage is to make an explicit approved primary plus approved local fallback authoritative in Hive, propagate it through Entity refresh and Genesis, and reject unapproved/unavailable fallbacks before construction. Then extend the same contract to an ordered pool of approved cloud/account connections if needed. A per-message multi-cloud planner is a broader alternative and is not recommended as the next step. Tests for unavailable approved providers, no-ready-model routing and forbidden fallback belong to that enforcement stage; current tests establish missing-model UI and preview-account restrictions only. Do not infer those future routing guarantees from this source update.

## Validation on AILap, 2026-10-21

| Check | Result |
| --- | --- |
| Hive UI full suite | 344 passed across 22 files |
| Entity Runtime UI full suite | 107 passed across 7 files |
| Both frontend production builds | Passed |
| `cargo check -p abigail-hive-app -p abigail-entity-runtime-app --locked --offline` | Both native shells passed compilation; neither was launched |
| `cargo test -p abigail-capabilities --lib cognitive::cli_provider::tests --locked --offline` | 19 passed |
| `cargo test -p hive-daemon routes::tests --locked --offline` | 10 passed; unrelated integration tests were filtered out |
| `cargo clippy -p hive-daemon -p abigail-capabilities --all-targets --locked --offline -- -D warnings` | Passed |
| Rust formatting of the three changed Rust files; `git diff --check` | Passed |

New regressions cover preview presence without any account probe, inconclusive authentication, malformed discovery, retry after discovery failure, refusal of unrecognized tools, explicit connection checks, mismatched backend selection, missing-model navigation without Entity mutation, foreign-Hive recovery, first-run postponement, preserved unfinished creation, Settings lock during persistence, and keyboard focus with collapsed discovery. Three pre-existing cloned-reference test slices were changed to `std::slice::from_ref` for strict Clippy; behavior is unchanged.

Browser review used the actual source components with an explicitly labeled in-memory fixture on loopback port 1461. All requests were intercepted; mutations were rejected, and no daemon, account or inference endpoint was contacted. Checked the Settings entry, missing-model home, friendly account states, postponable first invitation, and narrow 390-pixel layout. The fixture's first-run display was opened manually; App tests cover automatic invitation and persistence. The temporary browser tab and development server were closed after review.

Raw logs and screenshots are in ignored `output/provider-readiness/`, including `04-settings.jpg`, `05-friendly-accounts.jpg` and `06-first-run.jpg`. These are source UI evidence, not installed/native or real-account acceptance. No user-preview binaries/profile were replaced or restarted. The prepared `a18249a3` build-state hash and all five paused Store file hashes match their recorded values.

Toolchain: Node 24.14.0, npm 11.9.0, Rust 1.97.0. Dependencies used the committed lockfiles; no dependency pins changed.

Remaining acceptance: build new isolated preview bytes when requested, then validate a supported real model and creation/conversation/return with user-authorized account access. Native screen capture is still unavailable. Enabling isolated-preview CLI account adapters and enforcing an approved routing set remain separate source work; this branch does not remove either limitation.
