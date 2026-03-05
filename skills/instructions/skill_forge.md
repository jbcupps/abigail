# Skill Forge

Use `builtin.skill_factory` when the user asks to create, update, or remove a capability that should persist beyond a single message.

## When To Use

- The request is repeatable or likely to be reused.
- The user asks for a new integration, tool wrapper, or automation path.
- A dynamic skill must be revised without restarting the runtime.

## Operational Rules

- Generate or update `skill.toml` and the runtime file(s) through the factory tools.
- Keep manifests explicit about permissions and required secrets.
- Prefer small, composable tools over monolithic "do everything" skills.
- Validate generated artifacts before reporting success.

## Topology Context

Registry-defined skills are now provisioned into persistent request/response topics at startup. After forging a skill, ensure its registry entry and instruction mapping are present so the runtime can provision and route it correctly.
