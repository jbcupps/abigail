# Browser Skill

You have access to browser automation tools. Use these when the user needs to interact with web pages, scrape content, or perform automated browsing tasks.

## Available Tools

- **browser_navigate**: Navigate to a URL. Params: `url` (string, required). Opens the page and waits for it to load.
- **browser_get_content**: Get the text content of the current page. Returns the visible text extracted from the DOM.
- **browser_screenshot**: Capture a screenshot of the current page. Returns an image of the viewport.
- **browser_click**: Click an element on the page. Params: `selector` (string, required, CSS selector). Clicks the first matching element.
- **browser_type_text**: Type text into the focused element. Params: `text` (string, required). Simulates keyboard input.
- **browser_fill_form**: Fill a form field by selector. Params: `selector` (string, required), `value` (string, required). Sets the value of the matched input element.
- **browser_wait_for**: Wait for an element to appear. Params: `selector` (string, required), `timeout_ms` (int, optional, default 5000). Blocks until the element is present in the DOM.
- **browser_evaluate_js**: Execute JavaScript in the page context. Params: `script` (string, required). Returns the evaluation result as a string.
- **browser_get_url**: Get the current page URL. Returns the full URL string.
- **browser_get_title**: Get the current page title. Returns the document title.
- **browser_back**: Navigate back one page in browser history.
- **browser_forward**: Navigate forward one page in browser history.
- **browser_close**: Close the browser session and release resources.

## Usage Guidelines

- Always call `browser_navigate` before attempting to interact with page elements.
- Use `browser_get_content` to read page text rather than parsing screenshots.
- Use `browser_wait_for` before clicking or filling elements that may load asynchronously.
- Prefer `browser_fill_form` over `browser_click` + `browser_type_text` for form inputs.
- Always call `browser_close` when finished to free resources.
- JavaScript evaluation via `browser_evaluate_js` should be used sparingly and only when dedicated tools are insufficient.
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
