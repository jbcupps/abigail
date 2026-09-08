# CLAUDE.md - Abigail Project Constitution for Coding Agents

You are helping build Abigail - the private Entity Coordinator and Manager for real homes and families.

**Core Mission**
Abigail manages multiple personal AI Entities that the user, mentor, or family head creates. The family interacts directly with those Entities. The product mission is to put persistent, highly configurable, ethically governed agents into the hands of everyday people through one easy install and one obvious app launch. Your job is to keep Abigail simple, private, delightful, and genuinely useful for everyday family life.

**Development Rules (Always Follow)**
- The person talks to Abigail first for application setup, then creates and manages persistent Entities. Use **Abigail** for the coordinator in family-facing UI; Hive remains an internal technical name.
- Abigail retains the immortal local coordinator identity. Its bounded setup conversation runs in `hive-daemon`; model output has no tools or mutation authority. Do not launch a duplicate helper daemon.
- `Abigail Hive` stays open and usable even when a family-facing Entity is active.
- Provider and model management belongs to `Abigail Hive`, never inside Entity chat or Entity-specific settings.
- `Abigail Hive` owns the shared embedded SurrealDB persistence root (`memory.db`) and legacy SQLite files are migration inputs only, never active runtime stores.
- The stable internal direction is two interoperable applications: a Hive control app and a chat-first Entity Runtime app. Use authenticated HTTP boundaries between processes; setup orchestration belongs inside the coordinator.
- The family-facing direction is one installed `Abigail` app icon. Packaging must hide internal split binaries and start Hive, Entity Runtime, and daemons for the family automatically.
- `beta` is the permanent primary iteration branch. Target normal implementation PRs to `beta`, use the beta release automation for UAT, and promote only validated beta changes to `main`.
- Every beta UAT build must be traceable to a GitHub prerelease tag in the form `vX.Y.Z-beta.N`.
- Users should be encouraged to connect Entities to powerful cloud models from any provider. This multi-provider freedom is a major advantage.
- Current dev builds prioritize a clean working single-version experience over cross-version compatibility. Remove stale legacy paths when they conflict with the active Hive-first architecture.
- Unsigned stabilization builds are the default local path. Release signing and updater signing are beta/release-only concerns that should stay opt-in and isolated from day-to-day development.
- Privacy and local-first are non-negotiable. Cloud models are optional power-ups, never required.
- Keep per-Entity data scoped through Hive-owned storage interfaces so one Entity cannot read another Entity's records by accident.
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

**Initial Setup Invariants (2026-09-08)**
- First chat must work offline immediately after installation. Package the pinned runtime, model, and licenses; do not depend on a first-run download, Docker, or a user-installed Ollama.
- A real generated local completion determines readiness. Loading, pause, failure, retry, and local fallback must remain visible and recoverable.
- Keep API credentials out of prompts and transcripts. Validate the exact provider/model with a real completion, then persist and activate the encrypted connection atomically. Preserve identity and transcript through handoff and restart.
- Require an explicit caller token for all daemon routes except public health. Entity credentials may access only their own persistence/runtime scope; never inherit the desktop's root bearer. Keep each Runtime window's token in its own launch command.
- `hive-daemon` alone opens shared SurrealKV storage. Database sessions are separate per scope, and remote SQL cannot change session/schema or access the network.
- Household accounts and the complete cryptographic ethics path are not implemented by these process credentials. Do not present them as completed protections.
- See `docs/INITIAL_SETUP.md` for the acceptance probes and remaining installer UAT.

Application-owned setup guidance must remain visible independently of model prose:
API accounts are provider developer accounts; API keys are separate secrets created
inside those accounts. Only fixed official provider links and the secure connection
form direct credential entry. The live connection status and next setup action come
from application state. Qwen3.5-0.8B free-form explanations can confuse these terms,
even with examples; never treat its wording as proof of setup completion.
