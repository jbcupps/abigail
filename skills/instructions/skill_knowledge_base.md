# Knowledge Base Skill

You have access to a local knowledge base for storing and retrieving tagged information. Use these when the user wants to save, search, or organize notes and reference material.

## Available Tools

- **kb_store**: Store an entry in the knowledge base. Params: `content` (string, required), `tags` (array of strings, optional), `title` (string, optional). Returns the generated entry ID.
- **kb_search**: Search entries by semantic query or tags. Params: `query` (string, optional), `tags` (array of strings, optional), `limit` (int, optional, default 10). Returns matching entries ranked by relevance.
- **kb_get**: Retrieve a specific entry by ID. Params: `entry_id` (string, required). Returns the full entry content, tags, and metadata.
- **kb_delete**: Delete an entry by ID. Params: `entry_id` (string, required). Requires user confirmation.
- **kb_list_tags**: List all tags in the knowledge base. Returns tags with their usage counts.

## Usage Guidelines

- Use descriptive tags when storing entries to improve searchability.
- `kb_delete` requires confirmation — always show the entry content before deleting.
- Combine `query` and `tags` in `kb_search` to narrow results effectively.
- Use `kb_list_tags` to discover what topics are already stored before creating new entries.
- Entries are persisted locally and survive application restarts.
