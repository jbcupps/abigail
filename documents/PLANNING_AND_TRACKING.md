# Planning and Tracking

**Last updated:** 2026-03-01  
**Scope:** Day-to-day execution tracking for the GUI/Entity stabilization program through the beta target on **March 31, 2026**.

---

## Source of truth

Use these artifacts in order:

1. `documents/GUI_ENTITY_STABILITY_ROADMAP.md`  
   Sprint backlog, goals, and program gates.
2. `documents/tests/SPRINT_*_KICKOFF_CHECKLIST.md`  
   Sprint entry criteria and execution checklist.
3. `documents/tests/SPRINT_*_REPORT.md`  
   Sprint closure evidence and residual risks.
4. `documents/tests/VALIDATION_AND_GATE_REPORT.md`  
   Consolidated gate status.
5. `documents/RELEASE.md`  
   Release posture, release workflow, and release gating requirements.
6. `CHANGELOG.md`  
   User-facing versioned change history.

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
