# Code Formatting and Conventions Style Guide

This guide covers UI-facing code conventions across `hive-app`, `entity-runtime-app`, and `tauri-app`.

## Stack

- React with TypeScript.
- Vite.
- Tailwind CSS using Abigail theme tokens.
- Rust/Tauri for desktop shell and daemon integration.
- React Testing Library for UI behavior tests.

## Formatting

TypeScript/React:

- Indentation: 2 spaces.
- Quotes: double quotes.
- Semicolons: required.
- Component files: `.tsx`.
- Utility files: `.ts`.
- Max practical line length: 100 characters, but do not contort JSX.

Rust:

- `cargo fmt --all`.
- `cargo clippy --workspace --exclude abigail-app -- -D warnings` for CI-style checks.

CSS:

- Prefer Tailwind utilities and token classes.
- Use CSS files for tokens, global reset, keyframes, and reusable component layers only.

## Naming

| Item | Convention | Example |
| --- | --- | --- |
| React component | PascalCase | `ProviderWizard` |
| Hook | camelCase starting with `use` | `useHiveStatus` |
| Event handler | `handle` + action | `handleCreateEntity` |
| Boolean prop | `is`, `has`, `can`, `should` | `isOpen`, `canRetry` |
| Async state | noun + `Status` or `State` | `connectionStatus` |
| Test file | component + `.test.tsx` | `CreateEntityCard.test.tsx` |

Avoid abbreviations except well-known terms such as `id`, `url`, `api`, and `ui`.

## File Structure

Preferred UI structure:

```text
src/
  components/
    Button.tsx
    Dialog.tsx
    EntityCard.tsx
  lib/
    daemonClient.ts
    window.ts
  chat/
    chatGateway.ts
  test/
    setup.ts
  App.tsx
  main.tsx
  index.css
```

Rules:

- Shared components should move into a local `components/` folder before duplication spreads.
- Keep daemon/API clients in `lib/`, not inside components.
- Components should not import from sibling app roots.
- If all three UI apps need a component, create a shared package in a dedicated follow-up rather than copy-pasting.

## Imports

Order:

1. React and package imports.
2. Internal components.
3. Internal libraries.
4. Types.
5. CSS, if any.

Example:

```tsx
import { useCallback, useState } from "react";
import { Button } from "./Button";
import { createEntity } from "../lib/daemonClient";
import type { EntitySummary } from "../lib/daemonClient";
```

## Component Practices

Rules:

- Prefer function components.
- Keep components under roughly 200 lines. Extract subcomponents when behavior becomes hard to scan.
- Keep async side effects in named functions or hooks.
- Use `type` or `interface` for props next to the component unless shared.
- Always specify `type="button"` for non-submit buttons.
- Do not use array indexes as keys for dynamic data.
- Do not use inline styles except for safe dynamic values such as an Entity accent color after validation.

Good:

```tsx
<button type="button" onClick={handleOpen} className="rounded-theme-md ...">
  Open
</button>
```

Bad:

```tsx
<button onClick={() => openEntity(entity.id)}>Open</button>
```

## Tailwind Rules

- Use theme tokens: `bg-theme-bg`, `text-theme-text`, `border-theme-border`.
- Do not hard-code arbitrary colors in JSX except temporary prototypes.
- Keep class order roughly layout, size, spacing, border, color, typography, state.
- Extract repeated patterns after the third use.
- Do not use negative letter spacing.
- Do not use viewport-width font sizes.

## Comments and Documentation

Use comments for:

- Non-obvious async or security behavior.
- Browser/Tauri integration quirks.
- Accessibility decisions that are easy to break.

Avoid comments that restate JSX.

Good:

```tsx
// The daemon may still be warming up after the shell appears, so retry briefly
// before showing the user an error state.
```

Bad:

```tsx
// Set busy to true.
setBusy(true);
```

## Error Handling

- Catch async errors at the UI boundary and display a safe message.
- Preserve the detailed error for local diagnostics only when it does not contain secrets.
- Never `alert()` in production UI.
- Do not log tokens, prompts, private keys, file contents, or full local paths.

## Testing

Required for reusable components:

- Renders with accessible name.
- Disabled/loading behavior.
- Keyboard behavior when interactive.
- Error/empty state.
- Critical privacy behavior such as hidden secrets.

Example:

```tsx
it("disables create while the name is empty", () => {
  render(<CreateEntityCard onCreated={vi.fn()} />);
  expect(screen.getByRole("button", { name: /create/i })).toBeDisabled();
});
```

## Tooling

Current validation commands:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --exclude abigail-app -- -D warnings
cargo test --workspace --exclude abigail-app
npm test
npm run build
```

Recommended next additions:

- ESLint with `eslint-plugin-jsx-a11y`.
- Prettier for TypeScript/Markdown.
- Stylelint for token-only CSS rules.
- Axe checks in browser/e2e tests.
