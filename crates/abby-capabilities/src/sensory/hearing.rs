//! Audio I/O capability traits (stubs).

use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct AudioInputInfo {
    pub id: String,
}

#[derive(Debug, Clone)]
pub struct AudioChunk;

#[derive(Debug, Clone)]
pub struct AudioData;

#[derive(Debug, Clone)]
pub struct Transcription {
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct TranscriptionChunk;

#[derive(Debug, Clone)]
pub struct SpeakOptions;

#[async_trait]
pub trait AudioInputCapability: Send + Sync {
    fn info(&self) -> AudioInputInfo;
    async fn start(&mut self) -> anyhow::Result<()>;
    async fn stop(&mut self) -> anyhow::Result<()>;
}

#[async_trait]
pub trait SpeechRecognitionCapability: Send + Sync {
    async fn transcribe(&self, _audio: AudioData) -> anyhow::Result<Transcription> {
        Err(anyhow::anyhow!("stub: not implemented"))
    }
}

#[async_trait]
pub trait AudioOutputCapability: Send + Sync {
    async fn speak(&self, _text: &str, _options: SpeakOptions) -> anyhow::Result<()> {
        Err(anyhow::anyhow!("stub: not implemented"))
    }
}
