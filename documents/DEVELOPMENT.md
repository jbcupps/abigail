# Development branches

Keep these development lines and existing worktrees separate until their behavior and overlaps have been reviewed:

| Branch | Scope |
| --- | --- |
| `dev/first-signal-integration-0.0.76` | Reviewed main-based integration; development commit/push authorized; stable release safeguards and conversation history preserved; main/beta promotion remains held |
| `dev/first-signal` | Entity creation, visual identity metadata, and presence in the existing text runtime |
| `dev/continuity-boundary-hardening` | Continuity and runtime boundary hardening |

First Signal starts from `fcde548`. At publication preparation on 2026-10-19, remote `beta` at `ea92eff` contains newer overlapping Genesis/history work. Neither this branch nor the separate continuity branch should be treated as merged or release-ready. Reconcile overlaps and rerun integration checks in a dedicated integration branch before proposing promotion.

## Use an isolated worktree

Replace the path placeholders below with local directories. First inspect `git status` and `git worktree list` in the repository; reuse an existing worktree for the same branch rather than checking it out twice. Preserve unrelated changes.

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

## Publication requires the current task gate

The current integration branch starts at main `17b636b`, with First Signal applied as a reviewed patch. Integration tests and independent review passed; development commit/push is authorized. Use the explicit destination below only from that integration worktree. The remaining mobile and full-daemon acceptance gaps are recorded in `FIRST_SIGNAL_INTEGRATION.md`. No release workflow, tag, or main/beta merge is part of this bounded task.

The maintained/disposable Entity follow-up is documented in [PREVIEW_DATA_COMPATIBILITY.md](PREVIEW_DATA_COMPATIBILITY.md). A persistent preview needs the application and packaging boundary proposed in [PREVIEW_PROFILE_CONTRACT.md](PREVIEW_PROFILE_CONTRACT.md); the current data-root environment overrides do not provide that isolation. Do not treat this development branch as accepted for an ongoing production profile or signed preview update.

```sh
git -C "<worktrees>/first-signal-integration" push -u origin HEAD:refs/heads/dev/first-signal-integration-0.0.76
```

## Original prototype publication reference

Review the diff and validation evidence before committing. Publish the current branch with an explicit destination:

```sh
git -C "<worktrees>/first-signal" push -u origin HEAD:refs/heads/dev/first-signal
```

Use `HEAD:refs/heads/dev/continuity-boundary-hardening` only from that branch's worktree. Do not push these changes directly to `beta` or `main`, create release tags, or dispatch release workflows as part of development publication.

The current CI workflow runs on pushes to `main`/`beta` and on pull requests targeting either branch; a push to `dev/*` alone does not start it. Pull requests targeting a development branch also do not match those triggers. A later reviewed pull request to `beta` can request the full CI gate without merging. Pushing to `beta` or a `v*` tag starts the installer release workflow, so promotion remains a separate action.
