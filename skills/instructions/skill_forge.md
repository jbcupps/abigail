# Skill Forge

Use `builtin.skill_factory` for its existing authoring tools when the user asks to create, update, or remove a persistent capability. DevOps Forge's request/response topics are a separate proposal-validation path and do not apply changes.

## When To Use

- The request is repeatable or likely to be reused.
- The user asks for a new integration, tool wrapper, or automation path.
- A dynamic skill must be revised without restarting the runtime.

## Operational Rules

- Generate or update `skill.toml` and the runtime file(s) through the factory tools.
- Keep manifests explicit about permissions and required secrets.
- Prefer small, composable tools over monolithic "do everything" skills.
- Validate generated artifacts before reporting success.

## DevOps Forge Proposals

- Submit a bounded code + markdown envelope to `topic.skill.forge.request` and read `topic.skill.forge.response`.
- Unknown fields are rejected, including `mentor_approved`, permissions, scores, and adoption flags. Model output cannot establish host authorization or scoring.
- `status=validated` means static validation passed. `applied`, `fitness_evaluated`, and `hot_reload_triggered` remain false; no skill or registry artifacts are written by this worker.
- Do not report a validated proposal as built, improved, installed, or usable. The broker has no connected host approval, behavioral evaluation, adoption, or rollback path.
- The direct Rust writer requires a non-deserializable host authorization argument. That trusted-caller contract is not an approval mechanism exposed through messages or a tool to invoke from model output.

## Topology Context

Registry-defined skills are provisioned into persistent request/response topics at startup. After an authorized authoring operation, verify its registry entry, instruction mapping, and actual runtime availability before relying on it. A registry touch requests watcher processing; it does not acknowledge activation. A Forge validation response does not change topology.
