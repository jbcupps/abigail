//! Video/vision capability traits (stubs).

use async_trait::async_trait;

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
    async fn start(&mut self, _camera_id: Option<&str>) -> anyhow::Result<()> {
        Err(anyhow::anyhow!("stub: not implemented"))
    }
    async fn capture_frame(&self) -> anyhow::Result<VideoFrame> {
        Err(anyhow::anyhow!("stub: not implemented"))
    }
}

#[async_trait]
pub trait VisionCapability: Send + Sync {
    async fn analyze(&self, _image: ImageData, _prompt: &str) -> anyhow::Result<VisionResult> {
        Err(anyhow::anyhow!("stub: not implemented"))
    }
}
