# Browser Skill

The Browser skill now uses a persistent Playwright profile per Entity at `data/identities/<uuid>/browser_profile`, so cookies, OAuth sessions, and pre-logged states survive restarts.

## High-level tools

- `navigate`
- `click`
- `type`
- `screenshot`
- `execute_js`
- `login_with_oauth`

Compatibility aliases still exist for older flows:

- `browser_navigate`
- `browser_click`
- `browser_type_text`
- `browser_screenshot`
- `browser_evaluate_js`
- `browser_get_content`
- `browser_wait_for`
- `browser_get_url`
- `browser_get_title`
- `browser_back`
- `browser_forward`
- `browser_close`

## TriangleEthic preview

Before any mutating browser action, the first tool call returns:

- `status: triangle_ethic_preview_required`
- `triangle_ethic_preview`
- `triangle_ethic_token`

Replay the exact same tool call with `triangle_ethic_token` included to execute it.

Use that preview to confirm:

- the action belongs to the correct identity and site
- the scope is minimal
- the execution payload exactly matches what was previewed

## OAuth and auth-heavy flows

Use `login_with_oauth` when a site requires SSO, OAuth, or another browser-native auth path.

Recommended pattern:

1. Call `login_with_oauth` with `start_url`.
2. Include `success_url_contains` or `success_selector` when possible.
3. Reuse the same Entity later with normal browser tools; the persistent profile carries the session forward.

## Webmail fallback

Browser skill is the supported path for auth-heavy web workflows, including webmail.

Current provider heuristics:

- Gmail / Google Workspace webmail
- Outlook / Office 365 webmail

If the persistent browser profile is already signed in, the fallback can send without re-entering credentials.
