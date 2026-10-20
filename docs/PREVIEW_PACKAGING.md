# Preview Windows packaging

This is an implementation contract for a pending preview candidate. It does
not establish that the application consumes the launch metadata or that an
installer passes coexistence acceptance. The preview selector remains
unavailable until the exact combined source and installer pass
[the preview channel gates](PREVIEW_CHANNEL.md).

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

## Accepted profile contract and pending runtime interface

Both preview overlays supply identical `plugins.abigail-launch` JSON:

```json
{
  "schemaVersion": 1,
  "contract": "abigail-channel-profile-v1",
  "channel": "preview",
  "releaseChannel": "beta",
  "compiledProfile": {
    "environment": "ABIGAIL_BUILD_PROFILE",
    "value": "preview"
  },
  "profileRoot": {
    "windowsKnownFolder": "LocalAppData",
    "relativePath": "AbigailPreview/profiles/ongoing"
  },
  "profileMarker": {
    "fileName": "profile.json",
    "schemaVersion": 1,
    "kind": "preview",
    "channel": "beta",
    "persistentIdField": "profile_id"
  },
  "runtimeInterface": {
    "status": "pending-app-owner"
  }
}
```

The parent accepted this profile contract on 2026-10-20. The root is separate
from both installation directories. The launcher resolves the Windows
KnownFolder and validates the absolute root and containment. The staging script
builds all four native binaries with `ABIGAIL_BUILD_PROFILE=preview`.
Application code must ensure runtime variables cannot turn that build into
stable. `profile.json` must contain a persistent, non-secret
UUID in `profile_id`, preserved across preview updates. Synthetic tests use
separate disposable roots, never this continuing root.

Exact runtime switches and environment names remain pending application-owner
confirmation; this metadata deliberately contains no executable launch recipe.
The earlier draft `--channel/--profile-root` switches are withdrawn. The shell
must bind the metadata to its compiled identifier, reject missing/unknown or
conflicting channel/root inputs, and refuse an inherited cross-channel
endpoint. Runtime startup must receive an explicitly validated endpoint and
profile. Daemon discovery, adoption, restart and shutdown must validate
channel, profile and live process ownership. Stable fallback is forbidden for
preview. A custom synthetic fixture root must be explicit and validated; it
must never redirect tests into the continuing profile.

Tauri permits arbitrary plugin metadata, but does not implement this interface
automatically. Checking embedded strings proves only that the identity and
metadata were included in the binary. No Rust launch/process/path behavior was
changed by this packaging slice.

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
