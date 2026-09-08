# Abigail initial application setup

The family-facing coordinator is **Abigail**. Internal crate names retain Hive.
This slice covers the splash, real offline setup chat, and a validated handoff to
an API model. Household accounts and a new Entity birth conversation are later work.

## First launch and recovery

The native shell paints the splash before starting the coordinator. The coordinator
starts its packaged Ollama on a dedicated loopback port and loads the packaged model.
Only a completed, nonempty generated reply changes readiness to `ready`; a listening
HTTP server is insufficient. Missing packages, failed loads, and timeouts surface
an error with retry. Loading can be paused. A failed local chat turn preserves input
and offers a local-model retry. Retry also restarts a failed coordinator owned by the native app and refreshes
its authenticated connection. An externally launched developer daemon remains
under its launcher’s control.

The setup model receives a narrow system prompt about API accounts, billing, the
connection form, and local/cloud operation. It has no shell, tools, or configuration
write capability. It remains a small language model: generated advice can be wrong;
the application's explicit controls and validation determine what actually changes.

The API form supports Anthropic and OpenAI. The person supplies a key and explicitly
selects a model (discovery is optional). A synthetic completion against that exact
model must succeed before a single encrypted vault entry containing provider, model,
and key is saved and made active. Errors retain the previous connection. Chat and
connection changes are serialized. Credentials never enter the test prompt or chat.
The form explains API billing and that subsequent cloud turns send recent context.

The immortal coordinator identity's `birth:setup_conversation` record holds the
setup transcript. Completed message pairs are written before returning success.
Model changes reuse that record and the mounted chat view. Restart reloads both
transcript and the encrypted active connection; **Continue locally** removes the
saved API connection and continues the same transcript locally.

## Offline distribution

The Windows x64 release lane stages these alongside the internal app/daemon binaries:

| Component | Pin | Terms / source |
| --- | --- | --- |
| Portable Ollama CPU runtime | v0.33.3 | [MIT license](https://github.com/ollama/ollama/blob/v0.33.3/LICENSE), [Windows portable distribution](https://docs.ollama.com/windows) |
| Qwen3.5-0.8B, Ollama Q8_0 package | `qwen3.5:0.8b`, manifest digest in the lock file | [Ollama model](https://ollama.com/library/qwen3.5:0.8b), [Apache-2.0 license](https://huggingface.co/Qwen/Qwen3.5-0.8B/blob/main/LICENSE) |
| WebView2 installer | Tauri `offlineInstaller` mode | Bundled by the Windows installer lane |

`scripts/offline-bootstrap.lock.json` pins the runtime archive and license hashes,
plus the model manifest digest. The staging script verifies each model blob against
the digest and length recorded in the verified manifest. The registry's tag endpoint
is checked against the pinned manifest digest; tag movement fails the build instead
of silently upgrading the model. Runtime binaries and model weights are unmodified;
GPU-specific runtime folders are omitted. Upstream licenses and notices accompany
the payload. Model pulls and runtime downloads occur only during build staging.

The measured staged model/runtime is approximately **1.112 GB** (about 1.036 GB of
model weights), before installer compression. The app, daemons, and offline WebView2
add further size. The staging gate limits the bootstrap payload to 1.7 GB to leave
NSIS headroom; the final installer size still needs to be checked. This package uses
CPU inference; startup speed and memory use vary by hardware. Changing models means
updating the lock and rerunning acceptance, not downloading a replacement at startup.

`OLLAMA_NO_CLOUD=1` is set on the private runtime, following the
[Ollama local-only setting](https://docs.ollama.com/faq). The application never adopts
an unrelated service on port 11434 and never modifies a user-installed Ollama.

## Caller identity and persistence

Loopback reachability grants no authority over product APIs. All Hive and Entity
routes except `/health` require a bearer credential. The native desktop supplies
the coordinator credential through the child environment and Tauri IPC, keeps it
out of URLs and localStorage, and stores the reusable desktop credential in the
OS-backed encrypted vault. A runtime descriptor contains URL/PID metadata only.
Adoption requires an authenticated status response; a stale PID is never killed.

Each family Entity receives a separate scoped Hive token and a separate Runtime
window token. The desktop passes the latter directly in that window's launch
command, avoiding a process-global token handoff race. Entity Hive tokens can read
their own configuration, use their own runtime lease, and access their own persistence
scope. They cannot call setup, enumerate secrets, or select another Entity or Hive
scope. Tokens are revoked when the supervised runtime is stopped/replaced.

The coordinator is the sole process opening `memory.db`. Family runtime memory,
queue, calendar, and knowledge-base access use its authenticated persistence RPC.
Each scope has its own database session. Remote SQL rejects session/schema management
commands, and the embedded engine disables scripting and outbound network targets.
Database-authenticated sessions add a second boundary. Windows paths are canonical
absolute paths rather than URL-style `/C:/...` strings.

This is process/capability isolation, **not household login, ownership, age permissions,
or the complete signed ethics system**. Code running as the same OS user may access
process memory/environment or the user's vault. Ollama's internal inference listener
does not itself authenticate callers; it receives no API credentials or mutation
authority. Do not expose these services beyond loopback. Setup transcript storage
does not claim whole-database encryption.

## Verification

From the repository root on Windows with Rust, Node, Python, and PowerShell 7:

```powershell
pwsh ./scripts/stage_offline_bootstrap.ps1
cargo build -p hive-daemon -p entity-daemon
cargo test -p hive-daemon --bin hive-daemon
cargo test -p abigail-persistence --lib
cargo check -p abigail-hive-app -p abigail-entity-runtime-app
pwsh ./scripts/test_offline_bootstrap.ps1
python scripts/test_initial_setup.py
```

Run `npm run build` and `npm test` in each split app's `src-ui` directory. The UI
tests cover real-readiness gating, pause/error states, explicit model selection,
validation failure, handoff timing, and preserving conversation and unsent input.
Rust provider tests use local mock endpoints and fake keys to exercise validation,
failure rollback, transcript continuity, encrypted save, and restart reload without
paid calls. The real-process Python probe checks offline completion, missing bundle,
retry/cancel, authentication, concurrent Entity opens, isolated durable memory, and
restart recovery in a disposable profile. It keeps logs there for diagnosis.

`python scripts/test_initial_setup.py --ui` additionally serves the actual setup UI
on loopback port 1421 for browser inspection, using only its disposable profile.
Stop with Ctrl+C. Never place real desktop/provider credentials in Vite environment
variables: development source is readable by clients. Native production uses IPC.
The old unauthenticated dev browser harness is no longer an automatic fallback.

The acceptance probes block ordinary outbound proxy traffic but do not disconnect
the operating system. Before promoting a release, run the **built full installer**
on a clean Windows machine with its network disconnected, including one without
WebView2. Verify splash, local response, pause/retry, uninstall behavior, installer
size, and the single app launch. Actual Anthropic/OpenAI account UAT also remains;
mock-provider tests do not establish a real account's billing or model availability.

Build a portable unsigned package with `scripts/build-split-installer.ps1`, or stage
the installer resources with `scripts/stage_split_installer_resources.ps1`. The
release workflow includes the offline payload and checks its contents and local
inference. No release or protected-branch push is part of local acceptance.

Application-owned setup guidance must remain visible independently of model prose:
API accounts are provider developer accounts; API keys are separate secrets created
inside those accounts. Only fixed official provider links and the secure connection
form direct credential entry. The live connection status and next setup action come
from application state. Qwen3.5-0.8B free-form explanations can confuse these terms,
even with examples; never treat its wording as proof of setup completion.

## Local acceptance recorded on 2026-09-08

- Both daemon binaries built; both desktop shells passed Cargo checks.
- Hive: 20 unit tests passed, including both provider request formats and failed-handoff rollback.
- Persistence: 6 tests passed, including reopening in a separate process and working directory.
- Real daemon integration: 4 Hive and 2 Entity tests passed with authentication enabled.
- UI: 7 coordinator tests and 1 Runtime test passed; both production frontends built.
- Packaged Ollama generated offline replies in disposable profiles. Real-process acceptance
  verified failure/retry, simultaneous Entity processes, separate memories and tokens, and
  transcript/memory recovery after restarting the coordinator.
- Browser inspection verified theme rendering, setup guidance, form labels, keyboard focus,
  and dismissal. A deliberately incorrect generated answer could not change the official
  links, account/key distinction, current connection status, or credential entry controls.

Free-form local wording remained inconsistent (including account/key confusion and an
incorrect app name). This is an observed limitation, not a passed semantic-quality test.
Application-owned guidance is the authority for the bounded onboarding steps. Full native
installer UAT on a disconnected clean Windows machine and real provider account UAT
were not performed in this worktree.
