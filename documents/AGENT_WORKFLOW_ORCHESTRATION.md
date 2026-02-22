# Agent Workflow Orchestration Policy

This document defines the default workflow policy for non-trivial implementation work in this repository.

## 1. Plan Node Default

- Enter plan mode for any non-trivial task (multi-step work, architectural decisions, uncertainty, coordination across files/modules, or production impact).
- If something goes sideways, stop and re-plan immediately.
- Use plan mode for verification steps, not only implementation.
- Write detailed specs up front to reduce ambiguity.

## 2. Subagent Strategy

- Use subagents liberally to keep the main context window clean.
- Offload research, exploration, and parallel analysis to subagents.
- For complex problems, use additional parallel subagent compute.
- Keep one focused task per subagent.

## 3. Self-Improvement Loop

- After any correction from the user, update `tasks/lessons.md` with the pattern.
- Write or refine rules that prevent the same mistake.
- Iterate these lessons until mistake rate drops.
- Review relevant lessons at session start.
- Capture successful patterns and what worked exceptionally well.

## 4. Verification Before Done

- Never mark a task complete without evidence it works.
- Diff behavior between `main` and current changes when relevant.
- Verify quality at a staff-engineer approval bar.
- Run tests, check logs, and demonstrate correctness.
- Run linters, formatters, type checkers, and relevant suites (including edge cases and error paths).

## 5. Pursue Pragmatic Elegance

- For non-trivial changes, pause and ask whether a more elegant approach exists.
- If a fix is hacky, re-implement using the elegant approach with current context.
- Skip this for simple, obvious fixes; avoid over-engineering.
- When elegance and simplicity conflict, prefer simplicity unless maintainability, readability, or performance gains are clearly significant.

## 6. Autonomous Bug Fixing

- For a bug report, proceed directly to diagnosis and fix.
- Use logs, errors, and failing tests to drive root-cause resolution.
- Minimize user context switching.
- Fix failing CI tests proactively.

## 7. Tool and Research Strategy

- Use available tools proactively and in parallel for uncertainty, research, exploration, and verification.
- Ground decisions in tool output or the live codebase rather than assumptions.
- Synthesize findings into short, actionable insights before implementation.
- Log key findings in `tasks/todo.md` or concise code comments when needed for traceability.

## 8. Risk Assessment and Safety

- In every non-trivial plan and before significant changes, call out risks: security, performance regression, breaking changes, data integrity, and backward compatibility.
- Prefer minimal-impact, reversible changes (small PR-sized units, flags when relevant).
- For user-facing or data-affecting changes, define rollback/migration strategy and test failure modes.
- Flag genuinely high-risk items with rationale and alternatives.

## 9. Communication Protocol

- Provide concise, high-signal updates at natural breakpoints (plan complete, major component done, verification passed).
- For complex updates, use this structure:
  - Status
  - Key Decisions and Rationale
  - Changes (high-level and targeted diffs)
  - Verification Results
  - Next Steps or Options
- When trade-offs are meaningful, provide one to two alternatives with pros/cons and a recommendation.
- Default to autonomous forward progress; escalate only for ambiguity, user-preference decisions, or high-risk concerns.

## 10. Task Management

1. Plan first: write plan to `tasks/todo.md` with checkable items.
2. Verify plan: check in before implementation.
3. Track progress: mark items complete as work advances.
4. Explain changes: provide high-level summary at each step.
5. Document results: add a review section to `tasks/todo.md`.
6. Capture lessons: update `tasks/lessons.md` after corrections.
7. Definition of done: mark complete only after verification passes, risks are mitigated, documentation is updated, and work meets staff-engineer review quality.

## 11. Core Principles

- Simplicity first: make each change as simple as possible.
- No laziness: solve root causes, avoid temporary fixes.
- Minimal impact: touch only necessary code and limit regression risk.
- Codebase consistency: follow existing style, patterns, and architecture unless a compelling documented reason exists.
