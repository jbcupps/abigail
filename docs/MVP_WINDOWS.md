# Unsigned Windows MVP

The MVP is one Windows installer and one Abigail launch. Hive stays available
while a named Entity chats in its own window. Entity purpose, signed identity,
conversation history, and model settings survive restarting.

## Install and use

1. Run `dist/windows-mvp/Abigail-windows-x64-setup.exe` and launch Abigail.
2. In Hive, connect a model. Choose a loaded Ollama/LM Studio local server,
   a supported cloud provider with its API key, or an installed official
   Claude, Codex, or Grok CLI using your existing account.
3. Create an Entity with a name and purpose, then open it and chat.
4. Close and reopen the Entity to continue its saved conversation.

The installer is unsigned by request. It includes both desktop shells and both
internal daemons; no Rust, Node, terminal, or frontend development server is
required on the machine running the installed app. Tauri's installer includes
the WebView2 bootstrapper when that Windows runtime is missing.

Model weights are not included. A local option needs a running server with a
loaded model (for example Ollama at `http://127.0.0.1:11434` or LM Studio at
`http://127.0.0.1:1234`). Cloud processing sends messages and context to the
selected provider. The app stores identity and history locally. Installed CLI
connections are chat-only, with native tools and extensions disabled or rejected
when they cannot be isolated. Gemini CLI is not supported.

### Connect an installed account

Hive discovers supported official CLI installations without sending model
requests. Choose the installed connection to check it. A successful, nonempty
real model reply is required before Hive saves it; this can take up to a minute.
Codex and Grok can appear even when sign-in is unknown, so you can check an
existing account. If sign-in is needed, run `codex login` or `grok login` in that
CLI, then check the connection in Hive again. A desktop app alone does not
establish a working CLI connection.

The official CLI retains ownership of account sign-in, token storage, and
refresh. Abigail does not extract or forward account OAuth tokens. Account
limits and the selected CLI model still apply. If active hooks, plugins, MCP,
or other extensions cannot be isolated, disable them in your CLI or choose
another Hive model connection; Abigail does not rewrite your CLI settings.

Claude Code runs unmodified using each person's own authentication and billing,
subject to [Anthropic's product-use conditions](https://code.claude.com/docs/en/legal-and-compliance#can-customers-offer-claude-code-in-their-products).
Current Codex app-server account authentication is intended for local or
open-source use. Commercial or hosted distribution requires the applicable
approved Sign in with ChatGPT integration; see [OpenAI's authentication guidance](https://learn.chatgpt.com/docs/app-server#auth-endpoints)
and [Sign in with ChatGPT availability](https://developers.openai.com/siwc/quickstart).

## Repeatable build

From the repository root, with Rust, Node/npm, and Tauri CLI installed:

```powershell
pwsh ./scripts/build-mvp-windows.ps1 -VerifyInstall
```

The default debug profile is packaged as a standalone application using Tauri's
custom protocol. For an optimized build, add `-Configuration release`.
No certificate, SSL.com service, signing key, or updater is used in this lane.
The installer defaults to zlib compression for faster rebuilds. Add
`-Compression lzma` for a smaller installer with a longer packaging step.

Outputs:

- `dist/windows-mvp/Abigail-windows-x64-setup.exe`
- `dist/windows-mvp/mvp-build.json`, including installer and executable hashes
- An isolated installed-payload verification report under `target/manual-test/`

## Acceptance

```powershell
pwsh ./scripts/tests/run-mvp-acceptance.ps1 -BinaryDir target/debug
```

This test uses an explicitly synthetic local model. It checks the actual daemon
processes with a fresh durable data directory: helper plus two simultaneous
Entities, quickstart birth, ordinary and streaming chat, rejected invalid
persistence leases and unauthorized lease creation/disclosure, history after
Entity reopen, and history after Hive restart. Backend tests separately exercise
valid leases, cross-Entity denial, and rejected database management commands.
It clears inherited in-memory CI flags. Results are written beneath
`target/manual-test/mvp-acceptance/`, with `latest.json` pointing at the latest run.

Run the same command with `-BinaryDir` pointing at the isolated installed
directory to test its bundled daemons. Runtime contracts and real-model/desktop
acceptance are separate checks; a synthetic fixture does not prove model quality.

With a real loaded local model available, run the separate integration smoke:

```powershell
node ./scripts/tests/mvp-real-model.mjs --binary-dir <installed-directory> --model-url http://127.0.0.1:11434
```

This exercises real ordinary and streaming inference through Hive and an Entity,
then verifies the same saved conversation after Entity and Hive restarts.

To exercise an already signed-in official Codex CLI without injecting an API key:

```powershell
node ./scripts/tests/mvp-real-model.mjs --binary-dir target/debug --cli-provider codex-cli
```

Use an installed Abigail directory as `--binary-dir` to test its packaged
daemons. The CLI run checks selected-provider traces without fallback, live SSE,
conversation context using a random word from the prior turn, and exact saved
history after Entity and Hive restarts. Grok uses `--cli-provider grok-cli` after
successful user sign-in. To verify the current missing-sign-in error separately:

```powershell
node ./scripts/tests/mvp-real-model.mjs --binary-dir target/debug --cli-provider grok-cli --expect-connection-error "needs sign-in"
```

That negative mode verifies an actionable error and an unchanged, unconfigured
Hive. It does not send an accepted chat turn or prove real model inference.

For an isolated desktop launch, set `ABIGAIL_DATA_DIR` to a fresh test directory
before starting the installed `Abigail.exe`. This also isolates its daemon
descriptor and Documents folders. `ABIGAIL_DOCUMENTS_DIR` can explicitly select
a separate Documents root. Normal launches use the standard application data
directory and the user's Documents folder.

## MVP boundaries

The MVP covers setup, distinct Entities, useful chat, durable history, provider
changes, and safe default execution. Household member accounts, child-specific
permissions, automatic model downloads, and generated native-code execution are
outside this acceptance scope. Confirmation-required tools remain unavailable
to automatic chat/job calls until a real mentor confirmation is supplied.
Runtime database tools use scoped record operations and simple table reads;
arbitrary SQL and database management commands are unavailable in this MVP.

Hive remains the sole owner of the embedded memory engine. Entity runtimes use
lease-scoped HTTP calls to their own database; this avoids multi-process file
locks while retaining one shared local storage substrate.

## Verified MVP build — 2026-10-02

The prior unsigned Windows x64 installer was built, silently installed into an isolated
directory, and exercised through its actual desktop windows with a real local
model. It is a standalone debug-profile MVP build with embedded assets and zlib
compression, not a development-server launch.

| Check | Result |
| --- | --- |
| Backend unit tests | 495 passed; one pre-existing test ignored |
| Real daemon integration tests, explicitly enabled | 6 passed |
| Hive and Entity frontend tests, including generated CSS | 15 passed |
| Both frontend production builds and four native binaries | Passed |
| Installed executable hashes and daemon loader probes | All four payloads passed |
| Installed synthetic contracts | All 12 stages passed |
| Installed real-model integration with Ollama `llama3.2:1b` | All 10 stages passed |
| Desktop setup, named Entity, real reply, Hive availability and exact history reopening | Passed |

These counts describe the earlier local-model MVP acceptance, before the
installed Codex/Grok expansion. Updated CLI results are recorded separately
below.

The ignored legacy memory test races asynchronous file-lock release inside one
process. Separate uncached subprocess reopening and the installed Entity/Hive
restart tests passed. Cloud inference and Claude CLI inference were not accepted
with live credentials in this run; the real-model proof covers the local server.
Model answer quality is not scored by the integration tests.

The installer verified in that run was `99,596,180` bytes with SHA-256
`AE131EE47F6B01E19F9A2898F925CB6D6BD9D0029B867AD75644145C1572ED7C`.
Its installer and original manifests are archived under
`dist/windows-mvp/previous-local-model-mvp/`; the archived `mvp-build.json` records
that run's payload hashes and embedded frontend entries. Its recorded absolute
installer path describes the original build location. Tauri's unique NSIS
launcher marker is accounted for before comparison.

Local evidence for this run:

- `target/manual-test/mvp-backend-tests.log`
- `target/manual-test/mvp-installed-f9c08168-verification/installed-payload.json`
- `target/manual-test/mvp-acceptance/2026-10-02T15-57-26-502Z-6fd70f58/result.json`
- `target/manual-test/mvp-real-model/2026-10-02T16-01-47-334Z-06574d67/result.json`
- `target/manual-test/mvp-desktop/20261002-153940-ec1e2ac4/result.json`

That desktop run used Hive and Mira in an isolated demo profile with the
temporary local model server at `http://127.0.0.1:11436/v1`. Normal
launches from the installed app icon use the regular application profile and
need a model connection; the installer does not include model weights or install
a persistent model service.

## Installed-CLI validation — 2026-10-02

The installed-account expansion was tested against the actual installed daemon
payload in `target/manual-test/mvp-installed-241668ce/resources`. The official
installations were Codex `0.153.4` and Grok Build `0.2.93`. Codex's existing account
served real model requests without an injected API key. Grok returned
authentication required, and user sign-in with `grok login` remains pending.

| Check | Current status |
| --- | --- |
| Expanded backend unit and integration regression | 546 distinct tests passed: 517 unit and 29 integration; 10 ignored |
| Hive and Entity frontend tests | 17 passed: Hive 10, Entity 7 |
| Installed synthetic daemon contract | All 12 stages passed |
| Codex installed-account completion, streaming, and Hive/Entity pipeline | All 11 real-model stages passed without an injected API key |
| Codex provider selection and context | `codex-cli` trace without fallback; live tokens and a prior-turn random word reproduced |
| Codex exact persisted conversation after Entity and whole Hive restart | All four messages restored exactly |
| Codex native tool/extension isolation | Separate live app-server proof passed; no native tool items or server tool requests |
| Final installed desktop workflow | All eight stages passed: saved Codex connection, Nova creation/birth, real chat, Hive availability, and exact GUI history reopening |
| Grok failure in the actual desktop wizard | Missing-sign-in error preserved the existing Codex selection |
| Grok expected authentication error and unchanged Hive default | All four negative stages passed; no provider saved |
| Grok real completion and streaming | Pending successful user sign-in and real inference |
| Claude expected failed-connection check and unchanged Hive default | All four negative stages passed; no provider saved |
| Claude direct native diagnostic | HTTP 401/authentication required confirmed; user `claude auth login` and real inference pending |
| Final desktop assets and refreshed installer | Installed and all four payload hashes verified; Hive GUI includes small-window scrolling |

The backend count uses the full regression run plus the latest capabilities
result, replacing its earlier count rather than double-counting repeated tests.
Codex native isolation was verified separately through an owned app-server
protocol session equivalent to the adapter. Its installed 11-stage acceptance
then exercised the compiled adapter and complete Hive/Entity pipeline. Model
answer quality was not scored.

The final unsigned installer at
`dist/windows-mvp/Abigail-windows-x64-setup.exe` is `99,931,900` bytes with SHA-256
`669D542390FE35D76F0BFAA19C3BD7FECA20F2F66A0B87FADCB2F646C660F854`.
It was installed into `target/manual-test/mvp-installed-b6c11623` and all four
executable payloads were verified. Both daemons and the Entity Runtime GUI are
bit-for-bit identical to the earlier `mvp-installed-241668ce` payload used by the
12-stage contract, 11-stage real Codex run, and four-stage Grok negative run.
Those acceptance results apply to the same binaries; the harnesses were not
rerun under the final directory name. The refreshed Hive GUI embeds
`index-CFLL8OiJ.js`, adding small-window modal scrolling, and its 10 frontend
tests passed. `dist/windows-mvp/mvp-build.json` records the current package.

The final installed Hive and Nova desktop windows were exercised directly in
`target/manual-test/mvp-vendors-desktop/20261002-170045-c8b0a37e`. Hive restored
the saved Codex selection. A failed Grok connection check left that selection
intact. Nova was created and born with a practical purpose, then answered a
synthetic grocery prompt with two items: pasta and marinara sauce. Closing and
reopening its Runtime restored the exact prompt and reply while Hive stayed
usable. Hive and Nova remain open in this isolated demo profile; normal launches
use the regular application profile.

Existing-account Grok never sends ACP `authenticate`, including `cached_token`:
the vendor handler can fall back to interactive login. Authentication is consumed
only from `initialize`/`session/new`; unavailable credentials fail without
starting sign-in. Its negative acceptance proves that this failure is actionable
and does not persist a default; it is not real inference acceptance.

Claude's four-stage installed negative check likewise rejected the failed model
connection without saving a default. Hive gave safe guidance to check sign-in
and the selected model. A separate scalar-only native diagnostic confirmed
authentication required and HTTP 401 without recording raw CLI output or
credentials. User sign-in with `claude auth login` and live Claude inference are
still pending; no Claude real-inference acceptance is claimed.

Local evidence:

- `target/manual-test/mvp-vendors-backend-final.log`
- `target/manual-test/mvp-vendors-capabilities-final.log`
- `target/manual-test/mvp-vendors-hive-ui-tests.log`
- `target/manual-test/mvp-installed-b6c11623-verification/installed-payload.json`
- `target/manual-test/mvp-vendors-contract/2026-10-02T17-01-16-589Z-40d13cff/result.json`
- `target/manual-test/mvp-cli-codex-installed/2026-10-02T17-01-16-224Z-3ef3f2dc/result.json`
- `target/manual-test/vendor-integration/codex-isolation-probe-2026-10-02.json`
- `target/manual-test/mvp-cli-grok-installed/2026-10-02T17-01-16-753Z-ac3a7c6f/result.json`
- `target/manual-test/mvp-cli-claude-installed/2026-10-02T17-13-46-497Z-2f75d438/result.json`
- `target/manual-test/mvp-cli-claude-installed/direct-probe-2026-10-02T17-15-23-749Z-9b538960/result.json`
- `target/manual-test/mvp-vendors-desktop/20261002-170045-c8b0a37e/result.json`
