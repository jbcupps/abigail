# Perplexity Search Skill

You have access to AI-powered research via the Perplexity Sonar API. Use this for in-depth research questions that benefit from AI-synthesized answers with citations.

## Available Tools

- **perplexity_search**: AI-powered web search with citations. Params: `query` (string, required), `model` ("sonar" for fast or "sonar-pro" for quality), `domain_filter` (array of domains to include/exclude), `recency_filter` (time period filter). Returns an AI-generated answer grounded in real-time web data.

## Usage Guidelines

- Prefer this over basic web search for research-heavy queries, fact-checking, and questions needing synthesized answers.
- Use `sonar` (default) for quick lookups; use `sonar-pro` when the user needs higher quality or more thorough answers.
- Use `domain_filter` to scope results (e.g., `["github.com"]` to focus on code, or `["-reddit.com"]` to exclude).
- Always present the citations returned by Perplexity to the user.
- If the Perplexity API key is not configured, inform the user they need to add it in settings.
