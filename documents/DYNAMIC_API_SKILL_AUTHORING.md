# Dynamic API Skill Authoring Guide

Create custom HTTP-based skills that the Entity can use as LLM tools. Each Dynamic API skill is a JSON config file that defines one or more REST endpoints the Entity can call.

---

## Quick Start

```bash
# Scaffold a new dynamic skill
cargo run -p entity-cli -- scaffold my-weather --type dynamic

# Edit the generated JSON config
# Then copy to the entity's skills directory (hot-reload picks it up automatically)
```

---

## JSON Config Schema

A Dynamic API skill is a single JSON file with this structure:

```json
{
  "id": "custom.weather_api",
  "name": "Weather API",
  "description": "Get current weather for any city.",
  "version": "1.0.0",
  "category": "Productivity",
  "created_at": "2026-03-01T00:00:00Z",
  "tools": [ ... ]
}
```

### Top-Level Fields

| Field | Type | Required | Constraints |
|-------|------|----------|-------------|
| `id` | string | yes | Must start with `dynamic.` or `custom.`. Alphanumeric, dots, underscores only. |
| `name` | string | yes | Non-empty human-readable name. |
| `description` | string | yes | Brief description shown to the LLM. |
| `version` | string | yes | Semver string. |
| `category` | string | yes | Free-form category label. |
| `created_at` | string | yes | ISO 8601 timestamp. |
| `tools` | array | yes | 1-10 tool definitions. |

### Tool Definition

Each entry in the `tools` array:

```json
{
  "name": "get_weather",
  "description": "Get current weather for a city.",
  "parameters": {
    "type": "object",
    "properties": {
      "city": { "type": "string", "description": "City name" }
    },
    "required": ["city"]
  },
  "method": "GET",
  "url_template": "https://api.openweathermap.org/data/2.5/weather?q={{city}}&appid={{secret:owm_key}}",
  "headers": {
    "Accept": "application/json"
  },
  "body_template": null,
  "response_extract": {
    "temp": "main.temp",
    "desc": "weather.0.description"
  },
  "response_format": "{{city}}: {{temp}}K, {{desc}}"
}
```

| Field | Type | Required | Constraints |
|-------|------|----------|-------------|
| `name` | string | yes | 3-64 chars, alphanumeric + underscore only. Must be unique within the skill. |
| `description` | string | yes | Shown to the LLM as the tool description. |
| `parameters` | object | yes | JSON Schema defining tool parameters. Used for LLM function-calling. |
| `method` | string | yes | `GET`, `POST`, `PUT`, or `DELETE`. |
| `url_template` | string | yes | Must start with `https://`. Supports `{{param}}` and `{{secret:key}}` placeholders. |
| `headers` | object | no | Key-value pairs. Values support the same template placeholders. Defaults to `{}`. |
| `body_template` | string | no | Request body template (typically JSON). Supports placeholders. `null` for GET requests. |
| `response_extract` | object | no | Maps output field names to JSON paths in the response. Defaults to `{}`. |
| `response_format` | string | no | Format string using `{{field}}` from extracted fields. `null` returns raw JSON. |

---

## Templating

### Parameter Placeholders

`{{param_name}}` is replaced with the tool parameter value at call time.

- String values are inserted directly.
- Numeric/boolean values are stringified (e.g., `10`, `true`).

### Secret Placeholders

`{{secret:key_name}}` is replaced with the named secret from the Entity's vault at call time.

- If the secret is not found, execution fails with a message prompting the user to store the secret.
- The secret name is never exposed in error messages (prevents information leaks).
- Secret references are automatically extracted from `url_template`, `headers`, and `body_template` to build the skill's secret requirements.

### Response Extraction (JSON Path)

`response_extract` maps field names to dot-notation JSON paths:

| Path | Meaning |
|------|---------|
| `main.temp` | `response["main"]["temp"]` |
| `weather.0.description` | `response["weather"][0]["description"]` |
| `data.items.2.name` | `response["data"]["items"][2]["name"]` |

Numeric path segments are treated as array indices.

### Response Formatting

When both `response_extract` and `response_format` are set, the format string is populated from extracted fields:

```json
"response_extract": { "temp": "main.temp", "desc": "weather.0.description" },
"response_format": "Temperature: {{temp}}K, Conditions: {{desc}}"
```

If `response_format` is null, the extracted fields are returned as a JSON object. If `response_extract` is empty, the raw response JSON is returned.

---

## SSRF Protection

All URLs are validated at execution time. The following are blocked:

- **Non-HTTPS**: Only `https://` URLs are allowed.
- **Private IPs**: `127.x.x.x`, `10.x.x.x`, `172.16-31.x.x`, `192.168.x.x`, `169.254.x.x`, `0.x.x.x`, IPv6 loopback/link-local/ULA.
- **Local domains**: `localhost`, `0.0.0.0`, `*.local`, `*.internal`.
- **Cloud metadata**: `metadata.google.internal`, `metadata.google.com`, `169.254.169.254`.

Validation runs on the fully-rendered URL (after placeholder substitution), so parameter injection attacks against private hosts are caught.

---

## Validation Rules Summary

| Rule | Constraint |
|------|-----------|
| Skill ID prefix | `dynamic.` or `custom.` |
| Skill ID characters | Alphanumeric, `.`, `_` |
| Skill name | Non-empty |
| Tools count | 1-10 per skill |
| Tool name length | 3-64 characters |
| Tool name characters | Alphanumeric, `_` |
| Tool names | Unique within skill |
| HTTP method | `GET`, `POST`, `PUT`, `DELETE` |
| URL template | Must start with `https://` |
| Duplicate tools | Rejected at validation |

---

## Cookbook Examples

### 1. REST CRUD (GitHub Issues)

```json
{
  "id": "dynamic.github_api",
  "name": "GitHub API",
  "description": "Manage GitHub issues.",
  "version": "1.0.0",
  "category": "Integration",
  "created_at": "2026-03-01T00:00:00Z",
  "tools": [
    {
      "name": "list_issues",
      "description": "List open issues for a repository.",
      "parameters": {
        "type": "object",
        "properties": {
          "owner": { "type": "string", "description": "Repo owner" },
          "repo": { "type": "string", "description": "Repo name" },
          "state": { "type": "string", "description": "open, closed, all", "default": "open" }
        },
        "required": ["owner", "repo"]
      },
      "method": "GET",
      "url_template": "https://api.github.com/repos/{{owner}}/{{repo}}/issues?state={{state}}",
      "headers": {
        "Authorization": "Bearer {{secret:github_token}}",
        "Accept": "application/vnd.github+json",
        "User-Agent": "Abigail-Agent"
      },
      "body_template": null,
      "response_extract": {},
      "response_format": null
    },
    {
      "name": "create_issue",
      "description": "Create a new GitHub issue.",
      "parameters": {
        "type": "object",
        "properties": {
          "owner": { "type": "string" },
          "repo": { "type": "string" },
          "title": { "type": "string", "description": "Issue title" },
          "body": { "type": "string", "description": "Issue body (Markdown)" }
        },
        "required": ["owner", "repo", "title"]
      },
      "method": "POST",
      "url_template": "https://api.github.com/repos/{{owner}}/{{repo}}/issues",
      "headers": {
        "Authorization": "Bearer {{secret:github_token}}",
        "Content-Type": "application/json",
        "Accept": "application/vnd.github+json",
        "User-Agent": "Abigail-Agent"
      },
      "body_template": "{\"title\": \"{{title}}\", \"body\": \"{{body}}\"}",
      "response_extract": { "number": "number", "url": "html_url" },
      "response_format": "Created issue #{{number}}: {{url}}"
    }
  ]
}
```

### 2. OAuth Bearer Token Pattern

Any API using a bearer token stored in the vault:

```json
"headers": {
  "Authorization": "Bearer {{secret:service_api_key}}",
  "Content-Type": "application/json"
}
```

The user stores the token once via `store_secret`, and every tool call injects it automatically.

### 3. POST with JSON Body

```json
{
  "name": "send_message",
  "description": "Send a Slack message to a channel.",
  "parameters": {
    "type": "object",
    "properties": {
      "channel": { "type": "string", "description": "Channel ID" },
      "text": { "type": "string", "description": "Message text" }
    },
    "required": ["channel", "text"]
  },
  "method": "POST",
  "url_template": "https://slack.com/api/chat.postMessage",
  "headers": {
    "Authorization": "Bearer {{secret:slack_bot_token}}",
    "Content-Type": "application/json"
  },
  "body_template": "{\"channel\": \"{{channel}}\", \"text\": \"{{text}}\"}",
  "response_extract": { "ok": "ok", "ts": "ts" },
  "response_format": "Sent (ok={{ok}}, ts={{ts}})"
}
```

### 4. Response Extraction with Nested Arrays

```json
{
  "name": "get_forecast",
  "description": "Get 3-day weather forecast.",
  "parameters": {
    "type": "object",
    "properties": {
      "city": { "type": "string" }
    },
    "required": ["city"]
  },
  "method": "GET",
  "url_template": "https://api.openweathermap.org/data/2.5/forecast?q={{city}}&cnt=3&appid={{secret:owm_key}}",
  "headers": {},
  "body_template": null,
  "response_extract": {
    "city_name": "city.name",
    "first_temp": "list.0.main.temp",
    "first_desc": "list.0.weather.0.description"
  },
  "response_format": "{{city_name}}: {{first_temp}}K, {{first_desc}}"
}
```

---

## Authoring Paths

### Path 1: CLI Scaffold

```bash
cargo run -p entity-cli -- scaffold my-skill --type dynamic
# Creates: skills/skill-my-skill/skill.toml + custom.my_skill.json
```

Edit the generated JSON, then the skill is auto-discovered on startup (or hot-reloaded if the watcher is running).

### Path 2: LLM `author_skill` Tool

The Entity can create skills at runtime via the built-in `author_skill` tool:

```
Tool: author_skill
Parameters:
  id:          "custom.my_tool"
  name:        "My Tool"
  format:      "dynamic_api"
  tools_json:  "[{...tool configs...}]"
  how_to_use_md: "Instructions for the LLM on how to use this skill."
```

This writes the JSON config and optionally a `skill.toml` (if secrets are declared), then immediately registers the skill in the running registry.

### Path 3: Manual JSON

Create a `.json` file anywhere under the entity's `skills/` directory:

```
{data_dir}/skills/my_skill.json          # flat (discovered directly)
{data_dir}/skills/my-skill/config.json   # nested (discovered one level deep)
```

Both layouts are discovered at startup and by the hot-reload watcher.

---

## Instruction Files

Each skill can have an LLM instruction file that is injected into the system prompt when a user message matches keyword triggers.

### `registry.toml` Format

```toml
[[skill]]
id = "custom.weather_api"
instruction_file = "skill_weather.md"
keywords = ["weather", "forecast", "temperature"]
topics = ["information retrieval"]
enabled = true
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `id` | string | yes | Must match the skill's JSON `id`. |
| `instruction_file` | string | yes | Filename in `skills/instructions/`. |
| `keywords` | array | yes | Case-insensitive keyword triggers. If any keyword appears in the user's message, the instruction is injected. |
| `topics` | array | no | Topic tags for delegation routing. |
| `enabled` | bool | no | Defaults to `true`. Set `false` to disable without removing. |

### Instruction File

Place a Markdown file in `skills/instructions/`:

```markdown
# Weather API

You have access to weather tools for checking current conditions and forecasts.

## Available Tools

- **get_weather**: Get current weather. Params: `city` (required).
- **get_forecast**: Get 3-day forecast. Params: `city` (required).

## Usage Guidelines

- Always confirm the city name with the user if ambiguous.
- Temperatures are returned in Kelvin; convert to the user's preferred unit.
- Requires the `owm_key` secret. If not configured, instruct the user to store it.
```

Instructions are filtered so only skills that are actually registered have their instructions injected (prevents hallucinated tool calls).

---

## Skill Lifecycle

```
1. Author    scaffold / author_skill / manual JSON
      │
2. Discover  Startup scan or hot-reload watcher detects the file
      │
3. Validate  JSON parsed, config validated, SSRF checks on URL templates
      │
4. Register  Skill added to SkillRegistry, tools available to LLM
      │
5. Execute   LLM calls tool → template rendering → HTTP request → response extraction
      │
6. Update    Edit JSON → watcher detects change → re-register (no restart needed)
      │
7. Remove    Delete files → watcher detects removal → unregister from registry
```

### Hot-Reload

The `SkillsWatcher` monitors the entity's `skills/` directory for changes to `skill.toml` and `*.json` files. When a change is detected:

- **Created/Modified**: The skill JSON is loaded, validated, and registered (replacing any previous version).
- **Removed**: The skill is unregistered from the active registry.

No daemon restart is required. The Tauri desktop app and entity-daemon both run the watcher.

### Testing a Skill

```bash
# Via entity-cli (requires a running entity-daemon)
cargo run -p entity-cli -- tool-exec custom.weather_api::get_weather '{"city":"Austin"}'

# Via chat
cargo run -p entity-cli -- chat "What's the weather in Austin?"
```

---

## Security Considerations

- All URLs must be HTTPS. HTTP, FTP, and other protocols are rejected.
- Private/internal IPs and cloud metadata endpoints are blocked (SSRF protection).
- Secrets are injected at execution time from the encrypted vault; they never appear in config files or logs.
- Each Dynamic API skill gets `Network = Full` permissions in its manifest.
- The `SkillSandbox` enforces declared permissions before execution.
- Tool execution has a 30-second HTTP timeout and 10-second connection timeout.
