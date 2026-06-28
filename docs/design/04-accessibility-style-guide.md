# Accessibility Style Guide

Baseline: WCAG 2.2 AA. Abigail should be usable with keyboard, screen reader, magnification, reduced motion, and high-contrast preferences.

## Standards

Required external baseline:

- [WCAG 2.2 AA](https://www.w3.org/TR/WCAG22/).
- WCAG 2.2 contrast, keyboard, focus visible, focus not obscured, target size, status messages, and accessible authentication guidance.

## Color Contrast

Minimums:

- Body text: 4.5:1 contrast ratio.
- Large text, 18 px bold or 24 px regular: 3:1.
- Icons, focus indicators, input borders, and charts: 3:1 against adjacent colors.
- Disabled controls are exempt from contrast requirements but must remain recognizable.

Rules:

- Do not encode status with color alone.
- Text over images requires a stable overlay or solid background.
- Focus rings must be visible against both component and page backgrounds.

## Keyboard Navigation

Required behavior:

- Every interactive element is reachable by Tab or documented shortcut.
- Focus order follows visual order.
- No keyboard trap. Modals trap focus only while open and release focus on close.
- `Escape` closes menus, popovers, and non-destructive dialogs.
- Enter and Space activate buttons.
- Arrow keys navigate within menus, listboxes, radio groups, and tablists.

Testing checklist:

1. Load the app.
2. Use Tab from the top of the screen to the bottom.
3. Confirm the focus indicator is always visible.
4. Activate every control with keyboard only.
5. Open and close every dialog with keyboard only.
6. Confirm focus returns to the trigger.

## Semantic HTML

Use semantic elements before ARIA:

| UI | Element |
| --- | --- |
| Page/screen shell | `main` |
| Top navigation | `nav` |
| Repeated Entity cards | `ul` and `li` |
| Buttons | `button` |
| Links to URLs | `a` |
| Form field groups | `form`, `fieldset`, `legend` |
| Data comparison | `table` |
| Status text | `output`, `p role="status"` |

Bad:

```tsx
<div onClick={save}>Save</div>
```

Good:

```tsx
<button type="button" onClick={save}>Save</button>
```

## ARIA Rules

- Use ARIA only when native HTML cannot express the interaction.
- `aria-label` is allowed for icon-only controls.
- `aria-describedby` connects help and error text.
- `aria-live="polite"` for non-urgent async updates.
- `role="alert"` only for errors that immediately affect the user's task.
- Do not add `role="button"` to a `button`.
- Visible label and accessible name must match for voice control.

Example field:

```tsx
<label htmlFor="entity-name">Entity name</label>
<input
  id="entity-name"
  aria-describedby="entity-name-help entity-name-error"
  aria-invalid={Boolean(error)}
/>
<p id="entity-name-help">Use the name your family will say naturally.</p>
{error && <p id="entity-name-error" role="alert">{error}</p>}
```

## Target Size

- Preferred target: 44 x 44 px.
- Minimum desktop target: 36 x 36 px.
- Minimum touch target: 44 x 44 px.
- Inline text links may be smaller only inside prose and must have enough spacing from neighboring links.

## Text Readability

- Default body size: 15 px.
- Minimum UI text: 12 px.
- Paragraph line length: 45 to 80 characters.
- Avoid all-caps paragraphs.
- Keep labels concise and concrete.
- Support browser zoom up to 200 percent without clipping controls.

## Screen Reader Compatibility

Required patterns:

- Each screen has one `h1`.
- Heading levels do not skip for visual styling.
- Loading states announce the affected region.
- Toasts use `role="status"` or `role="alert"`.
- Entity cards expose name, status, and action labels.
- Tables have headers and captions when context is not obvious.

Entity card example:

```tsx
<li className="rounded-theme-lg border border-theme-border p-4">
  <h3 className="font-semibold text-theme-text-bright">Ada</h3>
  <p className="text-sm text-theme-text-dim">Ready. Last active 2 minutes ago.</p>
  <Button aria-label="Open Ada">Open</Button>
</li>
```

## Forms and Errors

- Show errors near the field and summarize at the top for multi-field forms.
- Error text must say what happened and how to fix it.
- Do not clear user input after validation fails.
- Required fields use visible text, not only an asterisk.
- Secret fields default to hidden text and have an accessible reveal control.

## Motion and Vestibular Safety

- Respect `prefers-reduced-motion`.
- Avoid parallax and zoom.
- No flashing more than 3 times per second.
- Loading animation must not be the only indication of progress.

## Accessibility Testing

Automated:

- React Testing Library queries by role/name.
- `@testing-library/jest-dom` assertions for disabled, invalid, and described states.
- Add axe checks when a browser/e2e test lane is established.

Manual:

- Keyboard-only pass.
- Windows Narrator or NVDA smoke pass.
- macOS VoiceOver smoke pass when macOS CI/dev is available.
- 200 percent zoom.
- Reduced motion.
- High contrast mode on Windows.

Definition of done:

- No raw clickable `div`.
- No unlabeled input.
- No icon-only button without an accessible name.
- No modal without focus management.
- No text below minimum contrast.
