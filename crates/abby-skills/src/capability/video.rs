//! Video/vision capability traits (stubs).

use async_trait::async_trait;

use crate::SkillResult;

#[derive(Debug, Clone)]
pub struct CameraInfo;

#[derive(Debug, Clone)]
pub struct VideoFrame;

#[derive(Debug, Clone)]
pub struct ImageData;

#[derive(Debug, Clone)]
pub struct VisionResult;

#[derive(Debug, Clone)]
pub struct Detection;

#[derive(Debug, Clone)]
pub struct OcrResult;

#[async_trait]
pub trait VideoInputCapability: Send + Sync {
    fn cameras(&self) -> Vec<CameraInfo> {
        vec![]
    }
    async fn start(&mut self, _camera_id: Option<&str>) -> SkillResult<()> {
        Err(crate::SkillError::ToolFailed("stub".into()))
    }
    async fn capture_frame(&self) -> SkillResult<VideoFrame> {
        Err(crate::SkillError::ToolFailed("stub".into()))
    }
}

#[async_trait]
pub trait VisionCapability: Send + Sync {
    async fn analyze(&self, _image: ImageData, _prompt: &str) -> SkillResult<VisionResult> {
        Err(crate::SkillError::ToolFailed("stub".into()))
    }
}
