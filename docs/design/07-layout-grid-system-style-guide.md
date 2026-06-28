# Layout and Grid System Style Guide

Abigail layout should make repeated work easy: scan, compare, open, adjust, return.

## Spacing Scale

Use a 4 px base grid.

| Token | Value | Tailwind |
| --- | ---: | --- |
| 1 | 4 px | `1` |
| 2 | 8 px | `2` |
| 3 | 12 px | `3` |
| 4 | 16 px | `4` |
| 5 | 20 px | `5` |
| 6 | 24 px | `6` |
| 8 | 32 px | `8` |
| 10 | 40 px | `10` |
| 12 | 48 px | `12` |
| 16 | 64 px | `16` |

Rules:

- Compact control gap: 8 px.
- Form field gap: 6 to 8 px inside a field, 16 px between fields.
- Card grid gap: 12 px compact, 16 px standard.
- Screen padding: 16 px mobile, 24 px tablet, 32 px desktop.

## Breakpoints

Use Tailwind defaults unless product research requires otherwise.

| Name | Width | Layout |
| --- | ---: | --- |
| Mobile | `< 640px` | Single column, bottom/stacked actions |
| Small | `640px` | Two-column card grids when content allows |
| Medium | `768px` | Side panels can appear below or beside content |
| Large | `1024px` | Persistent Hive sidebar allowed |
| Extra large | `1280px` | Two-pane Hive + helper chat |
| 2XL | `1536px` | Wider grids, max content constraints |

## App Shell

Default Hive shell:

```text
+--------------------------------------------------------------+
| Top bar: Abigail Hive, status, primary action                |
+--------------------------------------+-----------------------+
| Main workspace                       | Optional helper/chat  |
| Entity grid, setup cards, panels     | Persistent side pane   |
+--------------------------------------+-----------------------+
```

Rules:

- Use `h-screen` for desktop app shells.
- Main workspace scrolls independently from persistent helper/chat.
- Helper pane width: 340 to 400 px.
- Top bar height: 56 px.
- Do not place global navigation in a raw full-width bordered strip.

## Grids

Entity card grid:

```tsx
<ul
  className="grid gap-4"
  style={{ gridTemplateColumns: "repeat(auto-fill, minmax(240px, 1fr))" }}
>
  {entities.map((entity) => <EntityCard key={entity.id} entity={entity} />)}
</ul>
```

Rules:

- Entity cards: min 220 px, preferred 240 to 320 px.
- Provider cards: min 260 px.
- Settings forms: max width 640 px.
- Chat transcript: max readable line length 760 px.
- Do not allow timestamps or IDs to stretch a row beyond the viewport. Truncate or wrap in a details view.

## Responsive Typography

Use fixed type sizes by role. Do not scale type with viewport width.

Mobile adjustments:

- Display becomes page title.
- Page title remains 24 px unless it wraps badly, then shorten copy.
- Body remains 15 px.
- Metadata remains 11 to 12 px but can move to a second line.

## Layout Patterns

### Setup Wizard

```text
+-------------------------------+
| Step title                    |
| Helpful sentence              |
|                               |
| [ Provider choice cards ]     |
|                               |
|              [Back] [Connect] |
+-------------------------------+
```

- Max width: 720 px.
- Actions align right on desktop, full-width stack on mobile.
- Progress indicator is text plus step dots, not a decorative progress bar.

### Entity Detail

```text
+----------------------------------------------+
| Avatar  Ada                 [Open Chat]      |
| Status, model, last activity                 |
+---------------------+------------------------+
| Memory summary      | Model/provider settings|
| Recent activity     | Safety/privacy controls|
+---------------------+------------------------+
```

- Two columns at `lg`.
- Single column below `lg`.
- Keep destructive controls in a separate section at the end.

### Chat Runtime

```text
+----------------------------------------------+
| Entity header, status, model                 |
+----------------------------------------------+
| Messages                                     |
|                                              |
+----------------------------------------------+
| Composer                                     |
+----------------------------------------------+
```

- Header fixed within the app shell.
- Composer fixed at bottom of chat panel.
- Messages use max width and side alignment.
- Streaming responses reserve space for the thinking indicator.

## Fixed Dimensions

Define stable dimensions for:

- Icon buttons.
- Avatars.
- Status chips.
- Toolbars.
- Chat composer.
- Navigation bars.
- Card action rows.

This prevents hover text, loading text, and dynamic labels from shifting layout.

## Empty, Error, and Loading Layouts

Empty state max width: 480 px.

```text
No Entities yet
Create the first Entity your family will talk to.
[Create Entity]
```

Error state:

- Keep the failed region's dimensions stable.
- Put retry action next to the message.
- Preserve any user-entered data.

Loading:

- Use skeletons matching final card dimensions.
- Do not show a full-screen loader when only a panel is refreshing.

## Mobile Rules

- Primary action appears near the related content, not only in the header.
- Bottom action bars are allowed for multi-step flows.
- Avoid hover-dependent controls.
- Use `min-h-[44px]` for touch controls.
- Drawers become full-screen sheets below `640px`.
