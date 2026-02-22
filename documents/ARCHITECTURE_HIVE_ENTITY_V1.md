# Hive/Entity Architecture V1 (Canonical)

Date: 2026-02-22  
Branch: `dev-split`

## Scope

This document is the canonical architecture baseline for the clean restart.
Legacy Tauri/UI/harness flows are not authoritative for V1 planning.

## Core Model

- Boundary: `Hive` and `Entity` run as separate processes.
- Topology:
  - `hive-daemon` (control plane)
  - `hive-cli` (client to hive-daemon)
  - `entity-daemon` (runtime plane)
  - `entity-cli` (client to entity-daemon, plus one-shot mode)
- API contract: versioned endpoints under `/v1/*`.

## Ownership

- Hive owns root authority and policy source-of-truth.
- Hive can own core provider keys.
- Entity may add task/provider keys with mentor-assisted workflows.
- Entity applies local policy and is periodically audited by Hive.

## State and Storage

- Entity runtime state and memory remain entity-local.
- Hive stores registry, policy, and audit metadata.
- Storage model is hybrid:
  - managed root
  - optional external mounts
- Current logging posture: local logs first; Hive ingest deferred.

## Runtime Limits

- Initial concurrent running-agent cap: `3`.
- Dynamic cap logic (hardware/usage aware) is a planned follow-up.

## Lifecycle

- Hive orchestrates entity lifecycle (start/stop/health).
- Initial CLI command priorities:
  - Hive: `status`, `entity list/start/stop`, `logs`
  - Entity: `status`, `run`, `chat`, `logs`

## Security

- Target control-channel auth: mTLS for local CLI/daemon communications.
- Current skeleton does not fully enforce mTLS yet; this is an explicit next implementation step.

## Cutover Policy

- Clean restart mode (no migration compatibility required for this phase).
- Legacy UI/harness removal gate:
  - all 4 new binaries compile
  - smoke checks pass for status/lifecycle/run/chat/logs flows
