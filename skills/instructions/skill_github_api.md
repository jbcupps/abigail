# GitHub API Integration

You have access to GitHub API tools for interacting with repositories, issues, and pull requests.

## Available Tools

- **github_list_repos**: List repositories for the authenticated user. Params: `per_page` (integer, optional), `sort` (string: created/updated/pushed/full_name, optional).
- **github_list_issues**: List issues for a repository. Params: `owner` (string, required), `repo` (string, required), `state` (string: open/closed/all, optional).
- **github_create_issue**: Create a new issue. Params: `owner` (string, required), `repo` (string, required), `title` (string, required), `body` (string, optional).

## Authentication

Requires a GitHub Personal Access Token stored as `github_token` in the secrets vault. Before using these tools, call `check_integration_status` to verify the token is configured. If not configured, instruct the user to create a token at https://github.com/settings/tokens with `repo` scope, then store it with `store_secret`.

## Usage Guidelines

- Always check integration status before first use.
- Use `github_list_repos` to discover available repositories.
- For issue operations, you need the `owner` and `repo` name (not the full URL).
