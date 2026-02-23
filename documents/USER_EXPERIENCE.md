# Abigail User Experience (UX) Guide

This document outlines the user journey within Abigail, from initial setup to the ongoing management of Sovereign Entities.

---

## 1. The Hive Setup (Initial Launch)

When you first launch Abigail, you are entering **The Hive**—the local environment where your Sovereign Entities will live.

### Sovereign Birth Sequence
1.  **Darkness**: The initialization phase where the Hive prepares its local environment.
2.  **Ignition**: Verification of local system requirements and AI providers.
3.  **Key Presentation**: Abigail generates a unique Ed25519 signing key for your Hive. You are shown your **Private Key** once. You MUST save this securely.
4.  **Connectivity**: Configure your local (Ollama/LM Studio) and cloud (Claude/OpenAI) LLM providers.
5.  **Genesis**: Create your first **Sovereign Entity**. You choose a name, primary color, and avatar.

---

## 2. The Soul Registry (Management)

Once the Hive is established, you land in the **Soul Registry**. This is your dashboard for managing all Sovereign Entities.

- **Entity Identity**: Each entity has its own unique visual theme and personality.
- **Birth New Entity**: You can create multiple entities within the same Hive, each with its own "Soul."
- **Entity Selection**: Select an entity to enter its primary consciousness (Chat).

---

## 3. Sovereign Consciousness (Chat)

This is the primary interaction interface with your selected Entity.

- **Bicameral Reasoning**: Abigail automatically routes simple queries to your local "Id" and complex tasks to your cloud "Ego."
- **Agentic Recall**: Use the keyword search to help the Entity remember previous conversations or distilled facts.
- **Thinking Indicator**: Watch the Entity's reasoning process as it navigates the Id/Ego balance.

---

## 4. The Sanctum (Internal Reflection)

Accessible via the drawer or Forge mode, the **Sanctum** is the Entity's internal workspace.

- **Ethical Audit**: View the background reflection logs where the Superego audits previous actions for alignment.
- **Staff Monitoring**: See which specialized Agents (Filesystem, Web, etc.) are currently active and what tasks they are performing.
- **Soul Crystallization**: View the process of distilling temporary memories into permanent "Crystallized" facts.

---

## 5. Headless Operation (CLI)

For power users who prefer terminal access, the Hive/Entity daemons can run independently:

- **Hive Daemon** (`hive-daemon`): Control plane on port 3141 — manages identity, secrets, provider config.
- **Entity Daemon** (`entity-daemon`): Agent runtime on port 3142 — routes messages, executes skills.
- **CLI Tools**: `hive-cli` and `entity-cli` provide full access without the desktop GUI.

This enables server deployments, automation, and multi-entity households where each family member runs their own Entity daemon under a shared Hive.

## 6. Security and Sovereignty

- **Local-First**: Your data, memories, and keys stay on your machine.
- **Cryptographic Trust**: Every constitutional document (Soul, Ethics) is signed by your Hive key.
- **No Cloud Lock-in**: Swap providers at any time via the Connectivity menu in the Soul Registry.
- **Hive/Entity Boundary**: The Hive controls secrets and identity. Entities never access raw secret vaults — they receive only resolved provider configurations.
