# Preview Windows packaging

This records the combined development source's implemented runtime interface
and packaging contract. The metadata describes Rust behavior; it is not a
launch recipe interpreted by the app. An implemented interface does not
establish installer coexistence acceptance. The preview selector remains
unavailable until the exact combined source and installer pass
[the preview channel gates](PREVIEW_CHANNEL.md).
Current-source checks and the remaining acceptance boundary are recorded in
[combined development validation](PREVIEW_COMBINED_VALIDATION.md).

## Identities and build inputs

| Surface | Stable default | Preview |
| --- | --- | --- |
| Product | Abigail | Abigail Preview |
| Main launcher | Abigail.exe | Abigail Preview.exe |
| Hive identifier | com.abigail | com.abigail.preview |
| Runtime identifier | com.abigail.entity-runtime | com.abigail.preview.entity-runtime |
| Runtime product | Abigail Entity Runtime | Abigail Preview Entity Runtime |
| Default install directory | LocalAppData/Abigail | LocalAppData/Abigail Preview |
| Start-menu folder | Existing stable default | Abigail Preview |
| Desktop shortcut | Abigail.lnk | Abigail Preview.lnk |
| Uninstall product key | Abigail | Abigail Preview |

The two `tauri.preview.conf.json` files are explicit build overlays. They do
not change either stable `tauri.conf.json`. `mainBinaryName` excludes `.exe`;
the builder supplies the already-renamed executable to `tauri bundle`.
The internal Runtime/daemon resource filenames remain unchanged inside each
installation. App code must use the exact owned installation path, not find a
process by those shared basenames.

The checked Tauri CLI is 2.11.4. Its NSIS current-user installer derives the
uninstall key from the product name, under
`HKCU/Software/Microsoft/Windows/CurrentVersion/Uninstall/Abigail Preview`.
The remembered installation path is product scoped too. Its main-process
checks use `Abigail Preview.exe`; its shortcut AppUserModelID uses
`com.abigail.preview`. These are source-template findings, not a substitute
for inspecting and testing the generated candidate installer.

## Implemented profile and runtime interface

The selected `dev/preview-combined-0.0.76` integrates app
`ca6a7527da8787be6b5315e13e4fc228287ce15d`, its published cold-recovery descendant
`4558fe68d9812998eb53a94ba157970af552f923`, and packaging
`de65e7e47fa35c7b742421274db5e1fac4720f8d` (including scaffold
`6b246d3ab07322a899106d1198bfafba3a7b2309`).
See [the source alignment](../documents/PREVIEW_RELEASE_ALIGNMENT.md).

Both [Hive](../hive-app/tauri.preview.conf.json) and
[Runtime](../entity-runtime-app/tauri.preview.conf.json) overlays carry the same
`plugins.abigail-launch` contract: `schemaVersion: 1`,
`contract: "abigail-channel-profile-v1"`, `channel: "preview"`,
`releaseChannel: "beta"`. `runtimeInterface.status` is `implemented`,
`metadataRole` is `descriptive-only`, and `candidateAcceptance` remains
`pending`. These separate source implementation from exact candidate acceptance.

| Metadata area | Implemented invariant |
| --- | --- |
| `launcher` | Compiled-preview Hive fills only absent `ABIGAIL_PROFILE=preview` and `ABIGAIL_DATA_DIR=R`. Blank/conflicting inputs and any inherited `ABIGAIL_HIVE_URL` fail. |
| `daemonCli` | Both daemons require `--profile preview --data-dir R` matching validated environment. Hive gets an ephemeral `--port` outside `43141..43150`. Entity gets `--entity-id`, matching `--hive-url`, and `--port 0`. |
| `childEnvironment` | Profile/root and `ABIGAIL_PROFILE_ID`, `ABIGAIL_HIVE_INSTANCE_ID`, `ABIGAIL_PROCESS_INSTANCE_ID` propagate explicitly. Optional `ABIGAIL_DOCUMENTS_DIR` must equal `R/Documents`. |
| `runtimeSelectionEnvironment` | GUI receives validated `ABIGAIL_ENTITY_ID`, `ABIGAIL_ENTITY_URL`, `ABIGAIL_HIVE_URL` and the selected Entity daemon's expected process identity. |
| `packagedExecutableEnvironment` | Canonical packaged paths are required; blank, relative or conflicting inherited executable/internal-bin overrides fail. |
| `processIdentity` | Verify every `channel`, `profile_id`, `canonical_data_root`, `instance_id`, `hive_instance_id` field and the selected Entity. Hive instance is fresh and equals its daemon instance. Each Entity daemon instance is fresh/distinct; the Runtime GUI expects that daemon's instance. |
| `http` | Pin `x-abigail-profile-id` and `x-abigail-process-instance-id`. Only GET `/health`, GET `/v1/status` and OPTIONS are exempt. These non-secret routing guards are not authentication or binary provenance. No redirects or native browser/query/environment/default-port fallback. |
| `lifecycle` | Entity close pins `expected_entity_instance_id` and validates its API envelope. Hive replacement requires acknowledged drain/reaping and owned-child exit. No stale-PID killing; a stale creation endpoint rejects while preserving the exact key. All four preview processes hold a shared profile-use lease until exit; cold operations require exclusive profile-use and Hive-owner leases; startup rejects cold-staging paths including ancestors. |
| `dynamicValueSources` | Absolute roots, persistent profile UUIDs, fresh process UUIDs, endpoints and one-use runtime-bootstrap capabilities come from validated launch/runtime state, never static release metadata. |

The default root is `<KnownFolder LocalAppData>/AbigailPreview/profiles/ongoing`,
separate from both install directories. `R/profile.json` has strict
`schema_version: 1`, `kind: "preview"`, `channel: "beta"` and a persistent,
canonical non-nil `profile_id` UUID. Marker schema 1 is independent of Entity
config schema 27. Only a new empty validated root can initialize a marker;
children cannot create a missing parent marker. Synthetic fixtures use explicit
separate disposable roots, never the continuing root.

The Entity's `ABIGAIL_RUNTIME_BOOTSTRAP` is a generated one-use capability,
separate from the non-secret identity headers. Both daemon entry points reject
inherited `ABIGAIL_CI_MODE` and `ABIGAIL_DAEMON_INTEGRATION` in preview.
Verified shutdown is `POST /v1/shutdown` with
`{ok:true,data:{shutdown:true,owned_entities_reaped:true}}`, followed by owned
Hive child exit. Uncertain drain does not authorize force-stop/replacement.
Termination of a verified owned Entity child handle remains supported.

The [packaging validator](../scripts/packaging_profile.ps1) checks the complete
closed launch schema in both overlays, including exact types and nested members.
It rejects the old `pending-app-owner` placeholder, fixed `profileId`,
alternative `roots`, concrete endpoint/root/UUID additions and static
`daemonArguments` recipes. Named CLI options are descriptive: the Rust launcher
generates their actual arguments at runtime. Update both overlays, the validator,
tests and documentation together when the implemented interface changes.

All four binaries must compile with `ABIGAIL_BUILD_PROFILE=preview`. Tauri
plugin JSON does not implement isolation automatically, and embedded strings
prove inclusion only. The combined integration also narrows Entity handshake
exemptions to the documented GET methods and OPTIONS, with synthetic regression
tests; actual installed lifecycle and race acceptance remain open.

The lifecycle descriptors `profileUseLease: "shared-until-process-exit"`,
`coldOperationLease: "exclusive-profile-use-and-hive-owner"` and
`coldStagingStartup: "reject-including-ancestors"` describe the included recovery
source's ownership rules. Cooperative leases do not find unknown profile clones,
exclude every older or foreign writer, or permanently prevent two restored roots
from opening later. See [the cold-recovery contract](../documents/PREVIEW_COLD_RECOVERY.md).
The offline `abigail-profile` utility is a separate workspace executable, not a
fifth installer payload in this candidate. The four-binary staging and signing
contract is unchanged. Real Windows credential, GUI reopening and exact installed
recovery acceptance remain open.

## Nonpublishing build knobs

`stage_split_installer_resources.ps1 -Channel preview -Version X.Y.Z` builds
daemons without a Tauri overlay, then Hive and Runtime separately with their
respective overlays. All three Cargo calls use the compiled preview profile.
It defaults to `target/preview` if no target was supplied, preserves an explicit
target, and restores its environment on failure.
The default stable invocation retains its combined build and existing override
behavior. It rejects an inherited nonempty `ABIGAIL_BUILD_PROFILE` to prevent
accidentally compiling a preview profile under stable application identities.

`build_signed_installer.ps1` accepts `-Channel stable|preview`, default stable.
Repeat the same channel, numeric version and options for Prepare, Check and
Sign. Preview appends `/preview` to the requested target/cache root. Its
default preparation directory is
`target/preview/signed-release/X.Y.Z`. An explicit `-WorkDir` must be dedicated
to that channel; Prepare refuses an existing other-channel preparation.
Config/source/byte mismatches require reprepare.

Preparation freezes both overlay hashes, launch-contract hash, compiled
identities, source snapshot and four executable hashes. Check and Sign reject
a stable preparation reused as preview or the reverse. The signing callback
captures only the selected launcher name. Public candidate filenames retain
the canonical `Abigail-windows-x64-setup.exe`; the product/launcher identities
inside the installer are distinct.

Preview Sign emits only a signed candidate with compatibility pending and
`publicationEligible: false`. It writes `latest-candidate.json`, never the
stable `latest-success.json`, and returns before the legacy installer and
12-stage acceptance invocation. Run the separate disposable Windows
compatibility harness against those exact bytes. Do not rebuild or relabel
them after acceptance. No preview dispatch or publication is added by this
change, and no signing credentials or service enrollment are configured.
The existing release workflow still uses the stable default. Selecting preview
there requires a later reviewed integration change.

Raw builder output is not yet the release-asset set accepted by
`prepare_preview_assets.ps1`: it lacks final native-verification evidence and
`release-provenance.json`. The later acceptance/staging adapter must bind those
records to the same candidate source and installer hash, preserve all installer
bytes, and refuse promotion until all compatibility gates pass.

## Installer guard limits and remaining verification

The preview-only NSIS hooks must reject a destination overlapping the stable
installation or the continuing preview profile, including unsafe path aliases.
They must not remove profile data or stop processes by shared internal names.

In the pinned template, PREINSTALL runs after `SetOutPath`, which can create
the chosen directory, and after the WebView2 prerequisite section and
reinstallation page. It runs before payload, registration and shortcut writes.
PREUNINSTALL runs before payload deletion. Hooks alone therefore do not prove
zero earlier side effects or safe maintenance of a previous preview install.
Installer upgrade/uninstall behavior and all process behavior still require
the generated-installer coexistence tests in a disposable Windows environment.
Do not run this acceptance on a user's installed stable app or real profile.

References:
[Tauri configuration](https://v2.tauri.app/reference/config/),
[pinned NSIS installer template](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.11.4/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi),
[pinned NSIS helpers](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.11.4/crates/tauri-bundler/src/bundle/windows/nsis/utils.nsh).
