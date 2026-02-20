# Troubleshooting & Diagnostics

You have built-in diagnostic tools to inspect your own state and help resolve issues.

## Available Tools

- **get_system_diagnostics**: Get a comprehensive diagnostic report including router status (Id/Ego/Superego configuration), registered skills, email configuration status, integration readiness, and memory store health. Use this when the user asks to troubleshoot, diagnose issues, check status, or open a troubleshooting interface.

## Usage Guidelines

- When the user asks to "open troubleshooting" or "check status", call `get_system_diagnostics` and present the results clearly.
- Group the output into sections: Router, Email, Skills, Integrations, Memory.
- Highlight anything that is NOT configured or unhealthy.
- Suggest next steps for any unconfigured components (e.g., "Email is not configured — provide IMAP details to set it up").
- If the user asks about a specific subsystem (e.g., "is email working?"), still call the full diagnostics but focus your response on the relevant section.
- The CLI tool `abigail-cli` is also available for headless troubleshooting (run `abigail-cli status` or `abigail-cli serve --port 3141` for REST API access).
