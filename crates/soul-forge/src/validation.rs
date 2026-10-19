//! Bounded validation of untrusted Forge proposals. No scores or permissions
//! are accepted here; successful validation is not fitness evaluation/adoption.

use crate::{superego_scan, ForgeRequestEnvelope};
use serde::{Deserialize, Serialize};

pub const MAX_FORGE_REQUEST_BYTES: usize = 128 * 1024;
const MAX_CODE_BYTES: usize = 64 * 1024;
const MAX_MARKDOWN_BYTES: usize = 32 * 1024;
const MAX_METADATA_BYTES: usize = 128;
const MAX_TAGS: usize = 32;

fn valid_reference(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_METADATA_BYTES
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

/// Routing is independent of proposal acceptance. Prefer an unambiguous valid
/// body ID, then a valid transport header. Neither conveys authority, and neither
/// belongs in diagnostics. Malformed/oversized bodies are not partially recovered.
pub(crate) fn response_correlation_id(payload: &[u8], header: Option<&str>) -> Option<String> {
    #[derive(Deserialize)]
    struct RoutingMetadata {
        // Ignore other fields only in this routing projection. The proposal
        // parser still rejects unknown/duplicate fields and invalid content.
        // Serde rejects duplicate correlation_id keys, including escaped aliases.
        #[serde(default)]
        correlation_id: Option<String>,
    }

    let body_id = if payload.len() <= MAX_FORGE_REQUEST_BYTES
        && payload.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'{')
    {
        serde_json::from_slice::<RoutingMetadata>(payload)
            .ok()
            .and_then(|metadata| metadata.correlation_id)
            .filter(|id| valid_reference(id))
    } else {
        None
    };
    body_id.or_else(|| header.filter(|id| valid_reference(id)).map(str::to_string))
}

/// Fixed host-owned codes. Never include parser messages or proposal content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForgeDiagnosticCode {
    RequestTooLarge,
    IncompleteJson,
    InvalidJson,
    InvalidSchema,
    InvalidFields,
    DestructivePattern,
    RemoteExecutionPattern,
    ConstitutionalDocumentTarget,
    ValidatedNotApplied,
}

impl ForgeDiagnosticCode {
    pub(crate) fn rule(self) -> Option<&'static str> {
        match self {
            Self::DestructivePattern => Some("destructive-pattern"),
            Self::RemoteExecutionPattern => Some("remote-code-exec-pattern"),
            Self::ConstitutionalDocumentTarget => Some("constitutional-doc-target"),
            _ => None,
        }
    }
    pub(crate) fn status(self) -> &'static str {
        match self {
            Self::ValidatedNotApplied => "validated",
            Self::DestructivePattern
            | Self::RemoteExecutionPattern
            | Self::ConstitutionalDocumentTarget => "blocked",
            _ => "error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeDiagnostic {
    pub code: ForgeDiagnosticCode,
    /// Saturates at the request limit plus one; no raw response is retained.
    pub payload_bytes: usize,
}

impl ForgeDiagnostic {
    pub(crate) fn new(code: ForgeDiagnosticCode, payload_bytes: usize) -> Self {
        Self {
            code,
            payload_bytes: payload_bytes.min(MAX_FORGE_REQUEST_BYTES + 1),
        }
    }
}

/// Parse exactly one JSON object. Do not extract JSON from prose or fences,
/// recover truncated output, or allow model-supplied authority/scoring fields.
pub fn parse_forge_request(bytes: &[u8]) -> Result<ForgeRequestEnvelope, ForgeDiagnosticCode> {
    if bytes.len() > MAX_FORGE_REQUEST_BYTES {
        return Err(ForgeDiagnosticCode::RequestTooLarge);
    }
    let request = serde_json::from_slice::<ForgeRequestEnvelope>(bytes).map_err(|err| match err
        .classify()
    {
        serde_json::error::Category::Eof => ForgeDiagnosticCode::IncompleteJson,
        serde_json::error::Category::Data => ForgeDiagnosticCode::InvalidSchema,
        _ => ForgeDiagnosticCode::InvalidJson,
    })?;
    // Derived struct deserializers also accept positional arrays; the wire
    // contract permits an object only, even with the correct field order/types.
    if bytes.iter().find(|b| !b.is_ascii_whitespace()) != Some(&b'{') {
        return Err(ForgeDiagnosticCode::InvalidSchema);
    }
    validate_forge_request(&request)?;
    Ok(request)
}

/// Also used by the trusted direct writer, so typed callers cannot skip bounds
/// or existing static safety gates. This is not an arbitrary-code safety proof.
pub fn validate_forge_request(request: &ForgeRequestEnvelope) -> Result<(), ForgeDiagnosticCode> {
    fn text(value: &str, limit: usize) -> bool {
        !value.trim().is_empty() && value.len() <= limit
    }
    fn metadata(value: &str) -> bool {
        text(value, MAX_METADATA_BYTES) && !value.chars().any(char::is_control)
    }
    fn filename(value: &Option<String>) -> bool {
        value.as_deref().map(valid_reference).unwrap_or(true)
    }
    let references = [
        &request.correlation_id,
        &request.session_id,
        &request.entity_id,
    ];
    if !valid_reference(&request.skill_id)
        || !text(&request.code, MAX_CODE_BYTES)
        || !text(&request.markdown, MAX_MARKDOWN_BYTES)
        || references
            .iter()
            .any(|v| v.as_deref().is_some_and(|s| !valid_reference(s)))
        || !filename(&request.code_filename)
        || !filename(&request.markdown_filename)
        || [&request.keywords, &request.topics]
            .iter()
            .any(|tags| tags.len() > MAX_TAGS || tags.iter().any(|tag| !metadata(tag)))
    {
        return Err(ForgeDiagnosticCode::InvalidFields);
    }
    if let Some(code) = superego_scan(&request.code, &request.markdown) {
        return Err(code);
    }
    Ok(())
}
