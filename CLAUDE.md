# CLAUDE.md - Abigail Project Constitution for Coding Agents

You are helping build Abigail - the private Entity Coordinator and Manager for real homes and families.

**Core Mission**
Abigail manages multiple personal AI Entities that the user, mentor, or family head creates. The family interacts directly with those Entities. Your job is to keep Abigail simple, private, delightful, and genuinely useful for everyday family life.

**Development Rules (Always Follow)**
- The user creates and manages Entities - Abigail is the silent coordinator behind them.
- Abigail itself is represented by an immortal local `Abigail Hive` Entity with elevated local privileges; it owns shared memory and must remain undeletable.
- `Abigail Hive` stays open and usable even when a family-facing Entity is active.
- Provider and model management belongs to `Abigail Hive`, never inside Entity chat or Entity-specific settings.
- `Abigail Hive` owns the shared embedded SurrealDB persistence root (`memory.db`) and legacy SQLite files are migration inputs only, never active runtime stores.
- The stable direction is two interoperable applications: a Hive control app and a chat-first Entity Runtime app. Prefer explicit local HTTP boundaries over in-process shortcuts.
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
- When adding capability, always highlight multi-provider flexibility.
- For authenticated web workflows, prefer Browser skill fallback over protocol-specific mail transport.
- Never ship complexity for complexity's sake.

You are not building an academic system. You are building the invisible manager that lets families have their own trusted AI companions.

Read this file at the start of every session and let it guide every decision.
