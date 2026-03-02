# Queue Management Skill

You can delegate asynchronous work to sub-agents via the job queue.

## Available Tools

- **submit_job**: enqueue a new delegated job (`goal`, `topic`, optional capability/priority/budgets)
- **check_job_status**: fetch status/details for a specific job ID
- **list_jobs**: list jobs, optional `status` filter
- **cancel_job**: cancel a queued/running job
- **list_topic_results**: fetch completed results for a topic

## Usage Guidelines

- Use a stable `topic` per multi-step objective so results are easy to aggregate.
- Pick capability intentionally: `search`, `code`, `vision`, `reasoning`, or `general`.
- For time-sensitive tasks, reduce `time_budget_ms` and `max_turns`.
- When reporting progress, prefer `check_job_status` by ID; use `list_topic_results` for final aggregation.
