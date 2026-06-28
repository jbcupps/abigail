# Component-Based Style Guide

Reusable components are the only acceptable way to build Abigail UI. One-off class strings are allowed only while extracting a component in the same pull request.

## Component Contract

Every reusable component must define:

- Purpose and allowed use.
- Props and event semantics.
- Loading, empty, error, disabled, hover, focus, active, and selected states.
- Keyboard behavior.
- Accessible name and role.
- Security/privacy notes when it touches user data.

## Buttons

### Variants

| Variant | Use | Base style |
| --- | --- | --- |
| Primary | Main forward action | Filled `--color-primary`, white text |
| Secondary | Alternate action | `--color-bg-elevated`, border |
| Ghost | Low-emphasis utility | Transparent, hover fill |
| Danger | Destructive | Danger border/fill, confirm when irreversible |
| Icon | Toolbar action | Square, icon centered, visible focus |

States:

- Hover: background shifts by one token step, no layout movement.
- Active: use `--color-primary-muted` or inset transform `translateY(1px)` only.
- Disabled: opacity `0.45`, cursor `not-allowed`, no tooltip-only explanation.
- Focus: `2px solid --color-focus-ring`, `outline-offset: 2px`.

Complete React example:

```tsx
import type { ButtonHTMLAttributes, ReactNode } from "react";

type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  icon?: ReactNode;
}

const variants: Record<ButtonVariant, string> = {
  primary:
    "bg-theme-primary text-white hover:bg-theme-primary-muted disabled:hover:bg-theme-primary",
  secondary:
    "border border-theme-border bg-theme-bg-elevated text-theme-text hover:border-theme-primary hover:bg-theme-hover",
  ghost:
    "text-theme-text-dim hover:bg-theme-hover hover:text-theme-text",
  danger:
    "border border-theme-danger text-theme-danger hover:bg-theme-danger-dim",
};

export function Button({
  variant = "secondary",
  icon,
  children,
  className = "",
  type = "button",
  ...props
}: ButtonProps) {
  return (
    <button
      type={type}
      className={[
        "inline-flex min-h-9 items-center justify-center gap-2 rounded-theme-md px-3 py-2",
        "text-sm font-medium transition-colors duration-theme-fast",
        "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-theme-focus-ring",
        "disabled:cursor-not-allowed disabled:opacity-45",
        variants[variant],
        className,
      ].join(" ")}
      {...props}
    >
      {icon && <span className="grid size-4 place-items-center" aria-hidden="true">{icon}</span>}
      {children}
    </button>
  );
}
```

Mockup:

```text
[ Connect a model ]  [ Secondary action ]  [ Delete ]
```

## Inputs and Forms

Rules:

- Every input has a visible label.
- Placeholder text is an example, never the only instruction.
- Error text appears below the field and is associated with `aria-describedby`.
- Inputs never span edge-to-edge on large desktop. Use max widths.
- Do not show secrets in plaintext by default.

Example:

```tsx
interface TextFieldProps {
  id: string;
  label: string;
  value: string;
  placeholder?: string;
  error?: string;
  onChange: (value: string) => void;
}

export function TextField({ id, label, value, placeholder, error, onChange }: TextFieldProps) {
  const errorId = `${id}-error`;

  return (
    <div className="grid gap-1.5">
      <label htmlFor={id} className="text-xs font-semibold text-theme-text">
        {label}
      </label>
      <input
        id={id}
        value={value}
        placeholder={placeholder}
        aria-invalid={Boolean(error)}
        aria-describedby={error ? errorId : undefined}
        onChange={(event) => onChange(event.target.value)}
        className={[
          "min-h-10 rounded-theme-md border bg-theme-input-bg px-3 py-2 text-sm text-theme-text",
          "placeholder:text-theme-text-dim focus:border-theme-primary focus:outline-none",
          error ? "border-theme-danger" : "border-theme-border",
        ].join(" ")}
      />
      {error && <p id={errorId} className="text-xs text-theme-danger">{error}</p>}
    </div>
  );
}
```

## Navigation Bars and Rails

Use horizontal navigation for top-level Hive tasks and a side rail for persistent tools.

Top bar anatomy:

```text
+----------------------------------------------------------------+
| Abigail Hive        Status: Local model ready     [Connect]     |
+----------------------------------------------------------------+
```

Rules:

- Height: 56 px desktop, 52 px compact.
- Padding: 24 px desktop, 16 px tablet, 12 px mobile.
- Status indicators live on the right, not in a raw select-style strip.
- The active item must use both text weight and background/border state.

## Modals and Drawers

Use modals for blocking decisions and drawers for setup/configuration.

Required behavior:

- `role="dialog"` and `aria-modal="true"`.
- Label with `aria-labelledby`.
- Focus moves into the dialog on open and returns to the trigger on close.
- `Escape` closes unless the action is destructive or mid-save.
- Background scroll is locked.
- Destructive confirmation names the object being changed.

Example shell:

```tsx
export function ConfirmDeleteDialog({
  entityName,
  onCancel,
  onConfirm,
}: {
  entityName: string;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-theme-overlay p-4">
      <section
        role="dialog"
        aria-modal="true"
        aria-labelledby="delete-title"
        className="w-full max-w-md rounded-theme-lg border border-theme-border bg-theme-bg-elevated p-5 shadow-theme-elevated"
      >
        <h2 id="delete-title" className="text-lg font-semibold text-theme-text-bright">
          Delete {entityName}?
        </h2>
        <p className="mt-2 text-sm text-theme-text-dim">
          This removes the local Entity profile from this Hive. Back up anything you need first.
        </p>
        <div className="mt-5 flex justify-end gap-2">
          <Button variant="secondary" onClick={onCancel}>Cancel</Button>
          <Button variant="danger" onClick={onConfirm}>Delete Entity</Button>
        </div>
      </section>
    </div>
  );
}
```

## Dropdowns and Menus

Rules:

- Use native `select` for simple forms with fewer than 8 options.
- Use custom menu buttons only when options need descriptions, icons, or grouping.
- Menu opens on Enter, Space, or ArrowDown.
- Menu items use `role="menuitem"` only when the widget behaves like an app menu. For list selection, use listbox semantics.
- Width must fit the longest label without truncating critical text.

## Tables and Data Lists

Use tables only for dense comparison. Use cards/lists for Entities and setup tasks.

Table requirements:

- Sticky header when vertical scrolling.
- Column headers use `scope="col"`.
- Numeric values align right.
- Row actions live in the final column.
- Empty state explains what will appear and how to create the first item.

Example:

```tsx
<table className="w-full border-separate border-spacing-0 text-sm">
  <thead className="sticky top-0 bg-theme-bg-elevated">
    <tr>
      <th scope="col" className="border-b border-theme-border px-3 py-2 text-left">Provider</th>
      <th scope="col" className="border-b border-theme-border px-3 py-2 text-left">Status</th>
      <th scope="col" className="border-b border-theme-border px-3 py-2 text-right">Actions</th>
    </tr>
  </thead>
  <tbody>{rows}</tbody>
</table>
```

## Cards

Use cards for repeated Entities, provider choices, backups, and skills.

Rules:

- Radius: `--radius-lg`.
- Padding: 16 px compact, 20 px standard.
- Border always present, even when subtle.
- Whole-card click is allowed only when there are no nested buttons. Otherwise use explicit action buttons.
- No cards inside cards.

Entity card mockup:

```text
+------------------------------+
| A  Ada                       |
|    Ready - local memory      |
|                              |
| [Open]       Last active 2m  |
+------------------------------+
```

## Toasts and Status Messages

Use toasts for temporary feedback and inline status for persistent conditions.

Rules:

- Toast timeout: 5 seconds for success/info, persistent for errors requiring action.
- Toasts use `role="status"` unless urgent, then `role="alert"`.
- Never show secrets or full file paths in toast text.

## Implementation Best Practices

- Prefer component props over duplicated class strings.
- Use semantic HTML first, ARIA second.
- Keep components controlled from the parent for async operations.
- Create Storybook-like documentation later using the examples in this guide as fixtures.
- Add tests for keyboard behavior and accessible names when creating reusable components.
