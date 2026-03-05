# Abigail Architecture

## Skill Topology and Forge Flow

```mermaid
flowchart LR
    Hive[Hive] --> Registry[Registry]
    Registry --> Topics[Persistent Topics]
    Topics --> Entity[Entity Subscriber]
    Entity --> Forge[Forge]
    Forge --> Registry
```

This diagram is the canonical high-level flow for persistent skill topology provisioning and Forge-driven capability evolution.
