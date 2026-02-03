# Abby

Abby is a desktop agent application built with [Tauri](https://tauri.app/) and Rust. The MVP provides startup, a local built-in LLM (Id/Candle), signature verification of constitutional docs, and chat in the UI.

## Quick start

- **Prerequisites:** Rust (stable), Node.js 20+ (for the frontend).
- **Build & run:**
  ```bash
  cargo build
  cd tauri-app/src-ui && npm install
  cargo tauri dev   # from repo root
  ```
- **Optional:** Set `OPENAI_API_KEY` for cloud (Ego) routing. See `example.env` for placeholders.

Full instructions, installer options, and tests: [documents/HOW_TO_RUN_LOCALLY.md](documents/HOW_TO_RUN_LOCALLY.md).

## Project layout

| Path | Description |
|------|-------------|
| `crates/` | Rust workspace: abby-core, abby-memory, abby-llm, abby-router, abby-birth, abby-skills |
| `tauri-app/` | Tauri desktop app and React frontend (`tauri-app/src-ui`) |
| `templates/` | Constitutional docs (soul, ethics, instincts) |
| `documents/` | Runbooks, scope, release, and environment notes |
| `skills/` | Skill implementations (e.g. skill-proton-mail) |

## Docs

- [How to run locally](documents/HOW_TO_RUN_LOCALLY.md) — build, dev, installers, tests
- [MVP scope](documents/MVP_SCOPE.md) — what’s in and out of scope
- [Release process](documents/RELEASE.md) — versioning and publishing
- [Security notes](documents/SECURITY_NOTES.md) — threats and secrets handling

## License

MIT — see root [Cargo.toml](Cargo.toml) `[workspace.package]`.
