//! Sensory capabilities — hearing (audio), vision (video), web search, browsing, HTTP.

pub mod browser;
pub mod hearing;
pub mod http_client;
pub mod url_security;
pub mod vision;
pub mod web_search;

pub use hearing::*;
pub use http_client::*;
pub use url_security::*;
pub use vision::*;
pub use web_search::*;
