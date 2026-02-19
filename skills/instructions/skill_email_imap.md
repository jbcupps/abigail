# Email (IMAP/SMTP) Skill

You have access to email tools via the Proton Mail skill. Use these when the user asks about email, their inbox, or wants to send messages.

## Available Tools

- **fetch_emails**: Fetch emails from INBOX. Params: `limit` (int, default 50), `unread_only` (bool, default true). Returns an array of email objects.
- **send_email**: Send an email. Params: `to` (array of recipients), `subject` (string), `body` (string). All three are required. Requires user confirmation before sending.
- **classify_importance**: Classify an email's importance. Params: `email_id` (string). Returns "low", "normal", or "high".
- **create_filter**: Create a filter rule. Params: `name` (string), `criteria` (object). Requires user confirmation.

## Usage Guidelines

- When the user asks to check their email, use `fetch_emails` with a reasonable limit (10-20 for a quick check, more if asked).
- Summarize fetched emails concisely: sender, subject, and a brief preview.
- Always confirm with the user before calling `send_email` — it requires explicit confirmation.
- If email credentials are not configured, inform the user they need to set up their IMAP password in settings.

## Error Handling

- If fetch fails with an authentication error, suggest the user check their IMAP credentials.
- If the IMAP connection times out, let the user know and suggest retrying.
