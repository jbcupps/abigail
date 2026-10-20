# First Signal integration validation

Date: 2026-10-20. Branch: `dev/first-signal-integration-0.0.76`.
Base: `17b636b9e5f10b812deb287ad2c6637188991ec2`.
Validation was recorded before the authorized development commit/push. No release or full daemon startup occurred.

Later maintained/disposable Entity checks and the selected-Entity identity fix are recorded in [preview continuity validation](preview-continuity-validation.md). Its UI/backend counts replace the corresponding earlier counts below. The proposed persistent preview boundary remains unimplemented; see [the application/packaging contract](../../documents/PREVIEW_PROFILE_CONTRACT.md).

## Completed

- Hive UI: 264 tests passed. Runtime UI: 68 tests passed.
- Both TypeScript/Vite production builds passed.
- Actual IdentityManager creation durability: 15 tests passed.
- Actual creation handlers with inert, isolated state: six tests passed.
- Core API wire/compatibility: three tests passed.
- Real profile router/storage with synthetic identity lookup: 11 tests passed.
- Normal Hive and Entity daemon builds passed, locked/offline, without test fixture features in the production IdentityManager.
- Synthetic in-memory HTTP fixture: 17 transport checks passed.
- Independent reviews completed; concrete findings fixed and re-reviewed.
- Release workflows, release verification scripts, and Cargo.lock have no integration diff.

The direct creation-handler tests do not test Axum routing, extractors, middleware, or process bootstrap. The real profile router tests use a synthetic registered-identity resolver with real profile storage; their separate results are in `integration-backend-validation.txt`.

## Browser evidence

The browser used a loopback-only UI and synthetic in-memory HTTP fixture. No model, provider, microphone, credentials, or user files were used.

1. Created the invented brief “Lyra integration”, with Lumen/Curious presentation.
2. Stopped waiting during a deliberately delayed create. The UI retained the original key and warned that server creation might continue.
3. Closed the tab, reopened it, and resumed the same brief/key. Exactly one Entity appeared.
4. Safe retry confirmed Entity `442ec927-9708-48a5-a5c1-e661aa8ed959` for key `7efc6082-62db-408b-8645-f42ecfeeb6bb`.
5. Saved the brief explicitly, saw the reveal, and opened the same Entity.
6. Sent invented text with Enter; observed request-wait and streaming states.
7. Stopped a deliberately slow response and observed the interrupted state.
8. Requested a scripted error and observed explicit connection-error text.
9. Reloaded/reopened the conversation: the completed synthetic turn and submitted messages were restored. Cancelled/error turns had no invented completed assistant reply.
10. Pause motion and accessible control labels were inspected. The final mobile layout check is incomplete.

Screenshots:

- `14-integrated-home.jpg`: explicit synthetic transport banner.
- `15-integrated-character.jpg`: visual identity selection.
- `16-exact-key-recovered.jpg`: exact-key confirmation and retained brief.
- `17-recovered-reveal.jpg`: recovered identity saved.
- `18-restored-conversation.jpg`: final compiled app, synthetic banner, restored history.

The last creation-status margin correction was built after screenshots 15-16. Earlier 01-13 screenshots are historical prototype evidence.

The task-owned production preview on port 1523 and synthetic fixture on port 1532 were stopped after evidence capture. Screenshots and logs remain available locally.

## Held actions and limits

- Full real-daemon HTTP acceptance: not attempted. Existing normal startup reads the OS keyring before the raw test-key override. `integration-acceptance-plan.json` records the plan-only harness gate. No credential-code workaround was introduced.
- Mobile browser action: the last request was `Emulation.setDeviceMetricsOverride` on the local test tab, with width 390, height 844, deviceScaleFactor 1, mobile false. It was intended only to inspect the narrow layout. Platform approval was reported pending. The original browser tool call subsequently returned, but approval resolution was not independently confirmed; further dependent browser actions were held. No read-only approval-status tool was available to reconcile the result. The original platform approval record is authoritative.
- Server creation guarantees assume the existing supported single Hive owner per data root.
- Profile PUTs lack server-side version preconditions. An accepted stopped/timed-out PUT may finish later; the UI does not promise rollback.
- No claim is made about model quality, video/audio assets, microphone capture, cross-device draft sync, or production deployment.
