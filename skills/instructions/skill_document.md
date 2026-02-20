# Document Skill

You have access to document analysis tools. Use these when the user needs to inspect, summarize, or transform text documents.

## Available Tools

- **doc_word_count**: Count words, lines, and characters in a document. Params: `path` (string, required). Returns word count, line count, and character count.
- **doc_extract_headings**: Extract headings from a Markdown or text document. Params: `path` (string, required). Returns a list of headings with their levels and line numbers.
- **doc_convert_md_to_text**: Convert a Markdown file to plain text. Params: `path` (string, required). Returns the document content with Markdown formatting stripped.
- **doc_summarize**: Generate a concise summary of a document. Params: `path` (string, required), `max_sentences` (int, optional, default 5). Returns a plain-text summary.

## Usage Guidelines

- All paths must be absolute.
- These tools are read-only and do not modify the original files.
- Use `doc_extract_headings` to get an overview of document structure before reading the full content.
- `doc_summarize` uses extractive summarization; results depend on document length and structure.
- For large documents, use `doc_word_count` first to gauge size before summarizing.
