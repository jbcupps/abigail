# Abigail Design System

This directory is the source of truth for Abigail's product design standards. It exists because the UI must feel like a trusted family coordinator, not an exposed developer console.

## Required Guides

1. [Visual Style Guide](01-visual-style-guide.md)
2. [Component-Based Style Guide](02-component-style-guide.md)
3. [Interaction and Animation Style Guide](03-interaction-animation-style-guide.md)
4. [Accessibility Style Guide](04-accessibility-style-guide.md)
5. [Content and Writing Style Guide](05-content-writing-style-guide.md)
6. [Code Formatting and Conventions Style Guide](06-code-formatting-conventions-style-guide.md)
7. [Layout and Grid System Style Guide](07-layout-grid-system-style-guide.md)
8. [Security and Privacy Style Guide](08-security-privacy-style-guide.md)

## Product Principles

- Family-first, operator-capable: default surfaces are calm, readable, and helpful. Advanced controls exist, but they do not dominate the first screen.
- Hive stays present: model/provider management belongs to Abigail Hive, and the Hive shell remains visible while Entity work happens.
- No raw browser controls: every button, input, select, panel, dialog, table, and status area must use the system patterns in these guides.
- No accidental terminal UI: terminal, phosphor, and classic desktop treatments are allowed only as explicit themes or diagnostics, not as the default family experience.
- Local-first trust: the UI must make privacy, local storage, and user control obvious without turning every screen into a security lecture.

## Implementation Sources

The current token implementation lives in:

- `hive-app/src-ui/src/index.css`
- `entity-runtime-app/src-ui/src/index.css`
- `tauri-app/src-ui/src/index.css`
- each app's `tailwind.config.js`

When the docs and implementation disagree, fix the implementation or update this design system in the same pull request. Do not create a one-off component style to "just get the screen done."

## External Standards

- Accessibility baseline: [WCAG 2.2 AA](https://www.w3.org/TR/WCAG22/).
- Security baseline: [OWASP ASVS](https://owasp.org/www-project-application-security-verification-standard/), [OWASP Cheat Sheet Series](https://cheatsheetseries.owasp.org/), and Abigail's local-first privacy model.
- Privacy baseline: data minimization, purpose limitation, storage limitation, and user control.
