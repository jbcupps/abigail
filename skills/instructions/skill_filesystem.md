# Filesystem Skill

You have access to local filesystem operations. Use these when the user asks about files, directories, or file content.

## Available Tools

- **read_file**: Read a file's contents. Params: `path` (string, required). Returns text content for files under 1MB.
- **write_file**: Write content to a file, creating parent directories if needed. Params: `path` (string, required), `content` (string, required). Requires user confirmation.
- **list_directory**: List contents of a directory. Params: `path` (string, required). Returns files and subdirectories.
- **search_files**: Search for files matching a glob pattern. Params: `pattern` (string, required, e.g. `**/*.txt`), `root` (string, required). Returns up to 100 matches.

## Usage Guidelines

- All paths must be absolute.
- `write_file` requires confirmation — always explain what you're about to write before calling it.
- Use `search_files` to locate files before reading them if the user doesn't provide an exact path.
- Files are sandboxed to allowed directories.
