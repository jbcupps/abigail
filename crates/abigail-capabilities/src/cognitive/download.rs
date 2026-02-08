//! Model downloader: fetches Phi-3-mini GGUF from HuggingFace with progress callback.

use std::path::Path;

const PHI3_GGUF_URL: &str = "https://huggingface.co/microsoft/Phi-3-mini-4k-instruct-gguf/resolve/main/Phi-3-mini-4k-instruct-q4.gguf";

pub struct ModelDownloader;

impl ModelDownloader {
    pub fn new() -> Self {
        Self
    }

    /// Download the model to `dir`, calling `on_progress(bytes_done, total_bytes)` when known.
    /// Total may be None if Content-Length is missing.
    pub async fn download_to(
        &self,
        dir: &Path,
        mut on_progress: impl FnMut(u64, Option<u64>) + Send,
    ) -> anyhow::Result<std::path::PathBuf> {
        std::fs::create_dir_all(dir)?;
        let filename = Path::new(PHI3_GGUF_URL)
            .file_name()
            .and_then(|p| p.to_str())
            .unwrap_or("model.gguf");
        let dest = dir.join(filename);

        let client = reqwest::Client::new();
        let res = client.get(PHI3_GGUF_URL).send().await?;
        let total = res.content_length();
        let mut stream = res.bytes_stream();
        let mut file = tokio::fs::File::create(&dest).await?;
        let mut written: u64 = 0;

        use futures_util::StreamExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            let n = chunk.len() as u64;
            tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await?;
            written += n;
            on_progress(written, total);
        }

        Ok(dest)
    }
}

impl Default for ModelDownloader {
    fn default() -> Self {
        Self::new()
    }
}
