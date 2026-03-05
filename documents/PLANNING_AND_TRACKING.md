# Planning and Tracking

**Last updated:** 2026-03-01  
**Scope:** Day-to-day execution tracking for the GUI/Entity stabilization program through the beta target on **March 31, 2026**.

---

## Source of truth

Use these artifacts in order:

1. `documents/ARCHITECTURE_SKILL_TOPOLOGY_AND_FORGE.md`  
   Canonical topology + Forge contract (persistent topics, registry authority, Superego gates, watcher re-provision).
2. `documents/GUI_ENTITY_STABILITY_ROADMAP.md`  
   Sprint backlog, goals, and program gates.
3. `documents/tests/SPRINT_*_KICKOFF_CHECKLIST.md`  
   Sprint entry criteria and execution checklist.
4. `documents/tests/SPRINT_*_REPORT.md`  
   Sprint closure evidence and residual risks.
5. `documents/tests/VALIDATION_AND_GATE_REPORT.md`  
   Consolidated gate status.
6. `documents/RELEASE.md`  
   Release posture, release workflow, and release gating requirements.
7. `CHANGELOG.md`  
   User-facing versioned change history.

---

## Topology Planning Note

The previous dynamic-only skill model is deprecated. All planning and execution must assume:

- persistent startup provisioning from `skills/registry.toml`
- request/response topic topology per enabled skill
- out-of-band monitor layer on chat-topic (mentor, id, superego, memory observers)
- Forge outputs that are registry-backed and watcher-reprovisioned

---

## Status model

Use one status per tracked item:

- `Not Started`
- `In Progress`
- `Blocked`
- `Closed`

When `Blocked`, always include:

- Blocker description
- Owner
- Next unblock action
- Target unblock date

---

## Weekly operating cadence

Run this loop each week:

1. Review roadmap sprint items and update statuses.
2. Confirm gate state (command surface, chat parity, agent lifecycle, policy, release).
3. Update the active sprint report with pass/fail evidence and open risks.
4. Reconcile release readiness in `documents/RELEASE.md`.
5. Append user-visible changes to `CHANGELOG.md` when scope is complete.

---

## Tracker template

Copy this table into the active sprint checklist/report when needed:

| Item | Owner | Status | Evidence | Next action | Date |
|------|-------|--------|----------|-------------|------|
| Sx-01 | @owner | In Progress | link/path | short action | YYYY-MM-DD |
| Gate: Command Surface | @owner | Closed | link/path | n/a | YYYY-MM-DD |
| Risk: example risk | @owner | Blocked | link/path | mitigation step | YYYY-MM-DD |

---

## Exit criteria for beta readiness

Before a beta cut, confirm all are true:

1. Roadmap sprint commitments for stabilization are closed or explicitly deferred with rationale.
2. Program gates are green and documented in validation artifacts.
3. No open P0 defects in GUI chat/entity-agent stability scope.
4. Release workflow is run with correct versioning and documented outputs.
5. `CHANGELOG.md` and release notes reflect actual shipped behavior.
