# Email (IMAP/SMTP) Skill

You have access to email tools via the Proton Mail skill. Use these when the user asks about email, their inbox, or wants to send messages.

## Mailbox Setup

When the user provides mailbox / IMAP credentials, store each value using the `store_secret` tool from `builtin.hive_management`. Map the user-provided fields to these exact vault keys:

| User-provided field | Vault key       |
|---------------------|-----------------|
| Username / Email    | `imap_user`     |
| Password            | `imap_password` |
| Hostname / Server   | `imap_host`     |
| Port                | `imap_port`     |
| Security (STARTTLS / IMPLICIT) | `imap_tls_mode` |
| SMTP Hostname / Server | `smtp_host`     |
| SMTP Port              | `smtp_port`     |

Call `store_secret` once per field with `{ "key": "<vault_key>", "value": "<user_value>" }`.

Only `imap_user` and `imap_password` are required. The others have sensible defaults (host: `mail.proton.me`, port: `993`, TLS: `IMPLICIT`, smtp_host: `smtp.proton.me`, smtp_port: `587`) and only need to be stored when the user supplies non-default values.

After storing, confirm each key was saved and summarize the configuration (mask the password).

## Available Tools

- **fetch_emails**: Fetch emails from INBOX. Params: `limit` (int, default 50), `unread_only` (bool, default true). Returns an array of email objects.
- **send_email**: Send an email. Params: `to` (array of recipients), `subject` (string), `body` (string). All three are required. Requires user confirmation before sending.
- **classify_importance**: Classify an email's importance. Params: `email_id` (string). Returns "low", "normal", or "high".
- **create_filter**: Create a filter rule. Params: `name` (string), `criteria` (object). Requires user confirmation.

## Usage Guidelines

- When the user asks to check their email, use `fetch_emails` with a reasonable limit (10-20 for a quick check, more if asked).
- Summarize fetched emails concisely: sender, subject, and a brief preview.
- Always confirm with the user before calling `send_email` — it requires explicit confirmation.
- If email credentials are not configured, guide the user through the setup flow above.

## Error Handling

- If fetch fails with an authentication error, suggest the user check their IMAP credentials.
- If the IMAP connection times out, let the user know and suggest retrying.
