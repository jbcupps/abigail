# Abigail User Experience (UX) Guide

This document outlines the user journey within Abigail, from initial setup to ongoing management of Entities.

---

## 1. The Hive Setup (Initial Launch)

When you first launch Abigail, you are entering **The Hive** - the local environment where your Entities will live.

### Birth Sequence
1. **Darkness**: The initialization phase where the Hive prepares its local environment.
2. **Ignition**: Verification of local system requirements and AI providers.
3. **Key Presentation**: Abigail generates a unique Ed25519 signing key for your Hive. You are shown your **Private Key** once. You MUST save this securely.
4. **Connectivity**: Configure your local (Ollama/LM Studio) and cloud (Claude/OpenAI) LLM providers.
5. **Genesis**: Create your first **Entity**. You choose a name, primary color, and avatar.

---

## 2. The Soul Registry (Management)

On every launch, Abigail enters the **Soul Registry** first. This is your dashboard for managing all Entities.

- **Entity Identity**: Each entity has its own unique visual theme and personality.
- **Birth New Entity**: You can create multiple entities within the same Hive, each with its own "Soul."
- **Entity Selection**: Explicitly select an entity each session to enter Chat.
- **Lifecycle Controls**: Archive is the primary safe action, with delete as a secondary destructive action.
- **Agent Runtime Limit**: Up to 3 agents can run concurrently in the current branch.

---

## 3. Entity Consciousness (Chat)

This is the primary interaction interface with your selected Entity.

- **Bicameral Reasoning**: Abigail routes simple queries to local "Id" and complex tasks to cloud "Ego."
- **Agentic Recall**: Use keyword search to help the Entity recall previous conversations or distilled facts.
- **Thinking Indicator**: Watch the Entity's reasoning process as it navigates the Id/Ego balance.

---

## 4. The Sanctum (Internal Reflection)

Accessible via the drawer or Forge mode, the **Sanctum** is the Entity's internal workspace.

- **Ethical Audit**: View background reflection logs where the Superego audits previous actions for alignment.
- **Staff Monitoring**: See which specialized Agents (filesystem, web, etc.) are active and what tasks they are performing.
- **Soul Crystallization**: View the process of distilling temporary memories into permanent facts.

---

## 5. Security and Sovereignty

- **Local-First**: Your data, memories, and keys stay on your machine.
- **Cryptographic Trust**: Every constitutional document (Soul, Ethics) is signed by your Hive key.
- **No Cloud Lock-in**: Swap providers at any time via the Connectivity menu in the Soul Registry.
- **Isolation by Default**: Memory is isolated per entity by default.
- **Config Scope**: Provider configuration is Hive-global by default, with per-entity overrides for task optimization.
