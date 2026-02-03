pub mod candle;
pub mod download;
pub mod local_http;
pub mod openai;
pub mod provider;

pub use candle::CandleProvider;
pub use download::ModelDownloader;
pub use local_http::{stub_heartbeat, LocalHttpProvider};
pub use openai::OpenAiProvider;
pub use provider::{CompletionRequest, CompletionResponse, LlmProvider, Message};
