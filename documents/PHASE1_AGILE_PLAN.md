# Phase 1: Foundation — Agile Sprint Plan

**Start Date:** TBD (on your go)
**Duration:** ~6 weeks (15 cycles)
**Cadence:** 2-day cycles with 3x daily check-ins and every-other-day demos

---

## How This Works (The Git Workflow)

### The Concept

Think of it like architecture drafting. **Main** is the approved, stamped blueprint set. **The dev branch** is the drafting table where I sketch. When you review my draft and stamp it, we fold it into the blueprint set. Then I start the next sketch from a clean copy of the approved set.

### The Cycle (Repeats Every 2 Days)

```
Day 1 (Build Day)                    Day 2 (Demo Day)
─────────────────                    ─────────────────
Morning check-in  ←── you           Morning check-in  ←── you
  "Here's what I'll build today"       "Here's what's ready to demo"

I code...                            YOU walk through the running app
                                     or test output
Midday check-in   ←── you
  "Here's progress, any blockers"    Midday check-in   ←── you
                                       "Approved" or "Change X"
I code...
                                     If approved → I create PR
Evening check-in  ←── you             You say "merge it"
  "Here's what's done, what's next"    I merge → main is updated

                                     Evening: I start next cycle
                                       (fresh branch from main)
```

### What Happens in Git (Plain English)

| Step | What I Do | What It Means |
|------|-----------|---------------|
| **Cycle start** | Create a fresh branch from `main` | I'm working on a clean copy of the latest approved code |
| **I code** | Make commits on the dev branch | Saving my work as I go — you can see each save point |
| **Check-in** | I show you what changed | You tell me if I'm on track or need to adjust |
| **Demo** | You run the app and test it | You see the feature working with your own hands |
| **You approve** | I open a Pull Request (PR) | A PR is a formal "request to merge my work into main" |
| **You merge** | I merge the PR | The approved work becomes part of the official codebase |
| **Cycle resets** | New branch from updated `main` | Clean slate, incorporating everything approved so far |

### If You Want Changes

- **Small tweak:** I fix it on the same branch, you re-demo
- **Wrong direction:** I abandon the branch, we start fresh from main
- **Partial approval:** I split the work — merge what's good, carry forward what needs more work

### Key Terms Cheat Sheet

| Term | Meaning |
|------|---------|
| **Branch** | A parallel copy of the code where I make changes without affecting the original |
| **Main** | The "official" version — only approved work goes here |
| **Commit** | A save point with a description of what changed |
| **Pull Request (PR)** | A proposal saying "here's what I changed, please review and approve" |
| **Merge** | Folding the approved changes back into main |
| **Diff** | A side-by-side comparison showing exactly what changed |

---

## Release & Package Paths (Cross-Cutting Requirement)

Abigail ships through **6 release/package paths** today. This is a strategic advantage — OpenClaw only has `npm install`. Every feature we build must work across all paths, and new binaries get their own path added.

### Current Paths (Must Not Break)

| # | Path | Format | Trigger | Pipeline |
|---|------|--------|---------|----------|
| 1 | **Windows installer** | NSIS `.exe` | `v*` tag on main | `release.yml` → `tauri-action` |
| 2 | **macOS installer** | Universal `.dmg` (Intel + Apple Silicon) | `v*` tag on main | `release.yml` → `tauri-action` |
| 3 | **Linux installer** | `.deb` (Ubuntu 22.04+) | `v*` tag on main | `release.yml` → `tauri-action` |
| 4 | **npm package** | `npx abigail-desktop install` | `release.yml` → `npm publish` | Downloads correct platform binary |
| 5 | **GitHub Release** | All 3 platform artifacts + notes | `release.yml` Stage 2 | `softprops/action-gh-release` |
| 6 | **abigail-keygen** | Bundled egui binary (per-platform) | Built as resource in release | `cargo build -p abigail-keygen` |

### New Path Added in Phase 1

| # | Path | Format | Trigger | Pipeline |
|---|------|--------|---------|----------|
| 7 | **abigail-cli** | Standalone binary (Win/Mac/Linux) | `v*` tag on main | New job in `release.yml` |

### CI Quality Gate (Must Pass Every PR)

The existing CI runs **5 parallel checks** on every PR to main:

```
┌──────────┐  ┌────────────────────┐  ┌──────────┐  ┌─────────┐  ┌────────┐
│   Lint   │  │  Tests (3 platforms)│  │ Frontend │  │  Audit  │  │ CodeQL │
│ fmt+clip │  │  Win/Mac/Linux     │  │ tsc+vite │  │cargo+npm│  │  SAST  │
└────┬─────┘  └─────────┬──────────┘  └────┬─────┘  └────┬────┘  └───┬────┘
     └──────────────────┼──────────────────┼─────────────┼────────────┘
                        ▼                  ▼             ▼
                   ┌─────────┐
                   │  Gate   │  ← branch protection requires this
                   │(any pass)│
                   └─────────┘
```

### Rule: No Feature Ships Without All Paths Green

For every cycle/PR:
1. `cargo test --all` passes (covers new crate code)
2. `cargo clippy` clean (no warnings)
3. Frontend builds (`npm run build`)
4. New binaries (abigail-cli) added to release workflow when they exist
5. Demo runs on at least one platform before merge

This means the **Definition of Done** includes: "doesn't break any existing release path, and new artifacts have their release path defined."

---

## Phase 1 Feature Backlog

Six deliverables, ordered by dependency and value. Each is sized to fit a 2-day cycle (or split across cycles if larger).

### Feature 1: Anthropic Claude Provider
**Why:** Opens Abigail to the best available LLM. OpenClaw recommends Claude as primary.
**What you'll see in demo:** Type a message in Abigail, get a response from Claude instead of (or alongside) GPT.
**Cycles:** 1 (2 days)

**Current state:**
- `LlmProvider` trait exists with `complete()` method
- `OpenAiProvider` works (cloud GPT)
- `LocalHttpProvider` works (Ollama/LM Studio)
- API key validation for Anthropic already exists (just not wired to a provider)

**Work items:**
- [ ] Create `AnthropicProvider` struct implementing `LlmProvider` trait
- [ ] Handle Anthropic's message format (system prompt is separate, not a message)
- [ ] Map Abigail's `ToolDefinition` to Anthropic's tool format
- [ ] Wire into `IdEgoRouter` so `set_api_key("anthropic", key)` builds an Anthropic-backed Ego
- [ ] Add Tauri command to switch Ego provider between OpenAI/Anthropic
- [ ] Test with real API key

---

### Feature 2: Streaming Responses
**Why:** Currently the UI freezes while waiting for the full response. Streaming shows words as they arrive.
**What you'll see in demo:** Type a message, see the response appear word-by-word in real time.
**Cycles:** 1-2 (2-4 days)

**Current state:**
- `reqwest` has "stream" feature enabled but unused
- Chat returns a complete string in one shot
- Tauri event system exists (used for download progress)

**Work items:**
- [ ] Add `stream()` method to `LlmProvider` trait (returns async stream of tokens)
- [ ] Implement streaming for `OpenAiProvider` (SSE parsing)
- [ ] Implement streaming for `AnthropicProvider` (SSE parsing)
- [ ] Implement streaming for `LocalHttpProvider` (SSE parsing)
- [ ] Add `chat_stream` Tauri command that emits token events
- [ ] Update React `ChatInterface` to render tokens as they arrive
- [ ] Graceful fallback: if streaming fails, fall back to non-streaming

---

### Feature 3: Wire Superego (3-Way Routing)
**Why:** Abigail's unique architectural differentiator. Adds an ethical oversight layer to every interaction.
**What you'll see in demo:** A settings panel where you configure Id/Ego/Superego providers. Chat messages show which path they took.
**Cycles:** 1 (2 days)

**Current state:**
- `TrinityConfig` struct exists with superego fields (provider, API key)
- `SuperegoVerdict` and `check_search_query()` safety checks exist
- Comment: "Follow-up: 3-way routing" in code
- Router only uses 2-way (Id/Ego) today

**Work items:**
- [ ] Extend `IdEgoRouter` to `TrinityRouter` (or add Superego layer)
- [ ] Superego pre-check: before routing to Id or Ego, run Superego safety check
- [ ] Superego can use any provider (Anthropic, OpenAI, local)
- [ ] Add Tauri commands: `set_superego_provider`, `get_trinity_status`
- [ ] Update frontend settings to show 3-provider configuration
- [ ] Show routing decision in chat UI (small label: "via Id", "via Ego", "blocked by Superego")

---

### Feature 4: Core Skills (Filesystem + Shell + HTTP)
**Why:** The skill system works but only has 1 active skill. These 3 unlock the agent's ability to actually DO things.
**What you'll see in demo:** Ask Abigail to "list files in my home directory", "run `cargo --version`", or "fetch the weather from an API" — and it does it.
**Cycles:** 2 (4 days — one cycle per 1-2 skills)

**Current state:**
- `SkillRegistry`, `SkillExecutor`, `EventBus` all work
- `skill.toml` manifest system works
- `web-search` skill is the working reference implementation
- Permission system with `Sandbox` checks exists

**Work items:**

**4a: Filesystem skill (Cycle 1)**
- [ ] `skill.toml` manifest with FileSystem permission
- [ ] Tools: `read_file`, `write_file`, `list_directory`, `search_files`
- [ ] Path sandboxing (restrict to allowed directories)
- [ ] Register in Tauri app alongside web-search

**4b: Shell command skill (Cycle 1)**
- [ ] `skill.toml` manifest with ShellExecute permission
- [ ] Tools: `run_command` (with timeout, output capture)
- [ ] Allowlist/blocklist for dangerous commands
- [ ] Working directory control

**4c: HTTP request skill (Cycle 2)**
- [ ] `skill.toml` manifest with Network permission
- [ ] Tools: `http_get`, `http_post`, `http_request` (generic)
- [ ] SSRF protection (reuse existing validation)
- [ ] Response size limits

---

### Feature 5: Skills Watcher (Hot-Reload)
**Why:** During development (and for users adding skills), the app shouldn't need a restart to pick up new skills.
**What you'll see in demo:** Drop a new skill folder into the skills directory → Abigail detects it and shows it in the skill list without restart.
**Cycles:** 0.5 (1 day — pair with another small item)

**Current state:**
- `SkillRegistry::discover()` scans directories on startup
- No file watching

**Work items:**
- [ ] Add `notify` crate for filesystem watching
- [ ] Watch skills directories for changes (new/modified `skill.toml`)
- [ ] Re-run discovery on change
- [ ] Emit Tauri event when skills list updates
- [ ] Frontend reacts to skill list changes

---

### Feature 6: CLI Interface (Headless Operation)
**Why:** Enables Abigail to run without the desktop GUI — foundation for server/gateway deployment later. Also adds a **new release artifact** to Abigail's multi-path distribution.
**What you'll see in demo:** Run `abigail-cli chat "What is 2+2?"` in a terminal and get a response. No window opens.
**Cycles:** 1.5 (3 days)

**Current state:**
- All logic is in library crates (`abigail-router`, `abigail-skills`, `abigail-memory`, `abigail-core`)
- But wired together only in `tauri-app/src/lib.rs`
- No standalone CLI binary
- `npm-package/` already has a CLI pattern (`abigail-desktop`) we can extend

**Work items:**
- [ ] New crate: `abigail-cli` (or binary in existing crate)
- [ ] `clap` argument parser: `abigail-cli chat "message"`, `abigail-cli config set-key`, `abigail-cli skills list`
- [ ] Reuse `abigail-router`, `abigail-skills`, `abigail-memory` directly (no Tauri dependency)
- [ ] Interactive REPL mode: `abigail-cli` with no args drops into a chat loop
- [ ] Add to workspace `Cargo.toml`
- [ ] Output formatting (colored terminal output with streaming)

**Release path work items (new!):**
- [ ] Add `abigail-cli` build step to `release.yml` (3-platform matrix, like abigail-keygen)
- [ ] Upload `abigail-cli` binaries as GitHub Release assets (abigail-cli-windows-x64.exe, abigail-cli-macos-universal, abigail-cli-linux-x64)
- [ ] Update `npm-package/` to also offer `npx abigail-desktop cli` as a subcommand
- [ ] Add `abigail-cli` to CI test matrix (ensure `cargo build -p abigail-cli` passes on all 3 platforms)

---

## Sprint Schedule

### Week 1: LLM Core

| Day | Type | Cycle | Work | Demo Deliverable |
|-----|------|-------|------|-----------------|
| 1 | Build | C1 | Anthropic provider implementation | — |
| 2 | Demo | C1 | — | **Demo 1:** Chat with Claude in Abigail |
| 3 | Build | C2 | Streaming responses (trait + OpenAI + Anthropic) | — |
| 4 | Demo | C2 | — | **Demo 2:** Watch responses stream in real-time |
| 5 | Build | C3 | Streaming for LocalHttp + UI polish | — |

### Week 2: Routing & First Skills

| Day | Type | Cycle | Work | Demo Deliverable |
|-----|------|-------|------|-----------------|
| 6 | Demo | C3 | — | **Demo 3:** Streaming works for all providers |
| 7 | Build | C4 | Superego wiring + trinity routing | — |
| 8 | Demo | C4 | — | **Demo 4:** 3-way routing visible in UI |
| 9 | Build | C5 | Filesystem skill + shell skill | — |
| 10 | Demo | C5 | — | **Demo 5:** Ask Abigail to list files, run commands |

### Week 3: Skills Expansion

| Day | Type | Cycle | Work | Demo Deliverable |
|-----|------|-------|------|-----------------|
| 11 | Build | C6 | HTTP skill + skills watcher | — |
| 12 | Demo | C6 | — | **Demo 6:** HTTP requests + hot-reload skills |
| 13 | Build | C7 | CLI interface (core + chat subcommand) | — |
| 14 | Demo | C7 | — | **Demo 7:** `abigail-cli chat` works in terminal |
| 15 | Demo | — | — | **Buffer / catch-up / polish** |

### Weeks 4-6: Buffer & Hardening

Reserved for:
- Fixes from demo feedback
- Edge cases, error handling
- Integration testing
- Documentation
- Items that took longer than estimated
- **Stretch goals** if ahead of schedule

---

## Check-In Protocol

### Morning Check-In (start of your day)
**You provide:**
- Thumbs up/down on yesterday's evening update
- Any priority changes ("skip X, focus on Y")
- Questions or concerns

**I provide:**
- Today's specific plan (what I'll build in the next few hours)
- Any blockers or decisions I need from you

### Midday Check-In
**You provide:**
- Quick reaction to progress shown
- Course corrections if needed

**I provide:**
- Progress update with specifics (files changed, tests passing)
- Preview of what I'll tackle in the afternoon
- Any design decisions I need input on

### Evening Check-In (end of your day)
**You provide:**
- Review of the day's work
- Approval to continue on current path, or pivot

**I provide:**
- Summary of everything completed today
- What's staged for tomorrow
- Updated backlog status

---

## Demo Protocol

### Before Each Demo
I will:
1. Ensure the dev branch compiles and runs (`cargo tauri dev` or `cargo test`)
2. Provide clear steps for you to test the feature
3. List what's new vs. what was already there

### During Each Demo
You will:
1. Pull the dev branch and run the app (I'll give you exact commands)
2. Walk through the test steps
3. Note anything that doesn't work or feel right

### After Each Demo
You decide one of:
- **"Approved"** → I create a PR, you say merge, we move on
- **"Approved with notes"** → I create a PR + capture notes as issues for later
- **"Needs changes"** → I fix on the same branch, we re-demo next day
- **"Scrap it"** → I abandon the branch, we reassess

---

## How to Run Demos (Commands You'll Use)

```bash
# 1. Get the latest code (I'll tell you the branch name)
git fetch origin
git checkout <branch-name>

# 2. Build and run
cargo tauri dev

# 3. If testing CLI (Feature 6)
cargo run -p abigail-cli -- chat "Hello, Abigail"
```

I'll provide exact commands at each demo point. If anything fails, share the error output in our check-in and I'll fix it.

---

## Adjustability

This plan is designed to flex:

- **Ahead of schedule?** Pull work forward from weeks 4-6 buffer, or start Phase 2 items
- **Behind schedule?** Drop or defer lowest-priority items (CLI is the first candidate to defer)
- **Priority shift?** Reorder the backlog at any check-in — just tell me
- **Scope change?** Add/remove features between cycles — the 2-day cycle means nothing is more than 1 day from a decision point

### Priority Order (If We Need to Cut)
1. Anthropic provider ← **must have**
2. Streaming responses ← **must have**
3. Core skills (filesystem + shell) ← **must have**
4. Superego wiring ← **high value, Abigail differentiator**
5. HTTP skill ← **nice to have**
6. Skills watcher ← **nice to have**
7. CLI interface ← **can defer to Phase 2**

---

## Definition of Done (Per Feature)

- [ ] Code compiles with no warnings (`cargo clippy`)
- [ ] Existing tests still pass (`cargo test --all`)
- [ ] New tests cover the happy path
- [ ] Feature works in the running Tauri app (or CLI)
- [ ] **All existing release paths unbroken** (NSIS, DMG, deb, npm, GitHub Release, abigail-keygen)
- [ ] **New release artifacts have pipeline coverage** (if applicable)
- [ ] You've walked through the demo and approved
- [ ] PR merged to main

---

## Ready to Start?

When you say go, I will:
1. Confirm `main` is clean and up to date
2. Create the first dev branch: `dev/phase1-c1-anthropic-provider`
3. Begin Cycle 1: Anthropic Claude Provider
4. Post the first morning check-in
