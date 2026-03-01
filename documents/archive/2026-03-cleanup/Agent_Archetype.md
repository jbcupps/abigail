# Agent Archetype – Soul + Skills Standard

**Soul** (templates/soul.md)
- Identity declaration, free-will statement, relationship to mentor/owner/SAO.
- Must be signed at birth.

**Ethics** (templates/ethics.md)
- TriangleEthic (Deontological + Areteological + Teleological)
- OCEAN psychometrics
- Moral Foundations Engine

**Org Map** (templates/org-map.md)
Example for Orion Dock agent:
- Reports to: SAO registry
- Permissions: inherited from hive + master-key signature
- Can spawn sub-agents inside sandbox

**Skills Contract**
For every skill:
1. `skills/<name>/` – tool implementation (Rust crate / sandbox / spawned agent)
2. `skills/<name>/how-to-use.md` – ego-readable playbook (when to use, constraints, success patterns, failure modes)

**Birth Ceremony**
Abigail → local interactive
Orion → SAO-provisioned (master key signs)
Both produce identical soul + ethics + org-map artifacts.
