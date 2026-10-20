# Development branches

Keep the original development lines and worktrees intact. The selected combined candidate uses the published inputs below in its own isolated checkout:

| Branch | Scope |
| --- | --- |
| `dev/preview-combined-0.0.76` | Current authorized app + cold-recovery + packaging dev integration; exact source inputs below; combined lifecycle and installer acceptance remain open |
| `dev/first-signal-integration-0.0.76` | Published application slice `ca6a7527da8787be6b5315e13e4fc228287ce15d` and cold-recovery descendant `4558fe68d9812998eb53a94ba157970af552f923`; stable release safeguards and conversation history preserved |
| `dev/preview-packaging-0.0.76` | Published packaging slice `de65e7e47fa35c7b742421274db5e1fac4720f8d`, including release scaffold `6b246d3ab07322a899106d1198bfafba3a7b2309` |
| `dev/first-signal` | Entity creation, visual identity metadata, and presence in the existing text runtime |
| `dev/continuity-boundary-hardening` | Continuity and runtime boundary hardening |

First Signal starts from `fcde548`. Beta `ea92effd31f554f74bb5e109deaa42876ec4f06f` contains overlapping Genesis/history, static-runtime and installer-lock work that remains separate for explicit later comparison. Do not merge beta blindly. The combined candidate and separate continuity branch are not release-ready.

## Use an isolated worktree

Replace the path placeholders below with local directories. Within an authorized repository, inspect its current status and reuse a suitable isolated checkout. Preserve unrelated changes and other workers' checkouts. For the combined task, published GitHub sources were cloned into a new task workspace after checking remote branches; no combined remote branch existed. Do not inspect prior workers' Git directories or retry a denied cross-worktree inspection by another route.

For a fresh local checkout of the published First Signal branch:

```sh
git -C "<repository>" fetch origin
git -C "<repository>" worktree add --track -b dev/first-signal "<worktrees>/first-signal" origin/dev/first-signal
```

If the local branch already exists and has no worktree, omit `--track -b` and use `dev/first-signal` as the final argument. Use a different directory and `dev/continuity-boundary-hardening` for continuity work.

Run frontend checks from the chosen worktree:

```sh
cd "<worktrees>/first-signal/hive-app/src-ui"
npm ci
npm test
npm run build
cd ../../entity-runtime-app/src-ui
npm ci
npm test
npm run build
```

See [First Signal](FIRST_SIGNAL.md) for the browser preview, synthetic transport fixture, actual in-process route/storage tests, and the unrun full-daemon integration test. Use invented test content. The cached Rust validation helpers require explicit local dependency paths and do not replace a clean build. The focused route runner is `crates/hive-daemon/tests/check-presence-routes.ps1`; it imports the production handlers and avoids daemon startup.

## Current combined development publication

The selected `dev/preview-combined-0.0.76` integrates app `ca6a7527da8787be6b5315e13e4fc228287ce15d`, its published cold-recovery descendant `4558fe68d9812998eb53a94ba157970af552f923`, and packaging `de65e7e47fa35c7b742421274db5e1fac4720f8d`. These are based on stable main `17b636b9e5f10b812deb287ad2c6637188991ec2`; packaging includes scaffold `6b246d3ab07322a899106d1198bfafba3a7b2309`. Review, tests, all-four-binary preview build, dev commit and push are authorized. No signing, installer execution, real credentials, production deployment, new access grants, release workflow, tag or main/beta merge is part of this bounded task. Stable 0.0.76 stays available.

The maintained/disposable Entity follow-up is documented in [PREVIEW_DATA_COMPATIBILITY.md](PREVIEW_DATA_COMPATIBILITY.md). The accepted [preview profile boundary](PREVIEW_PROFILE_CONTRACT.md) is implemented in application source through one validated immutable context, scoped paths/credentials and verified process transport. `ABIGAIL_DATA_DIR` alone is not the contract. The included [Windows cold-recovery source and CLI](PREVIEW_COLD_RECOVERY.md) preserve the whole retained profile and UUIDs under process/cold leases, restore only into a fresh root, and require existing same-user/device credentials. The published slice's [104 distinct Rust tests and seven CLI checks](../output/playwright/cold-profile-validation.md) remain prior component evidence; fresh combined reruns are recorded in [combined validation](../docs/PREVIEW_COMBINED_VALIDATION.md). The offline utility is not added to the four-payload installer. Combined lifecycle, normal GUI reopening, real OS credentials and Windows install/upgrade/uninstall/restore acceptance remain open. Do not treat this development branch as accepted for an ongoing real profile or signed preview update.

[PREVIEW_RELEASE_ALIGNMENT.md](PREVIEW_RELEASE_ALIGNMENT.md) records exact launcher parameters, assigned packaging identifiers, selected source inputs and remaining acceptance. The combined candidate aligns both preview overlays, validator, tests and documentation. Build all four binaries with `ABIGAIL_BUILD_PROFILE=preview` using the channel-aware staging path and the separate shell overlays. Reprepare source, overlay, contract and executable hashes after changes; preserve stable defaults and signing/publication guards. The application's prior 167 selected Rust and 373 frontend passes and the prior packaging checks do not prove combined acceptance.

```sh
git -C "<combined-checkout>" push -u origin HEAD:refs/heads/dev/preview-combined-0.0.76
```

## Original prototype publication reference

Review the diff and validation evidence before committing. Publish the current branch with an explicit destination:

```sh
git -C "<worktrees>/first-signal" push -u origin HEAD:refs/heads/dev/first-signal
```

Use `HEAD:refs/heads/dev/continuity-boundary-hardening` only from that branch's worktree. Do not push these changes directly to `beta` or `main`, create release tags, or dispatch release workflows as part of development publication.

The current CI workflow runs on pushes to `main`/`beta` and on pull requests targeting either branch; a push to `dev/*` alone does not start it. Pull requests targeting a development branch also do not match those triggers. A later reviewed pull request to `beta` can request the full CI gate without merging. Pushing to `beta` or a `v*` tag starts the installer release workflow, so promotion remains a separate action.
