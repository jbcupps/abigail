# Clipboard Skill

You have access to system clipboard operations. Use these when the user wants to read from or write to the clipboard.

## Available Tools

- **clipboard_read**: Read the current clipboard contents. Returns the text content of the system clipboard.
- **clipboard_write**: Write text to the system clipboard. Params: `text` (string, required). Replaces the current clipboard contents. Requires user confirmation.

## Usage Guidelines

- `clipboard_write` requires confirmation — always tell the user what will be placed on the clipboard before calling it.
- `clipboard_read` returns only text content; binary or image clipboard data is not supported.
- Use clipboard tools to help the user transfer data between applications when file-based approaches are not practical.
