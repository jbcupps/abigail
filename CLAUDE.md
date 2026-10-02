# CLAUDE.md - Abigail Project Constitution for Coding Agents

You are helping build Abigail - the private Entity Coordinator and Manager for real homes and families.

**Core Mission**
Abigail manages multiple personal AI Entities that the user, mentor, or family head creates. The family interacts directly with those Entities. The product mission is to put persistent, highly configurable, ethically governed agents into the hands of everyday people through one easy install and one obvious app launch. Your job is to keep Abigail simple, private, delightful, and genuinely useful for everyday family life.

**Development Rules (Always Follow)**
- The user creates and manages Entities - Abigail is the silent coordinator behind them.
- Abigail itself is represented by an immortal local `Abigail Hive` Entity with elevated local privileges; it owns shared memory and must remain undeletable.
- `Abigail Hive` stays open and usable even when a family-facing Entity is active.
- Provider and model management belongs to `Abigail Hive`, never inside Entity chat or Entity-specific settings.
- `Abigail Hive` owns the shared embedded SurrealDB persistence root (`memory.db`) and legacy SQLite files are migration inputs only, never active runtime stores.
- The stable internal direction is two interoperable applications: a Hive control app and a chat-first Entity Runtime app. Prefer explicit local HTTP boundaries over in-process shortcuts.
- The family-facing direction is one installed `Abigail` app icon. Packaging must hide internal split binaries and start Hive, Entity Runtime, and daemons for the family automatically.
- `beta` is the permanent primary iteration branch. Target normal implementation PRs to `beta`, use the beta release automation for UAT, and promote only validated beta changes to `main`.
- Every beta UAT build must be traceable to a GitHub prerelease tag in the form `vX.Y.Z-beta.N`.
- Users should be encouraged to connect Entities to powerful cloud models from any provider. This multi-provider freedom is a major advantage.
- Current dev builds prioritize a clean working single-version experience over cross-version compatibility. Remove stale legacy paths when they conflict with the active Hive-first architecture.
- Unsigned stabilization builds are the default local path. Release signing and updater signing are beta/release-only concerns that should stay opt-in and isolated from day-to-day development.
- Privacy and local-first are non-negotiable. Cloud models are optional power-ups, never required.
- Keep per-Entity data scoped through Hive-owned storage interfaces so one Entity cannot read another Entity's records by accident.
- Only the Hive daemon opens the durable embedded database. Split Entity runtimes use lease-scoped HTTP persistence through Hive, including the coordinator's own conversation and per-Entity jobs.
- The unsigned Windows MVP must run with embedded frontend assets and all internal binaries. Use `scripts/build-mvp-windows.ps1`; signing and updater keys are not prerequisites.
- First-run model setup must verify a real provider. Do not claim a local model works when only a placeholder is configured. Explain that cloud providers receive conversation content and context.
- Hive may connect installed official Claude, Codex, and Grok CLIs using the user's existing account; Gemini CLI remains unsupported. Detect installations passively, and require a successful nonempty real model response on explicit selection before saving a CLI connection. Desktop installation and account metadata alone are not inference proof.
- Official CLIs own account authentication, token storage, and refresh. Abigail does not extract or forward account OAuth tokens. The CLI paths are chat-only: native tools, MCP, hooks, and extensions are disabled or rejected if isolation cannot be established. Do not alter the user's CLI settings. Automatic skill and job execution never substitutes for mentor confirmation.
- In existing-account Grok mode, do not call ACP `authenticate` even for `cached_token`, which can trigger interactive login fallback. Use the account established by `initialize`/`session/new`; missing authentication fails with a sign-in hint. Installed Codex real inference and the final desktop workflow are verified. Grok and Claude currently have negative connection acceptance only; user sign-in and actual inference remain pending. Track precise current results in `docs/MVP_WINDOWS.md`.
- Run unmodified Claude Code under [Anthropic's product-use conditions](https://code.claude.com/docs/en/legal-and-compliance#can-customers-offer-claude-code-in-their-products), with each user's own authentication and billing. Current [Codex app-server authentication](https://learn.chatgpt.com/docs/app-server#auth-endpoints) supports local/open-source use; commercial or hosted distribution requires the applicable approved Sign in with ChatGPT integration.
- Keep everything dead-simple for the family user. Delight and ease of use come first.

**Origin Story (Respect This Link)**
Abigail grew from the vision in "Toward a Decentralized Trust Framework for Verifiable and Ethically Aligned AI" (see `documents/DECENTRALIZED_TRUST_PAPER_ALIGNMENT.md`). That remains the philosophical foundation. The current mission is personal and human-scale: give families their own private, multi-capable AI Entities coordinated by Abigail.

**How You Should Work**
- Prefer the simplest solution that feels magical to a busy parent.
- Prefer `beta` as the base branch for iterative PRs unless the user explicitly asks for a stable hotfix to `main`.
- When adding capability, always highlight multi-provider flexibility.
- For authenticated web workflows, prefer Browser skill fallback over protocol-specific mail transport.
- Never ship complexity for complexity's sake.

You are not building an academic system. You are building the invisible manager that lets families have their own trusted AI companions.

Read this file at the start of every session and let it guide every decision.
