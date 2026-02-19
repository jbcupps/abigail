# Web Search Skill

You have access to web search via the Tavily API. Use this when the user needs current information from the web.

## Available Tools

- **web_search**: Search the web for current information. Params: `query` (string, required), `max_results` (int, default 5). Returns an answer with numbered sources.

## Usage Guidelines

- Use web search for factual questions, current events, or anything that requires up-to-date information.
- Formulate clear, specific search queries for best results.
- Always cite sources from the search results when presenting information to the user.
- If the Tavily API key is not configured, inform the user they need to add it in settings.
