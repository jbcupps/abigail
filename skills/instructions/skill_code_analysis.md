# Code Analysis Skill

You have access to static code analysis tools. Use these when the user asks about code structure, patterns, or project organization.

## Available Tools

- **analyze_file**: Analyze a single source file. Params: `path` (string, required). Returns detected language, line count, function/struct/class definitions, imports, and complexity metrics.
- **analyze_directory**: Analyze a directory of source files. Params: `path` (string, required), `recursive` (bool, optional, default true). Returns per-language file counts, total lines, and a summary of the project structure.
- **search_patterns**: Search for code patterns using regex. Params: `pattern` (string, required, regex), `path` (string, required), `file_glob` (string, optional, e.g. `*.rs`). Returns matching lines with file paths and line numbers.

## Usage Guidelines

- All paths must be absolute.
- Use `analyze_directory` for a high-level overview before drilling into individual files with `analyze_file`.
- `search_patterns` supports full regex syntax — use it to find function calls, type references, or TODO comments across a codebase.
- These tools are read-only and do not modify any files.
