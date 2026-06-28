# Interaction and Animation Style Guide

Abigail interactions should feel responsive and steady. Motion is used to explain state, not to decorate the screen.

## Timing Tokens

| Token | Duration | Use |
| --- | ---: | --- |
| Instant | 0 ms | Reduced motion, terminal/classic themes |
| Fast | 120-150 ms | Hover, focus, button active |
| Normal | 200-250 ms | Drawer/modal fade, card selection |
| Slow | 320-400 ms | Startup transitions, non-blocking panel entrance |
| Loading loop | 900-1400 ms | Skeleton shimmer, thinking dots |

Current CSS tokens:

```css
--transition-fast: 150ms;
--transition-normal: 250ms;
```

## Easing

Use these curves consistently:

| Name | Curve | Use |
| --- | --- | --- |
| Standard | `cubic-bezier(0.2, 0, 0, 1)` | Most UI transitions |
| Emphasized out | `cubic-bezier(0.16, 1, 0.3, 1)` | Drawer/modal entrance |
| Emphasized in | `cubic-bezier(0.7, 0, 0.84, 0)` | Drawer/modal exit |
| Linear | `linear` | Progress bars only |

Do not use spring/bouncy motion for privacy, security, or error states.

## Hover and Press States

Buttons:

- Hover: color/background shift only.
- Active: optional `transform: translateY(1px)`.
- Disabled: no hover transform.

Cards:

- Hover border shifts to `--color-primary` only when the card is clickable.
- Non-clickable cards must not react on hover.
- Do not animate card dimensions.

Inputs:

- Focus changes border and focus outline.
- Validation messages appear immediately after blur or submit, not on every keystroke unless the field has already errored.

## Loading Indicators

Use the smallest indicator that explains the wait.

| Wait | Pattern |
| --- | --- |
| Under 500 ms | No loader |
| 500 ms to 2 s | Inline spinner or disabled button text |
| 2 s to 8 s | Skeleton or progress text |
| Over 8 s | Progress state with cancel/retry when possible |

Button loading example:

```tsx
<Button disabled={busy} aria-busy={busy}>
  {busy ? "Creating..." : "Create Entity"}
</Button>
```

Skeleton example:

```tsx
<div className="grid gap-3" aria-label="Loading Entities">
  {[0, 1, 2].map((item) => (
    <div
      key={item}
      className="h-24 animate-pulse rounded-theme-lg border border-theme-border-dim bg-theme-surface-dim"
    />
  ))}
</div>
```

## User Feedback

- Success: quiet inline confirmation or toast.
- Warning: inline banner with one clear action.
- Error: plain-language cause plus recovery action.
- Destructive: confirmation dialog naming the object.
- Long-running background work: persistent status region, not a blocking modal.

ARIA:

- Non-urgent status: `role="status"`.
- Error that blocks progress: `role="alert"`.
- Loading regions: `aria-busy="true"` on the affected region.

## Scroll Behavior

- Main app shell owns page scroll.
- Drawers and modals have internal scroll only when content exceeds viewport.
- Preserve scroll position when refreshing data.
- Use `scroll-margin-top: 80px` for anchored sections under sticky headers.
- Do not scroll the page on validation unless the errored field is outside the viewport.

## Micro-Interactions

Recommended:

- Provider connection test: inline status changes from `Checking...` to `Connected`.
- Entity creation: disable button, then add the new Entity card without a full page reload.
- Chat send: message appears immediately with pending state, then resolves or shows retry.
- Secret entry: reveal button is momentary or toggled with clear label.

Forbidden:

- Flashing borders.
- Infinite glow around important actions.
- Motion that is required to understand state.
- Hover-only controls.
- Auto-advancing carousels.

## Reduced Motion

Every motion pattern must respect user preference:

```css
@media (prefers-reduced-motion: reduce) {
  *,
  *::before,
  *::after {
    animation-duration: 0.01ms !important;
    animation-iteration-count: 1 !important;
    scroll-behavior: auto !important;
    transition-duration: 0.01ms !important;
  }
}
```

## Implementation Guidelines

- Use CSS transitions for simple state changes.
- Use React state for semantic states, not for decorative animation timers.
- Keep animations interruptible. If state changes, the UI must settle quickly.
- Use `transform` and `opacity` when animating; avoid animating layout properties.
- Test with keyboard navigation and reduced motion enabled.
