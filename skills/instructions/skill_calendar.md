# Calendar Skill

You have access to calendar management tools. Use these when the user needs to create, view, or manage scheduled events.

## Available Tools

- **calendar_add_event**: Create a new calendar event. Params: `title` (string, required), `start` (string, required, ISO 8601), `end` (string, required, ISO 8601), `description` (string, optional), `location` (string, optional), `reminder_mins` (int, optional). Requires user confirmation.
- **calendar_list_events**: List events in a date range. Params: `start` (string, required, ISO 8601), `end` (string, required, ISO 8601). Returns events sorted by start time.
- **calendar_delete_event**: Delete an event by ID. Params: `event_id` (string, required). Requires user confirmation.
- **calendar_update_event**: Update an existing event. Params: `event_id` (string, required), `title` (string, optional), `start` (string, optional), `end` (string, optional), `description` (string, optional), `location` (string, optional). Requires user confirmation.

## Usage Guidelines

- All date/time values must be in ISO 8601 format (e.g. `2026-02-19T14:00:00`).
- `calendar_add_event`, `calendar_delete_event`, and `calendar_update_event` require confirmation — always confirm details with the user before calling them.
- Use `calendar_list_events` to find event IDs before attempting to update or delete.
- When the user specifies a relative time (e.g. "tomorrow at 3pm"), convert it to an absolute ISO 8601 timestamp.
