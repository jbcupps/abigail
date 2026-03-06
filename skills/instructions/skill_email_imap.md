# Email (IMAP/SMTP) Skill

You have access to email tools via the Email skill. Use these when the user asks about email, their inbox, or wants to send messages.

## Mailbox Setup

When the user provides mailbox / IMAP credentials, store each value using the `store_secret` tool from `builtin.hive_management`. Map the user-provided fields to these exact vault keys:

| User-provided field | Vault key |
|---------------------|-----------|
| Username / Email | `imap_user` |
| Password | `imap_password` |
| Hostname / Server | `imap_host` |
| Port | `imap_port` |
| Security (STARTTLS / IMPLICIT) | `imap_tls_mode` |
| SMTP Hostname / Server | `smtp_host` |
| SMTP Port | `smtp_port` |
| SMTP Username / Email | `smtp_user` |
| SMTP Password | `smtp_password` |
| SMTP Security (STARTTLS / IMPLICIT) | `smtp_tls_mode` |

Call `store_secret` once per field with `{ "key": "<vault_key>", "value": "<user_value>" }`.

`imap_user`, `imap_password`, and `imap_host` are required. `smtp_host`, `smtp_user`, and `smtp_password` are required for sending mail, but the skill can still initialize in IMAP-only mode without them. `imap_port` defaults to `993`, `smtp_port` defaults to `587`, `imap_tls_mode` defaults to `IMPLICIT`, and `smtp_tls_mode` defaults to `STARTTLS` unless port `465` implies implicit TLS. Only store port or TLS values when the user supplies non-default values.

After storing, confirm each key was saved and summarize the configuration while masking passwords.

## Available Tools

- **fetch_emails**: Fetch emails from INBOX. Params: `limit` (int, default 50), `unread_only` (bool, default true). Returns an array of email objects.
- **send_email**: Send an email. Params: `to` (array of recipients), `subject` (string), `body` (string). All three are required. Requires user confirmation before sending.
- **classify_importance**: Classify an email's importance. Params: `email_id` (string). Returns "low", "normal", or "high".

## Usage Guidelines

- When the user asks to check their email, use `fetch_emails` with a reasonable limit (10-20 for a quick check, more if asked).
- Summarize fetched emails concisely: sender, subject, and a brief preview.
- Always confirm with the user before calling `send_email`; it requires explicit confirmation.
- If email credentials are not configured, guide the user through the setup flow above.
- If SMTP fields are missing, treat the account as read-only and explain that sending is disabled until SMTP is configured.

## Error Handling

- If fetch fails with an authentication error, suggest the user check their IMAP credentials.
- If the IMAP connection times out, let the user know and suggest retrying.
