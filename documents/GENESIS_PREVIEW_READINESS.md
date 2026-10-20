# Genesis / First Signal preview preparation

2026-10-20, AILap, `dev/genesis-first-signal-integration`.

## Mechanical compiler cleanup

The three supervisor warnings follow the four vault diagnostics documented in
[the correctness review](GENESIS_FIRST_SIGNAL_REVIEW.md). Repository-wide caller
inspection found `entity_daemon_binary` and `HiveSupervisor::new` are test-only;
both now use `#[cfg(test)]`. The unreferenced `spawn_entity_daemon` wrapper is
removed. `hive-daemon` is a binary-only crate with no package features. Production
continues to use `entity_daemon_binary_for_profile`, `HiveSupervisor::with_profile`
and `spawn_entity_with_profile`. No runtime branch, ownership check, credential
operation, shutdown behavior or supported feature configuration changes.

Workspace Clippy then exposed the same unused-constructor issue in
`entity-daemon::backup_ops::LocalBackupOps::new`. There are no production or test
callers anywhere in the repository, and this is also a binary-only crate without
package features. The unused constructor is removed; the existing production and
test callers of `new_with_profile` and its validation remain unchanged.

The existing supervisor tests exercise those test helpers. Final validation must
run the full Hive and Runtime frontend suites/builds, scoped Genesis and creation
handler regressions, supervisor regressions, native-shell compile, formatting and
strict daemon/workspace Clippy on the exact committed source. Raw command logs and
the exact-SHA result summary belong in the ignored `output/preview-source-ready/`
directory. Compiling a test target is not executing its tests. No broad credential
test suite or advisory security audit is part of this cleanup.

## Minimum build-only operation

After final source checks pass, use the existing preparation phase in this isolated
worktree, recording the exact HEAD first:

```powershell
pwsh -NoProfile -File scripts/build_signed_installer.ps1 -Phase Prepare -Channel preview -Version 0.0.76
```

The existing harness uses `0.0.76`; that is a candidate packaging version, not a
new release or permission to replace the accepted bytes. Preparation compiles
all four executables with `ABIGAIL_BUILD_PROFILE=preview`, separate Hive/Runtime
overlays, embedded frontend assets and static Windows runtime checks. It freezes
the source, overlays, runtime contract and executable hashes in
`target/preview/signed-release/0.0.76/prepared-build-state.json`. It neither signs
nor starts the product or an installer. This is unsigned build material, not yet
a user-tested installer.

The concrete missing local prerequisite is the pinned npm Tauri CLI `2.11.4`:
neither `tauri`/`tauri.cmd` on PATH nor the script's local
`.cache/signing/tauri-cli/node_modules/.bin/tauri.cmd` existed at inspection.
Git, Cargo, Node, npm and PowerShell are available. No CLI was installed in this
turn. Both locked frontend dependency trees must be present before preparation;
Hive imports Runtime source. The script rebuilds the frontends with `npm ci`.
No credentials, paid service or OS security changes are needed for preparation.

## Existing hosted paths and boundaries

Read-only GitHub inspection confirmed `Abigail Preview Candidate` is registered
and active (workflow `390817899`). Its successful run
[39468905473](https://github.com/jbcupps/abigail/actions/runs/39468905473) used
harness `dfebfb8b589bbf3f8275eff5547e1e7c8d7b91c4` and product
`f7c7848d25726480b253ec1ca637c3ffa5ea3c77`. Default-branch registration is therefore
not the current blocker. That signed candidate and its synthetic acceptance are
historical evidence for different source/bytes.

Both the candidate workflow and the manual source-CI lane still pin `f7c7848`.
Dispatching either unchanged would not validate this integration. A new hosted
candidate needs a reviewed dev-only source-pin/harness update, corresponding guard
and fixture alignment, and exact reviewed harness/source SHAs. The actual
candidate workflow then signs and runs disposable installation/DPAPI fixtures;
those actions need their separate specific scope and are excluded from this
turn. The full manual source-CI suite also contains native credential fixtures;
local scoped regression results do not substitute for that hosted evidence.

`Stabilization Build (Unsigned)` is also registered, but its unchanged workflow
builds raw default-profile binaries without the preview overlays/embedded
frontend staging. It is not an equivalent ready-to-use preview installer path.
No workflow was dispatched and no beta/main branch or publication pointer changed.

After new candidate bytes exist, remaining acceptance includes real native
creation, honest interruption/retry, first useful conversation, full process
restart, and exact identity/session/history return in a disposable profile.
Native capture is unavailable here. Source tests and browser fixtures cannot
establish native installed acceptance. H-011 is already saved; no acknowledgement
or duplicate coordination item is needed.
