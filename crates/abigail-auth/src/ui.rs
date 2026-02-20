use async_trait::async_trait;

use crate::error::Result;

/// Information needed to display a device-code prompt to the user.
#[derive(Debug, Clone)]
pub struct DeviceCodePrompt {
    /// URL the user should visit.
    pub verification_uri: String,
    /// Code the user should enter.
    pub user_code: String,
    /// How long until the code expires, in seconds.
    pub expires_in_secs: u64,
}

/// Trait for presenting auth-related UI to the user.
///
/// Implementations live in the Tauri frontend layer. Auth providers
/// call these methods when user interaction is needed (e.g. OAuth consent,
/// device code display).
#[async_trait]
pub trait AuthUI: Send + Sync {
    /// Open a URL in the user's browser for OAuth consent.
    async fn open_browser(&self, url: &str) -> Result<()>;

    /// Display a device code prompt and wait for the user to complete
    /// the out-of-band flow.
    async fn show_device_code(&self, prompt: &DeviceCodePrompt) -> Result<()>;

    /// Show a status/progress message during an auth flow.
    async fn show_status(&self, message: &str) -> Result<()>;
}
