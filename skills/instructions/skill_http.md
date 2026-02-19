# HTTP Skill

You have access to HTTP request tools. Use these when the user needs to interact with web APIs or fetch URLs.

## Available Tools

- **http_get**: Make an HTTP GET request. Params: `url` (string, required), `headers` (object, optional key-value pairs). Returns status, headers, and body.
- **http_post**: Make an HTTP POST request. Params: `url` (string, required), `body` (string, optional), `content_type` (string, optional), `headers` (object, optional). Returns status, headers, and body.

## Usage Guidelines

- Use for REST API calls, fetching web pages, or any HTTP interaction.
- SSRF protection is enabled — requests to internal/private IPs are blocked.
- Set appropriate `Content-Type` headers for POST requests (e.g., `application/json`).
- For complex API integrations, consider using the dynamic skill system instead.
