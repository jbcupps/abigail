# Skills + Shared Chat Proof Checklist

Date: 2026-03-07

## Objective

Prove, with repeatable local evidence, that:
1. The shared `entity-chat` engine correctly executes the tool-use loop.
2. Skills are discovered and registered through the shared runtime bootstrap.
3. Chat through CLI triggers tool execution and returns results.
4. Chat through GUI triggers the same engine path.
5. Browser fallback remains viable for authenticated, stateful workflows.

## Proof Cases

### PROOF-001: Tool-use loop unit coverage
- **Layer**: `entity-chat` crate
- **Check**: `cargo test -p entity-chat`
- **Criteria**:
  - `build_tool_definitions` returns correct qualified names for registered skills.
  - `build_tool_definitions` skips tools with malformed parameter schemas.
  - `execute_single_tool_call` handles invalid tool name format.
  - `execute_single_tool_call` handles malformed JSON arguments.
  - `execute_single_tool_call` records success and failure correctly.

### PROOF-002: Shared runtime bootstrap inventory
- **Layer**: `entity-daemon` / `abigail-runtime`
- **Check**: `cargo test -p entity-daemon --test integration_skills`
- **Criteria**:
  - Dynamic skills are discoverable from JSON files.
  - Built-in and factory skills register without error.
  - Shared bootstrap registers the supported native skill inventory.

### PROOF-003: CLI scaffold-to-chat proof
- **Layer**: `entity-cli` + `entity-daemon`
- **Check**: `cargo test -p entity-cli`
- **Criteria**:
  - Scaffolding creates valid dynamic skill artifacts.
  - The generated skill is discoverable and registerable.

### PROOF-004: GUI harness parity
- **Layer**: `tauri-app/src-ui`
- **Check**: `npm run test:coverage`
- **Criteria**:
  - Browser harness returns tool-call metadata.
  - Chat UI renders tool invocation results.
  - Skill factory and clipboard paths both work through the shared UI contract.

### PROOF-005: SkillFactory authoring round-trip
- **Layer**: `abigail-skills`
- **Check**: `cargo test -p abigail-skills`
- **Criteria**:
  - `author_skill` creates the expected on-disk artifact set.
  - The created skill becomes discoverable.

### PROOF-006: Persistent browser auth survives restart
- **Layer**: `abigail-skills` + `skill-browser`
- **Check**: `cargo test -p abigail-skills --test browser_persistent_auth -- --nocapture`
- **Criteria**:
  - Login state is persisted to the entity profile.
  - Restarting the Browser skill preserves the authenticated session.
  - The authenticated destination remains accessible after restart.
