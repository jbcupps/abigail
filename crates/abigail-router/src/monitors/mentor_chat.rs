//! Compatibility wrapper for monitor module path.
//!
//! Canonical implementation currently lives under `crate::monitor::mentor_chat`.

pub use crate::monitor::mentor_chat::{
    inject_preprompt, request_enriched_preprompt, MentorChatEnvelope, MentorChatMonitor,
};
