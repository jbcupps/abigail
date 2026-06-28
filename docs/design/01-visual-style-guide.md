# Visual Style Guide

Abigail should look like a private home-control surface: quiet, confident, legible, and warm enough for non-technical family members. The default UI must not resemble an unstyled admin page, terminal dump, raw HTML form, or database registry.

## Brand Position

- Product name: `Abigail`
- Control-plane name: `Abigail Hive`
- Runtime surface name: `Entity Runtime`
- Primary promise: private family AI coordination
- Visual personality: calm, protective, precise, capable

## Logo and Wordmark

Use text wordmarks until a final logo asset exists.

- Primary wordmark: `Abigail`
- Control-plane wordmark: `Abigail Hive`
- Minimum wordmark size: 18 px text height in compact headers, 28 px in startup or empty states.
- Do not use all-caps wordmarks except short labels and diagnostics.
- Do not stretch, outline, glow, or skew the wordmark.
- Do not make `SOUL REGISTRY` the main product headline in family-facing UI. Use `Abigail Hive` and describe registry concepts in supporting UI.

Optional monogram:

```text
A
```

- Use only inside a 32 x 32 px or 40 x 40 px square/circle.
- Background: `--color-bg-elevated`.
- Foreground: `--color-primary`.
- Border: `1px solid --color-border`.

## Typography

Default family-facing typography uses Inter with system fallbacks. Monospace is reserved for IDs, logs, code, and short technical metadata.

| Token | Family | Use |
| --- | --- | --- |
| `--font-primary` | `Inter, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif` | UI, forms, navigation, body copy |
| `--font-mono` | `"JetBrains Mono", "Fira Code", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace` | IDs, logs, timestamps, code |

Type scale:

| Role | Size | Line-height | Weight | Tailwind example |
| --- | ---: | ---: | ---: | --- |
| Display | 32 px | 40 px | 650 | `text-[32px] leading-10 font-semibold` |
| Page title | 24 px | 32 px | 650 | `text-2xl leading-8 font-semibold` |
| Section title | 18 px | 28 px | 600 | `text-lg leading-7 font-semibold` |
| Body | 15 px | 24 px | 400 | `text-[15px] leading-6` |
| Body small | 14 px | 22 px | 400 | `text-sm leading-[22px]` |
| Label | 12 px | 16 px | 600 | `text-xs leading-4 font-semibold` |
| Metadata | 11 px | 16 px | 500 | `text-[11px] leading-4 font-mono` |

Rules:

- Letter spacing is `0` for body and controls.
- Uppercase labels may use `0.04em` letter spacing, max 16 characters when possible.
- Line-height must never be below `1.35`.
- Do not scale font size with viewport width.
- Do not use 10 px text for actions. The minimum action text size is 12 px.

## Color Palette

Modern Clean is the default product theme. This is the baseline for first-run and all family-facing screens.

### Modern Clean Tokens

| Token | Hex/RGB | Use |
| --- | --- | --- |
| `--color-bg` | `#0F1419` / `rgb(15 20 25)` | App background |
| `--color-bg-elevated` | `#1A2028` / `rgb(26 32 40)` | Shells, drawers, top bars |
| `--color-bg-inset` | `#0A0E12` / `rgb(10 14 18)` | Input wells, chat transcript wells |
| `--color-surface` | `rgba(30, 41, 59, 0.40)` | Cards and panels |
| `--color-surface-dim` | `rgba(30, 41, 59, 0.20)` | Subtle cards, empty states |
| `--color-surface-bright` | `rgba(30, 41, 59, 0.60)` | Hovered panels, active rows |
| `--color-border` | `#334155` / `rgb(51 65 85)` | Default border |
| `--color-border-dim` | `#1E293B` / `rgb(30 41 59)` | Quiet separators |
| `--color-text` | `#E2E8F0` / `rgb(226 232 240)` | Main text |
| `--color-text-bright` | `#F8FAFC` / `rgb(248 250 252)` | Titles and active text |
| `--color-text-dim` | `#94A3B8` / `rgb(148 163 184)` | Secondary text |
| `--color-primary` | `#6366F1` / `rgb(99 102 241)` | Primary action/accent |
| `--color-primary-dim` | `#818CF8` / `rgb(129 140 248)` | Accent text on dark |
| `--color-primary-muted` | `#4F46E5` / `rgb(79 70 229)` | Pressed/active accent |
| `--color-primary-faint` | `#3730A3` / `rgb(55 48 163)` | Low-emphasis accent border |
| `--color-success` | `#22C55E` / `rgb(34 197 94)` | Healthy state |
| `--color-warning` | `#F59E0B` / `rgb(245 158 11)` | Attention state |
| `--color-danger` | `#EF4444` / `rgb(239 68 68)` | Destructive/error state |
| `--color-info` | `#3B82F6` / `rgb(59 130 246)` | Informational state |
| `--color-focus-ring` | `rgba(99, 102, 241, 0.50)` | Keyboard focus |
| `--color-overlay` | `rgba(0, 0, 0, 0.60)` | Modal scrim |

State fills:

| Token | Value |
| --- | --- |
| `--color-primary-glow` | `rgba(99, 102, 241, 0.15)` |
| `--color-hover` | `rgba(99, 102, 241, 0.08)` |
| `--color-success-dim` | `rgba(34, 197, 94, 0.15)` |
| `--color-warning-dim` | `rgba(245, 158, 11, 0.15)` |
| `--color-danger-dim` | `rgba(239, 68, 68, 0.15)` |
| `--color-info-dim` | `rgba(59, 130, 246, 0.15)` |

### Theme Rules

- Default first-run theme: Modern Clean.
- Phosphor Terminal: allowed for developer diagnostics or a user-selected personality theme only.
- Classic Desktop: allowed only as an explicit nostalgic theme. Do not use it for default onboarding, registry, provider setup, or family-facing chat.
- Avoid one-note palettes. Screens need neutral foundations plus restrained accent and state colors.
- Do not use decorative gradient blobs, floating orbs, or bokeh backgrounds.

## Iconography

Preferred library: `lucide-react` when available. If not installed in a specific package, add it only with a focused UI task.

Icon sizes:

| Context | Size | Stroke |
| --- | ---: | ---: |
| Toolbar icon button | 18 px | 1.75 |
| Primary button leading icon | 16 px | 2 |
| Empty state | 32 px | 1.5 |
| Navigation rail | 20 px | 1.75 |
| Status dot | 8 px | n/a |

Rules:

- Icons must have accessible names when they are the only visible label.
- Icon-only buttons must be 36 x 36 px minimum on desktop and 44 x 44 px on touch layouts.
- Do not draw custom SVG icons when a lucide icon exists.
- Pair status color with text. Color alone is not state.

## Image and Avatar Style

Images should reveal a real product, person, place, or generated Entity identity. Avoid generic stock atmosphere.

Entity avatars:

- Size: 40 px in lists, 64 px in detail panels, 96 px in profile/edit screens.
- Shape: circle for Entity identity, 8 px rounded square for tools/apps.
- Border: `1px solid --color-border`.
- Fallback: first initial or monogram on `--color-bg-inset`.
- Do not crop faces tighter than forehead-to-chin plus 12 percent margin.

Screenshots:

- Use current UI only.
- Hide secrets, local file paths, tokens, and private chat content.
- Prefer 1440 x 900 desktop and 390 x 844 mobile captures.

## Surface, Radius, and Shadow

| Token | Value | Use |
| --- | ---: | --- |
| `--radius-sm` | 4 px | Inputs, small chips |
| `--radius-md` | 6 px | Buttons, compact controls |
| `--radius-lg` | 10 px | Cards, modals, drawers |
| `--shadow-elevated` | `0 4px 12px rgba(0, 0, 0, 0.30)` | Modal/card elevation |
| `--shadow-dropdown` | `0 8px 24px rgba(0, 0, 0, 0.40)` | Popovers |

Rules:

- Cards stay at 8 to 10 px radius. Do not use pill-shaped cards.
- Do not put cards inside cards.
- Page sections are unframed layout bands. Cards are for repeated items, modals, and framed tools.

## Visual Anti-Patterns

The screenshot that triggered this guide shows several forbidden states:

- Full-width raw select/status bars.
- Buttons rendered as default browser rectangles.
- Labels and metadata colliding with headings.
- All-caps headings used as the primary product brand.
- Unbounded rows and timestamps overflowing the viewport.
- Layout that depends on browser defaults instead of design tokens.

Every production screen must pass this quick visual test: if Tailwind failed to load, the screen should be obviously broken in QA, not quietly shippable.
