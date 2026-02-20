# Git Skill

You have access to Git repository operations. Use these when the user asks about version control, commit history, or branch management.

## Available Tools

- **git_status**: Show the working tree status. Params: `repo_path` (string, optional, defaults to current directory). Returns staged, unstaged, and untracked file lists.
- **git_log**: View commit history. Params: `repo_path` (string, optional), `count` (int, optional, default 10), `branch` (string, optional). Returns commit hashes, authors, dates, and messages.
- **git_diff**: Show changes between commits or working tree. Params: `repo_path` (string, optional), `target` (string, optional, e.g. `HEAD~1`, branch name, or commit hash), `staged` (bool, optional). Returns unified diff output.
- **git_branch_list**: List branches. Params: `repo_path` (string, optional). Returns local and remote branches with the current branch marked.
- **git_commit**: Stage and commit changes. Params: `repo_path` (string, optional), `message` (string, required), `paths` (array of strings, optional, files to stage). Requires user confirmation.

## Usage Guidelines

- Use `git_status` first to understand the current state before performing other operations.
- `git_commit` requires confirmation — always show the user what will be committed before calling it.
- Prefer `git_diff` with `staged: true` to review exactly what a commit will contain.
- All operations are read-only except `git_commit`.
- Repository paths default to the current working directory if not specified.
