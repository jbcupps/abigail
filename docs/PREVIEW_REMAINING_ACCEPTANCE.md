# Remaining preview acceptance for the signed candidate

2026-10-20. This is a finite checklist for product
`f7c7848d25726480b253ec1ca637c3ffa5ea3c77` and installer SHA-256
`a59580764b7cf072cf7c6eeccf040f5c934c5b0c5c164cfe5338e2ccfe5199d1`.
It does not request a new build, signing pass, provider account, control-model
redesign or rollout system.

“Complete acceptance” below means the existing ten stages in
[PREVIEW_CHANNEL.md](PREVIEW_CHANNEL.md#required-acceptance-evidence), with their
actual recorded observations. It is not an open-ended compatibility claim.
Keep already observed results; rerun only what is needed to capture missing
observations or validate a change. Prior success cannot reconstruct deleted
private fixture state.

## Required visible smoke

1. Start the exact installed Hive through the packaged launch path; verify the
   native WebView renders against only the chosen preview fixture. Verify the
   installed shortcut resolves to that same signed executable. A shortcut that
   would open an existing/default user profile must not be used for this test.
2. Select the loopback synthetic model, complete name/purpose/create/birth, and
   hold a visible conversation. No provider credential or external model call.
3. Close/reopen Runtime, then Hive, and observe the same ongoing Entity UUID,
   settings and exact visible history.
4. With separately approved disposable stable and preview fixtures, exercise
   visible coexistence in both launch orders. Closing either must leave the
   other usable. A one-preview-fixture approval does not cover creating a fresh
   stable fixture as an incidental step.
5. Keep fresh-creation tests distinct from the ongoing Entity. Record their
   different fixture identities; do not reset the continuing Entity.

## Existing selector stages: observed and missing

| Stage | Already observed for this candidate | Concrete remaining proof |
| --- | --- | --- |
| `separate-installer-identity` | Hosted payload signatures/hashes, uninstallers, registrations and shortcuts checked. | Bind the generated recipe, compiled identities and channel-specific process checks to the candidate; observe its normal packaged GUI launch. |
| `stable-preview-coexistence` | Both daemon launch orders; quiescent install/reinstall/uninstall; stable fingerprints and stable return preserved. | Install/reinstall/uninstall with the disposable stable instance active, proving its processes and conversation unaffected; visible coexistence above. |
| `profile-path-isolation` | Explicit hosted roots and selected snapshot/restore destinations; conflicting profile mode refused. | Inventory persistent paths including archives, backup discovery, recovery documents, logs and descriptors. Verify blank/invalid roots refuse and default KnownFolder resolution cannot fall back to stable, within a disposable environment. Never probe this by opening Jim's existing/default profile. |
| `daemon-ownership-isolation` | Wrong/missing/stale routing pins and invalid close identities rejected; acknowledged preview shutdown and owned process exit. | Wrong/stale descriptors, inherited endpoints, occupied ports, concurrent starts, crash/orphan and GUI close/reopen cases must never adopt, fall back to or terminate stable. |
| `fresh-creation-fixture` | Payload checks; synthetic create/birth/chat/stream; same-key replay and creation receipts. | Complete the preview-aware 12-stage daemon contract, including helper/two concurrent Entities, cross-Entity transcript isolation, lease and persistence denials, using the separate creation fixture. |
| `ongoing-entity-upgrade` | Same-candidate reinstall retained UUID, birth certificate, presence, receipts and history. | Record the supported seeded-first-preview baseline and actual seed/source, identity, history and settings digests before/after. An earlier accepted preview is not required. |
| `ongoing-entity-reopen` | Daemon and Hive restart retained exact history. | Visible Runtime/Hive reopening and exported actual continuity observations, not only pass Booleans. |
| `full-cold-backup-restore` | Cold snapshot/verify/fresh restore/reopen passed with original profile/Entity IDs and master public key. | Record snapshot, identity, history and settings digests required by the selector. Any further recovery utility must have its actual binary/source identity recorded; only the old utility hash was exported. |
| `newer-schema-refusal` | Six exact signed-Hive preflight checks passed with complete synthetic file/directory inventories unchanged: future marker, malformed/duplicate marker, newer root/active-Entity config, conflicting profile mode. | Bind this existing scoped report into final evidence. Observe the installed entry point as part of the visible smoke; do not relabel standalone preflight evidence as installed-daemon refusal. |
| `return-to-stable` | Stable reopened with retained state after preview uninstall; compatible preview snapshot restored separately. | Visible stable return and final continuity evidence. Hosted stable cleanup used its owned job boundary, not graceful stable shutdown. |

The missing continuity record is specific: ongoing Entity UUID; before/after/
restored history digests; before/after settings digests; seeded baseline and
snapshot digests; and distinct creation/ongoing fixture IDs. The completed
hosted artifact intentionally exported Boolean assertions rather than these
private observations. The removed fixtures cannot supply them retrospectively.

## Claims outside this launch checklist

Native Windows Credential Manager support, cross-user/device recovery and real
external-provider inference remain unclaimed. They are not additional launch
requirements for this build. Neither an imaginary older accepted preview nor
new governance, control-model or live-customer rollout machinery is required.
The current synthetic model checks must remain labelled synthetic.

Exact-product source CI, the reviewed publication-only operation, and actual
public-download verification are separate release steps. Signature validity
and installed-lifecycle success do not resolve the existing advisory audit.

## Read-only desktop inspection and its limits

The registry inspection attempted `RegistryKey.OpenBaseKey` for CurrentUser and
LocalMachine, using Registry32 and Registry64, followed by read-only
`OpenSubKey` on these four relative paths:

```text
Software\Microsoft\Windows\CurrentVersion\Uninstall\Abigail
Software\Microsoft\Windows\CurrentVersion\Uninstall\Abigail Preview
Software\abigail\Abigail
Software\abigail\Abigail Preview
```

It sought only installation location/default value and display version.
Two `OpenSubKey` operations returned **“Requested registry access is not
allowed.”** The original output did not label each individual hive/view/path
before opening it, so the exact two failing combinations are unknown. This is
an instrumentation limitation, not proof that any registration is absent.
The exception establishes a read-access refusal; it does not identify whether
an OS ACL or the execution environment caused it. No access-control change,
repeat registry read, registry UI workaround or elevated retry was performed.

A separate permitted process snapshot at `2026-10-20T18:59:16Z` found zero
matching Abigail Hive/Runtime/daemon processes. That says nothing about
installed-but-closed products and does not complete the registry inspection.

Computer-use discovery performed:

- Read the installed computer-use skill and its guidance/confirmation rules.
  Its Windows entry point requires `node_repl` with `@oai/sky`.
- Searched exposed tool metadata for `node_repl`, native desktop and screen
  tools. No `node_repl` was exposed. Deferred tool search returned an
  unsupported-call error, not a user approval denial.
- Refreshed the documented CUA interface and called its read-only `getState`.
  The tool declaration disables native APIs; the enabled inventory returned
  `apps: []` and a connected Chrome browser. Generic documentation mentioning
  native methods is not evidence those methods are enabled.
- Read the existing Chrome tab at
  `http://127.0.0.1:1522/?presencePreview=1`. It visibly identifies itself as
  **LOCAL DESIGN PREVIEW**, with browser-only identity drafts and no connected
  model/provider. No reload, navigation, form action or native app launch was
  performed.

Thus browser visual inspection is available without repeating the denied
registry operation. It is **not** an inspection route into the signed installed
WebView. Product Tauri configs distinguish development URLs from embedded
`frontendDist`; native mode supplies verified Rust IPC and profile-bound WebView
storage, while the design preview skips readiness and uses browser localStorage.
Source has no configured browser mirror or default CDP inspection listener for
the packaged app. No debugging port, injected bridge, custom automation helper
or security setting was added to manufacture that route.

Browser-only design review can cover appearance, layout, form/error states and
browser-local drafts. It cannot establish native rendering/IPC, real Entity
creation, retained profile state, DPAPI or stable/preview coexistence. Fresh
local fixture approval remains pending; method permission alone does not supply
it. The supported native desktop tool and completed installation isolation
inspection are also prerequisites before executing that fixture.
