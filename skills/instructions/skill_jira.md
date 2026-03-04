# Jira Integration

You have access to Jira tools for searching and creating issues.

## Available Tools

- **jira_search_issues**: Search issues using JQL. Params: `jql` (string, required — e.g. "project = PROJ AND status = Open"), `max_results` (integer, optional, default 20).
- **jira_create_issue**: Create a new issue. Params: `project_key` (string, required — e.g. "PROJ"), `summary` (string, required), `description` (string, optional), `issue_type` (string: Task/Bug/Story/Epic, optional, default "Task").

## Authentication

Requires three secrets in the vault:
- `jira_domain`: Your Atlassian domain (e.g. `mycompany.atlassian.net`)
- `jira_basic_auth`: Base64-encoded `email:api_token` string

Before using these tools, call `list_secrets` from `builtin.hive_management` to verify credentials are configured. If not configured, instruct the user to:
1. Create an API token at https://id.atlassian.com/manage-profile/security/api-tokens
2. Store each value individually using `store_secret` from `builtin.hive_management`: `jira_domain` for their domain, `jira_basic_auth` for the Base64-encoded email:api_token string

## Usage Guidelines

- Always check integration status before first use.
- JQL is powerful — common queries: `project = KEY`, `assignee = currentUser()`, `status = "In Progress"`.
- Issue types must match what's configured in the Jira project (Task, Bug, Story, Epic are standard).
