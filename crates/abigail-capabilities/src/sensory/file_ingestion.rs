//! File ingestion — parse uploaded files into text content for LLM context.

use std::path::Path;

/// Result of ingesting a file.
#[derive(Debug, Clone)]
pub struct IngestionResult {
    /// Extracted text content.
    pub content: String,
    /// Detected content type / MIME.
    pub content_type: String,
    /// Original filename.
    pub filename: String,
    /// File size in bytes.
    pub size_bytes: u64,
    /// Whether the content was truncated.
    pub truncated: bool,
}

/// Maximum content size before truncation (100KB of text).
const MAX_CONTENT_SIZE: usize = 100_000;

/// Ingest a file and extract its text content for LLM context.
pub fn ingest_file(path: &Path) -> anyhow::Result<IngestionResult> {
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".into());

    let metadata = std::fs::metadata(path)?;
    let size_bytes = metadata.len();

    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    let content_type = detect_content_type(&extension);

    let (content, truncated) = match content_type.as_str() {
        "text/plain" | "text/markdown" | "text/csv" => read_text_file(path)?,
        "application/json" | "text/xml" | "text/html" => read_text_file(path)?,
        "text/x-code" => read_text_file(path)?,
        "image/png" | "image/jpeg" | "image/gif" | "image/webp" => {
            // For images, return base64-encoded content for vision-capable LLMs
            let bytes = std::fs::read(path)?;
            let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
            let truncated = b64.len() > MAX_CONTENT_SIZE;
            let content = if truncated {
                format!(
                    "[Image: {} ({} bytes) — too large for inline display]",
                    filename, size_bytes
                )
            } else {
                format!("data:{};base64,{}", content_type, b64)
            };
            (content, truncated)
        }
        "application/zip" | "application/x-tar" | "application/gzip" => {
            // For archives, just describe them
            (
                format!(
                    "[Archive: {} ({} bytes) — extract and re-upload individual files]",
                    filename, size_bytes
                ),
                false,
            )
        }
        _ => {
            // Try reading as text, fall back to description
            match read_text_file(path) {
                Ok(result) => result,
                Err(_) => (
                    format!(
                        "[Binary file: {} ({} bytes, type: {})]",
                        filename, size_bytes, content_type
                    ),
                    false,
                ),
            }
        }
    };

    Ok(IngestionResult {
        content,
        content_type,
        filename,
        size_bytes,
        truncated,
    })
}

/// Read a text file with truncation support.
fn read_text_file(path: &Path) -> anyhow::Result<(String, bool)> {
    let content = std::fs::read_to_string(path)?;
    if content.len() > MAX_CONTENT_SIZE {
        let truncated = content[..MAX_CONTENT_SIZE].to_string();
        Ok((
            format!(
                "{}\n\n[... truncated, {} bytes total]",
                truncated,
                content.len()
            ),
            true,
        ))
    } else {
        Ok((content, false))
    }
}

/// Detect content type from file extension.
fn detect_content_type(extension: &str) -> String {
    match extension {
        // Text
        "txt" | "text" => "text/plain".into(),
        "md" | "markdown" => "text/markdown".into(),
        "csv" => "text/csv".into(),
        "json" => "application/json".into(),
        "xml" => "text/xml".into(),
        "html" | "htm" => "text/html".into(),

        // Code files
        "rs" | "py" | "js" | "ts" | "tsx" | "jsx" | "go" | "java" | "c" | "cpp" | "h" | "hpp"
        | "rb" | "php" | "swift" | "kt" | "scala" | "sh" | "bash" | "zsh" | "fish" | "ps1"
        | "bat" | "cmd" | "sql" | "r" | "lua" | "zig" | "nim" | "toml" | "yaml" | "yml" | "ini"
        | "cfg" | "conf" | "env" | "dockerfile" | "makefile" | "cmake" => "text/x-code".into(),

        // Images
        "png" => "image/png".into(),
        "jpg" | "jpeg" => "image/jpeg".into(),
        "gif" => "image/gif".into(),
        "webp" => "image/webp".into(),
        "svg" => "image/svg+xml".into(),

        // Archives
        "zip" => "application/zip".into(),
        "tar" => "application/x-tar".into(),
        "gz" | "tgz" => "application/gzip".into(),

        // PDF
        "pdf" => "application/pdf".into(),

        _ => "application/octet-stream".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_content_type() {
        assert_eq!(detect_content_type("rs"), "text/x-code");
        assert_eq!(detect_content_type("txt"), "text/plain");
        assert_eq!(detect_content_type("png"), "image/png");
        assert_eq!(detect_content_type("zip"), "application/zip");
        assert_eq!(detect_content_type("xyz"), "application/octet-stream");
    }

    #[test]
    fn test_ingest_text_file() {
        let tmp = std::env::temp_dir().join("abigail_ingest_test.txt");
        std::fs::write(&tmp, "Hello, world!").unwrap();

        let result = ingest_file(&tmp).unwrap();
        assert_eq!(result.content, "Hello, world!");
        assert_eq!(result.content_type, "text/plain");
        assert!(!result.truncated);

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_ingest_code_file() {
        let tmp = std::env::temp_dir().join("abigail_ingest_test.rs");
        std::fs::write(&tmp, "fn main() { println!(\"hello\"); }").unwrap();

        let result = ingest_file(&tmp).unwrap();
        assert_eq!(result.content_type, "text/x-code");
        assert!(result.content.contains("fn main()"));

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_ingest_nonexistent_file() {
        let result = ingest_file(Path::new("/nonexistent/file.txt"));
        assert!(result.is_err());
    }
}
