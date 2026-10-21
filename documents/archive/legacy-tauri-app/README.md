# Archive: Retired Single-Agent App Program (2026-02 to 2026-03)

These plans, reviews, and sprint reports were written against the original single-agent desktop app (`abigail-app`). That app is retired and is no longer how Abigail ships. It now lives in [`legacy/tauri-app/`](../../../legacy/README.md).

They are kept for history only. Paths such as `tauri-app/...` in these files refer to what is now `legacy/tauri-app/...`, and commands such as `cargo check -p abigail-app` no longer apply to the root workspace.

For current behavior, read [`README.md`](../../../README.md), [`docs/architecture.md`](../../../docs/architecture.md), and [`documents/tests/TEST_PROGRAM_INDEX.md`](../../tests/TEST_PROGRAM_INDEX.md).

| File | What it was |
|------|-------------|
| `PLANNING_AND_TRACKING.md` | Day-to-day tracker for the GUI/Entity stabilization program |
| `GUI_ENTITY_CODE_REVIEW_REPORT.md` | Code review of the single-app GUI/Entity coupling |
| `GUI_ENTITY_STABILITY_ROADMAP.md` | Sprint backlog for that program |
| `SPRINT_*_KICKOFF_CHECKLIST.md`, `SPRINT_*_REPORT.md` | Sprint entry checklists and closure reports |
| `VALIDATION_AND_GATE_REPORT.md` | Consolidated gate evidence for those sprints |
| `LIVE_UAT_STATUS_REPORT.md` | Live UAT status for the single-app build |
| `BROWSER_HARNESS_QA_PROTOCOL.md` | QA protocol for the single app's in-browser Tauri harness |
