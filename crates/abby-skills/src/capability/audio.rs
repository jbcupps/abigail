//! Audio I/O capability traits (stubs).

use async_trait::async_trait;

use crate::SkillResult;

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
    async fn start(&mut self) -> SkillResult<()>;
    async fn stop(&mut self) -> SkillResult<()>;
}

#[async_trait]
pub trait SpeechRecognitionCapability: Send + Sync {
    async fn transcribe(&self, _audio: AudioData) -> SkillResult<Transcription> {
        Err(crate::SkillError::ToolFailed("stub".into()))
    }
}

#[async_trait]
pub trait AudioOutputCapability: Send + Sync {
    async fn speak(&self, _text: &str, _options: SpeakOptions) -> SkillResult<()> {
        Err(crate::SkillError::ToolFailed("stub".into()))
    }
}
