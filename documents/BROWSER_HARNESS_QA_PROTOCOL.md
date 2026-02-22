# Browser Harness QA Protocol

## Purpose

This protocol defines a strict, repeatable QA process for validating the browser Tauri harness and ensuring no regressions in the core Birth -> Provider -> Chat -> Clipboard flow.

Targets covered:
- `tauri-app/src-ui/src/browserTauriHarness.ts`
- `tauri-app/src-ui/src/main.tsx`
- `tauri-app/src-ui/src/__tests__/App.browserFlow.test.tsx`
- `tauri-app/src-ui/src/components/BootSequence.tsx`
- `tauri-app/src-ui/src/components/ChatInterface.tsx`

---

## 1) Test Case Template and ID Convention

Use this format for every test case in this protocol.

### 1.1 Required Fields

- **Test ID**: `<suite>-<number>` (examples: `HARN-001`, `FLOW-010`, `PROV-020`, `CHAT-030`, `REG-040`, `NATIVE-050`)
- **Title**
- **Priority**: `P0`, `P1`, `P2`
- **Type**: `Automated` or `Manual`
- **Preconditions**
- **Steps** (numbered)
- **Expected Result**
- **Evidence Required**
- **Result**: `Pass`, `Fail`, `Blocked`
- **Notes / Defect Link**

### 1.2 ID Ranges

- `HARN-001` to `HARN-009`: Harness bootstrap and guarding
- `FLOW-010` to `FLOW-019`: Birth progression and stage gates
- `PROV-020` to `PROV-029`: Provider setup and routing indicators
- `CHAT-030` to `CHAT-039`: Chat streaming/status + clipboard path
- `REG-040` to `REG-049`: Build and regression suite
- `NATIVE-050` to `NATIVE-059`: Native parity smoke checks

---

## 2) Preflight and Environment

### 2.1 Preconditions

- Branch checked out with harness implementation.
- Node modules installed for `tauri-app/src-ui`.
- No pending local changes that would interfere with test artifacts.

### 2.2 Mandatory Commands

Run from `tauri-app/src-ui`:

1. `npm run build`
2. `npx vitest run src/__tests__/App.browserFlow.test.tsx`
3. `npm run test:coverage`

Capture command outputs as evidence.

### 2.3 Dual-runtime mode declaration (required)

Before execution, record the runtime under test:
- `native` (Tauri desktop)
- `browser-harness` (browser parity mode)

Every evidence artifact must include the runtime mode in its filename or note text.

---

## 3) Executable Test Suites

## 3.1 Harness Guarding Suite

### HARN-001
- **Title**: Harness installs in browser context
- **Priority**: P0
- **Type**: Automated
- **Preconditions**: Browser context run (no native Tauri window globals)
- **Steps**:
  1. Launch app test run in jsdom/browser context.
  2. Trigger app render from `main.tsx`.
- **Expected Result**: Harness invoke/listen routes are available and app boot does not throw `invoke undefined`.
- **Evidence Required**: Passing test assertion and no startup error banner mentioning `invoke`.

### HARN-002
- **Title**: Harness does not override native runtime
- **Priority**: P0
- **Type**: Manual
- **Preconditions**: Native Tauri app running via `cargo tauri dev`
- **Steps**:
  1. Open app in native Tauri window.
  2. Verify normal app behavior (commands work, no harness-specific synthetic responses).
- **Expected Result**: Real backend commands execute; no browser harness behavior leaks into native run.
- **Evidence Required**: Screenshot and short note confirming native command behavior.

---

## 3.2 Birth Flow Suite

### FLOW-010
- **Title**: Splash to management transition
- **Priority**: P1
- **Type**: Automated
- **Steps**:
  1. Render app.
  2. Click `[skip]` on splash.
- **Expected Result**: Management screen appears with new entity birth controls.
- **Evidence Required**: Assertion for management text and input control.

### FLOW-011
- **Title**: Birth starts and reaches key presentation
- **Priority**: P0
- **Type**: Automated
- **Steps**:
  1. Enter entity name.
  2. Click `Birth`.
- **Expected Result**: Key presentation screen appears with signing-key warning content.
- **Evidence Required**: Assertion for `YOUR CONSTITUTIONAL SIGNING KEY`.

### FLOW-012
- **Title**: Key acknowledgement gate
- **Priority**: P0
- **Type**: Automated
- **Steps**:
  1. Try to continue without checking acknowledgment.
  2. Check acknowledgment box.
  3. Continue.
- **Expected Result**: Continue action is gated until checkbox is enabled; then transition to Ignition.
- **Evidence Required**: Button disabled/enabled evidence and stage assertion.

### FLOW-013
- **Title**: Ignition to Connectivity
- **Priority**: P0
- **Type**: Automated
- **Steps**:
  1. At Ignition, choose `Continue without model` (or equivalent deterministic path).
- **Expected Result**: Connectivity command center is displayed.
- **Evidence Required**: Assertion for `CONNECTIVITY COMMAND CENTER`.

### FLOW-014
- **Title**: Connectivity to Crystallization to Emergence to Chat
- **Priority**: P0
- **Type**: Automated
- **Steps**:
  1. Configure at least one provider.
  2. Click `ESTABLISH LINKAGE`.
  3. Select a path and complete crystallization.
  4. Begin emergence ceremony.
- **Expected Result**: App reaches primary chat interface.
- **Evidence Required**: Assertion for chat message input placeholder.

---

## 3.3 Provider Suite (OpenAI, Google, xAI)

### PROV-020
- **Title**: OpenAI key save + readiness
- **Priority**: P0
- **Type**: Automated
- **Steps**:
  1. Open OpenAI provider modal.
  2. Enter synthetic key.
  3. Save and validate.
- **Expected Result**: Modal closes; provider shows ready state.
- **Evidence Required**: UI ready indicator + modal close assertion.

### PROV-021
- **Title**: Google key save + readiness
- **Priority**: P0
- **Type**: Automated
- **Steps**: Same as PROV-020 for Google.
- **Expected Result**: Google provider ready state.
- **Evidence Required**: UI ready indicator assertion.

### PROV-022
- **Title**: xAI key save + readiness
- **Priority**: P0
- **Type**: Automated
- **Steps**: Same as PROV-020 for xAI.
- **Expected Result**: xAI provider ready state.
- **Evidence Required**: UI ready indicator assertion.

### PROV-023
- **Title**: Routing/provider indicator in chat
- **Priority**: P1
- **Type**: Automated
- **Steps**:
  1. Reach chat.
  2. Send normal prompt.
- **Expected Result**: Response includes deterministic provider/routing behavior expected by harness.
- **Evidence Required**: Response assertion (`Harness reply via ...` or configured equivalent).

---

## 3.4 Chat and Clipboard Suite

### CHAT-030
- **Title**: Normal chat turn stability
- **Priority**: P0
- **Type**: Automated
- **Steps**:
  1. Send standard prompt.
- **Expected Result**: Assistant response streams/renders correctly without UI breakage.
- **Evidence Required**: Final response assertion and no runtime error.

### CHAT-031
- **Title**: Clipboard skill request path
- **Priority**: P0
- **Type**: Automated
- **Steps**:
  1. Send prompt containing clipboard intent.
- **Expected Result**: Tool-status flow is emitted and response includes clipboard result string.
- **Evidence Required**: Assertion for clipboard result response text.

### CHAT-032
- **Title**: Post-send flicker regression guard
- **Priority**: P1
- **Type**: Manual (native and browser)
- **Steps**:
  1. Send a message.
  2. Observe UI while response streams.
- **Expected Result**: No rapid full-screen or message-list flicker.
- **Evidence Required**: Screen recording or timestamped observation note.

---

## 4) Regression and Native Parity

### REG-040
- **Title**: Targeted integrated test passes
- **Priority**: P0
- **Type**: Automated
- **Steps**: Run `npx vitest run src/__tests__/App.browserFlow.test.tsx`.
- **Expected Result**: 100% pass.
- **Evidence Required**: Command output.

### REG-041
- **Title**: Full frontend regression suite passes
- **Priority**: P0
- **Type**: Automated
- **Steps**: Run `npm run test:coverage`.
- **Expected Result**: All tests pass; coverage report generated.
- **Evidence Required**: Command output and coverage summary.

### REG-042
- **Title**: Typecheck and production build pass
- **Priority**: P0
- **Type**: Automated
- **Steps**: Run `npm run build`.
- **Expected Result**: No TypeScript/build errors.
- **Evidence Required**: Command output.

### NATIVE-050
- **Title**: Native startup parity
- **Priority**: P0
- **Type**: Manual
- **Steps**:
  1. Run `cargo tauri dev`.
  2. Open native app window.
  3. Verify startup and command calls work.
- **Expected Result**: Native app path works without harness dependency.
- **Evidence Required**: Screenshot + checklist note.

### NATIVE-051
- **Title**: Native chat + provider smoke
- **Priority**: P1
- **Type**: Manual
- **Steps**:
  1. Reach chat in native app.
  2. Send one normal prompt.
  3. Verify provider/routing details can be shown.
- **Expected Result**: Native behavior matches expected app design; no harness-only strings.
- **Evidence Required**: Screenshot and short transcript snippet.

### NATIVE-052
- **Title**: Runtime contract guardrail check
- **Priority**: P0
- **Type**: Manual
- **Steps**:
  1. In native run, verify no harness debug panel is active by default.
  2. In browser-harness run, verify runtime badge indicates `browser-harness`.
  3. Enable harness debug panel and verify snapshot/fault controls are present.
- **Expected Result**: Runtime separation is explicit and guardrails hold (no native override).
- **Evidence Required**: Native + browser screenshots and one note per step.

---

## 5) Evidence and Result Recording Rules

For each test case, store:
- **Execution timestamp** (local timezone)
- **Tester name**
- **Build/commit reference** (`git rev-parse --short HEAD`)
- **Result** (`Pass`/`Fail`/`Blocked`)
- **Evidence link**:
  - command output snippet, or
  - screenshot path, or
  - test assertion reference
- **Defect ID** (if failed), with severity and reproducibility.

Recommended result table columns:

`Test ID | Result | Evidence | Defect ID | Notes`

Failure handling:
- `Fail`: expected behavior not met, reproducible issue.
- `Blocked`: test cannot run due to environment/dependency issues.
- `Pass with risk`: acceptable only when explicitly approved; must include risk note.

---

## 6) Final Go / No-Go Rubric

Release recommendation is `Go` only if all are true:

1. All `P0` tests are `Pass`.
2. No unresolved `Blocked` in required suites.
3. `npm run build` passes.
4. `npm run test:coverage` passes.
5. Native parity smoke (`NATIVE-050`) passes.
6. Runtime guardrail check (`NATIVE-052`) passes.

Automatic `No-Go` conditions:
- Any `P0` failure.
- Harness active in native Tauri runtime.
- Reproducible post-send flicker in chat under normal flow.

Conditional `Go with Exceptions` (requires explicit sign-off):
- Only `P1/P2` failures remain.
- Workarounds documented.
- Follow-up issue owner + due date assigned.

---

## 7) Completion Checklist

- [ ] All suites executed in defined order.
- [ ] Result table completed for every test ID.
- [ ] Evidence artifacts attached and reachable.
- [ ] Defects triaged by severity (`Critical`, `High`, `Medium`, `Low`).
- [ ] Final recommendation recorded (`Go` / `No-Go` / `Go with Exceptions`).

---

## 8) Acceptance Gate Report Template

Use this table in final test reports:

`Gate | Required Tests | Status | Evidence | Owner`

Suggested gates:
- `Gate A: Harness Core` -> `HARN-001`
- `Gate B: Browser Capability Proof` -> `FLOW-011`, `FLOW-014`, `PROV-020..023`, `CHAT-030..031`
- `Gate C: Regression Health` -> `REG-040..042`
- `Gate D: Native Parity` -> `NATIVE-050..052`

