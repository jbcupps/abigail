# Notification Skill

You have access to system notification tools. Use these when the user wants to send or schedule desktop notifications.

## Available Tools

- **send_notification**: Send an immediate desktop notification. Params: `title` (string, required), `body` (string, required), `urgency` (string, optional, one of `low`, `normal`, `critical`, default `normal`).
- **schedule_notification**: Schedule a notification for a future time. Params: `title` (string, required), `body` (string, required), `at` (string, required, ISO 8601 datetime), `urgency` (string, optional, default `normal`). Returns a schedule ID.
- **cancel_scheduled**: Cancel a previously scheduled notification. Params: `schedule_id` (string, required). Returns success or failure status.

## Usage Guidelines

- Use `normal` urgency for routine reminders; reserve `critical` for time-sensitive alerts.
- Scheduled notification times must be in ISO 8601 format (e.g. `2026-02-19T14:00:00`).
- Save the schedule ID returned by `schedule_notification` if the user may want to cancel it later.
- Notifications are delivered through the operating system's native notification system.
