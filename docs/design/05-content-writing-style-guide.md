# Content and Writing Style Guide

Abigail's voice is calm, capable, and human. The UI should make the family head feel in control without sounding clinical or mystical.

## Voice

| Attribute | Write like this | Avoid |
| --- | --- | --- |
| Clear | "Connect a model" | "Configure provider substrate" |
| Reassuring | "Your data stays on this computer." | "No exfiltration detected." |
| Direct | "Create Entity" | "Proceed with entity instantiation" |
| Warm | "Name the Entity your family will talk to." | "Input entity identifier." |
| Honest | "The Hive is not responding yet." | "Something went wrong." |

## Terminology

Preferred terms:

- `Abigail`
- `Abigail Hive`
- `Entity`
- `family`
- `mentor` only when referring to the family head's elevated role
- `model`
- `provider`
- `local memory`
- `backup`
- `private key`

Avoid in primary UI:

- `soul registry` as the main screen label
- `birth` when a simpler `Create Entity` works
- `in-utero`
- `superego`
- `id`
- raw IDs unless in diagnostics or detail views

Developer/research terms can appear in advanced diagnostics, logs, and docs.

## Labels

Rules:

- Button labels start with verbs: `Create`, `Open`, `Connect`, `Retry`, `Back up`.
- Navigation labels are nouns: `Entities`, `Models`, `Memory`, `Settings`.
- Labels should be 1 to 3 words.
- Do not use punctuation in labels unless needed for clarity.

Examples:

| Good | Bad |
| --- | --- |
| `Create Entity` | `Birth New Entity` |
| `Connect a model` | `Configure Providers` |
| `Open Ada` | `Launch` |
| `Back up Entity` | `Recover entity from backup` as a primary CTA |

## Helper Text

Use helper text to reduce anxiety:

```text
Good:
Use the name your family will say naturally.

Bad:
Entity name...
```

Helper text rules:

- One sentence.
- No more than 120 characters.
- Explain why, not just what.
- Do not repeat the label.

## Error Messages

Error formula:

```text
What happened. What to do next.
```

Examples:

| Situation | Message |
| --- | --- |
| Hive unavailable | `The Hive is not responding yet. Retry in a moment.` |
| Empty Entity name | `Enter a name before creating an Entity.` |
| Provider key invalid | `That key was not accepted. Check it and try again.` |
| Backup restore failed | `The backup could not be restored. Choose another backup or keep the current Entity.` |
| Network unavailable | `Abigail cannot reach that provider right now. Local models are still available.` |

Security-sensitive errors:

- Do not reveal whether a secret exists.
- Do not print tokens, keys, local paths, or request payloads.
- Use a diagnostic code only when it maps to a safe troubleshooting page.

## Tooltips

Tooltips explain controls, not required information.

Rules:

- 1 sentence.
- Max 90 characters.
- Available on hover and focus.
- Never contain secrets or primary instructions.
- Never be the only way to learn a field requirement.

## Empty States

Pattern:

1. What is empty.
2. Why it matters.
3. One action.

Example:

```text
No Entities yet
Create the first Entity your family will talk to.
[Create Entity]
```

## Dates and Times

Family-facing:

- Recent: `2 minutes ago`, `Yesterday`, `Apr 13 at 9:55 PM`.
- Full timestamp only in diagnostics.

Diagnostics:

- ISO 8601 with timezone: `2026-04-13T21:55:29-04:00`.
- Use monospace and allow copy.

## Localization

- Keep strings in complete sentences.
- Avoid concatenating translated fragments.
- Avoid idioms that do not translate.
- Use ICU-style pluralization when localization is added.
- Do not hard-code date/time formats in components.

## Capitalization and Grammar

- Use sentence case for headings and buttons.
- Product names keep capitalization: `Abigail Hive`, `Entity Runtime`.
- Use contractions sparingly: `isn't` is okay in friendly status, but avoid in legal/security copy.
- Use `you` for the family head. Avoid `user` in UI.

## Privacy Copy

Privacy language should be factual and specific:

```text
Good:
This memory stays on this computer.

Bad:
Your data is totally safe.
```

Do not overpromise. If a cloud provider is involved, state that clearly:

```text
Messages sent with this model are processed by OpenAI. Abigail still stores memory locally.
```
