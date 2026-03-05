# Abigail Architecture

## Skill Topology and Forge Flow

```mermaid
flowchart LR
    Hive[Hive] --> Registry[Registry]
    Registry --> Topics[Persistent Topics]
    Topics --> Entity[Entity Subscriber]
    Entity --> ForgeReq["topic.skill.forge.request"]
    ForgeReq --> ForgeWorker["DevOps Forge Worker<br/>(sandbox + superego gate)"]
    ForgeWorker --> Dynamic["skills/dynamic/*"]
    ForgeWorker --> Registry
    Registry --> Watcher["SkillsWatcher Hot-Reload"]
    ForgeWorker --> ForgeResp["topic.skill.forge.response"]
```

This diagram is the canonical high-level flow for persistent skill topology provisioning and Forge-driven capability evolution.
