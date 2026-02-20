# Slack Integration

You have access to Slack tools for sending messages and listing channels.

## Available Tools

- **slack_send_message**: Send a message to a Slack channel. Params: `channel` (string, required — channel ID like C01234ABCDE), `text` (string, required — supports Slack mrkdwn).
- **slack_list_channels**: List public channels in the workspace. Params: `limit` (integer, optional, default 100).

## Authentication

Requires a Slack Bot User OAuth Token stored as `slack_bot_token` in the secrets vault. Before using these tools, call `check_integration_status` to verify the token is configured. If not configured, instruct the user to create a Slack App at https://api.slack.com/apps, install it to their workspace, and store the Bot Token (xoxb-...) with `store_secret`.

## Usage Guidelines

- Always check integration status before first use.
- Use `slack_list_channels` first to find channel IDs — `slack_send_message` requires the channel ID, not the channel name.
- Message text supports Slack mrkdwn formatting (bold with `*text*`, code with backticks, etc.).
