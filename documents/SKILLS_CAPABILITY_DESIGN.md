# Skills Capability & Autonomous Configuration Design

**Date:** 2026-02-20
**Status:** Design Proposal

This document outlines the architectural plan for advancing Abigail's skill system toward Level 4 Earned Autonomy, enabling the Sovereign Entity to self-configure, self-repair, and manage its own Hive operations with minimal Mentor intervention.

> Update (2026-03-05): The dynamic-only skill approach is now deprecated. Runtime skill execution topology is defined by persistent startup provisioning from `skills/registry.toml` and the Forge contract in `documents/ARCHITECTURE_SKILL_TOPOLOGY_AND_FORGE.md`.
Canonical runtime topology reference: `documents/ARCHITECTURE_SKILL_TOPOLOGY_AND_FORGE.md`.

---

## 1. The "Hive Management" Skill

To allow the Entity to autonomously perform actions currently restricted to the Mentor menus (e.g., configuring providers, changing themes, creating new identities), we will implement a dedicated **Hive Management Skill**.

### Security Model: The "Filtered Proxy"
*   **Direct Ego Access is Forbidden:** The Entity will *not* have direct memory access to the `AppState` or configuration structs. This protects the core system integrity.
*   **Targeted API Exposure:** The Hive Management skill will expose specific, safe setter functions (e.g., `update_theme`, `set_active_provider`).
*   **Superego Protection:** The Backend API will explicitly exclude any endpoints that modify or read Superego configurations, cryptographic keys, or core routing logic. The Entity can query its status but cannot alter its fundamental constraints.

---

## 2. Dynamic Skill Synthesis (The Skill Factory)

The Entity will transition from being a static consumer of tools to an active synthesizer of capabilities.

### The Decision Path
When the Entity encounters a requirement (e.g., "Automate a browser-authenticated family workflow"), it will evaluate the persistence of the need:

1.  **The Routine (Persistent Capabilities):**
    *   If the task is common, repeatable, or frequently requested (based on memory), the Entity will use a **Skill Factory** approach.
    *   It will write a formal `skill.toml` manifest and an execution script (Python, Node, or Shell) directly into the `skills/` directory.
    *   Crucially, it will author a `how-to-use.md` file within that skill's folder, documenting exactly how future Ego invocations should use the tool.

2.  **The Ephemeral (One-Off Tasks):**
    *   If the task is rare or highly specific (e.g., parsing a unique legacy log file once), the Entity will utilize generic `Shell` or `HTTP` skills to accomplish the task without polluting the registry with a permanent manifest.

---

## 3. The Sliding Scale of Autonomy (Risk-Based Interception)

Autonomy must be earned and verified. The Entity's freedom to self-configure will be governed by a configurable Risk Threshold.

### Risk Tiers
*   **Low Risk (Auto-Execute):** Utilizing verified package managers (npm, pip, cargo), interacting with official vendor APIs, or modifying superficial UI settings. The Entity proceeds autonomously.
*   **High Risk (Mentor Interception):** Executing unverified scripts from forums (Reddit, StackOverflow), installing system-wide binaries, or making network changes. The Superego intercepts these actions and mandates a **Mentor Approval Request** via the UI.

The Mentor can adjust this sliding scale globally or per-Entity.

---

## 4. Self-Inventory & The Orion Dock Strategy

The Entity must manage its own "Biology" to prevent code bloat and ensure reliability.

### The Reflection Audit
*   The Entity will run a scheduled background audit of its `skills/` directory.
*   Skills that haven't been utilized within a configurable timeframe (e.g., 30 days) will be flagged for archival or deletion.

### Orion Dock Alignment
*   This self-inventory model aligns directly with the `jbcupps/orion_dock` strategy.
*   Skills will eventually support tags, versioning, and cryptographic signatures (signed by the Hive or SAO).
*   The local `skills/` directory becomes a "checked-out" instance of a broader, verifiable registry.

---

## 5. Dual Keyvault Architecture

Credential management must be strictly compartmentalized.

### 1. The Hive Vault (System Level)
*   Stores LLM provider API keys, Superego configurations, and root cryptographic secrets.
*   **Access:** Strictly limited to the Rust backend (`IdEgoRouter` and Core). The Ego *cannot* read from this vault.

### 2. The Skills Vault (Entity Level)
*   Stores operational credentials (e.g., browser-session helpers requested by the Mentor, API keys for third-party services like weather or Jira).
*   **Access:** The Ego can request the Backend to *inject* these secrets into a skill execution context, but it cannot extract the plaintext.
*   **Protection:** Managed via DPAPI encryption, with strict prompt-level guidelines enforcing secure usage.

---

## 6. Instructional Legacy (State vs. Filesystem)

The documentation on *how* to use a skill (`how-to-use.md`) must survive LLM provider changes (e.g., moving from Claude to Gemini) and Hive migrations.

---

## 7. Persistent Topology and Forge Contract (Current Model)

- Skill topology is provisioned at startup from `skills/registry.toml`.
- Every enabled skill gets deterministic request/response topics and a dedicated subscriber worker.
- Forge-generated or updated skills are not considered active architecture until represented in the registry contract.
- Watcher updates (`skill.toml`, `*.json`, and `registry.toml`) trigger safe runtime refresh behavior, including topology re-provision on registry changes.
- Chat-topic monitors are out-of-band and must not block the forge or completion path.

### The Decision: Markdown First
*   We will store the instructional legacy as physical Markdown files (`how-to-use.md`) alongside the `skill.toml` in the filesystem.
*   **Why:** This makes every skill a "Self-Contained Unit." If a skill directory is copied to a new Abigail instance or an Orion container, the new Entity immediately understands how to use it by reading the file.
*   While a Database (Memory) approach is faster, the Markdown approach is infinitely more scalable, less fragile, and strictly aligns with the principle of **Digital Sovereignty**.
