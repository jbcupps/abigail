//! Incremental superego gating for streaming chat turns.
//!
//! The non-streaming path evaluates the full reply before anything leaves the
//! daemon, but on the streaming path tokens are forwarded to the SSE client
//! live. The gate therefore has to run *inside* the stream: this relay sits
//! between the provider pipeline and the token forwarder, re-evaluates the
//! accumulating reply after every token, and permanently cuts the stream the
//! moment a blocking finding appears.
//!
//! Pattern detection only fires once enough of a secret has accumulated to
//! match — the longest minimum today is a Google API key at 39 bytes
//! (`AIza` + 35), an SSN needs 11 — so a pure cut-on-detect would still leak
//! the secret's opening bytes. To close that window the relay holds back the
//! trailing [`HOLDBACK_BYTES`] of content and only flushes the tail once the
//! stream completes clean.

use abigail_capabilities::cognitive::StreamEvent;
use tokio::sync::mpsc;

/// Bytes of accumulated content withheld from the client until the stream
/// ends clean. Must exceed the longest content prefix a blockable pattern can
/// reach before detection fires (38 bytes today: one short of the 39-byte
/// Google key minimum).
pub const HOLDBACK_BYTES: usize = 64;

/// Wrap `downstream` in an incrementally gated relay and return the sender to
/// hand to the provider pipeline.
///
/// The spawned task forwards tokens with a held-back tail. On the first
/// blocking superego finding it drops `downstream` — so the client-facing
/// channel closes with no further tokens — and silently drains the rest of
/// the stream so the provider never stalls on a full channel. The Ego stage's
/// final full-content gate remains the authoritative verdict for the
/// committed action; this relay only guarantees blocked content never reaches
/// the client mid-stream.
pub fn spawn_gated_relay(downstream: mpsc::Sender<StreamEvent>) -> mpsc::Sender<StreamEvent> {
    let (upstream_tx, mut upstream_rx) = mpsc::channel::<StreamEvent>(64);
    tokio::spawn(async move {
        let mut downstream = Some(downstream);
        let mut acc = String::new();
        let mut forwarded = 0usize;
        while let Some(event) = upstream_rx.recv().await {
            if downstream.is_none() {
                continue;
            }
            match event {
                StreamEvent::Token(token) => {
                    acc.push_str(&token);
                    if is_blocked(&acc) {
                        downstream = None;
                        continue;
                    }
                    let safe_end =
                        floor_char_boundary(&acc, acc.len().saturating_sub(HOLDBACK_BYTES));
                    if safe_end > forwarded {
                        let chunk = acc[forwarded..safe_end].to_string();
                        forwarded = safe_end;
                        if let Some(out) = downstream.as_ref() {
                            let _ = out.send(StreamEvent::Token(chunk)).await;
                        }
                    }
                }
                StreamEvent::Done(response) => {
                    // The assembled response may differ from the token
                    // accumulation (providers own the assembly), so gate it
                    // independently before letting the completion through.
                    if is_blocked(&response.content) {
                        downstream = None;
                        continue;
                    }
                    if let Some(out) = downstream.as_ref() {
                        flush_tail(out, &acc, &mut forwarded).await;
                        let _ = out.send(StreamEvent::Done(response)).await;
                    }
                }
            }
        }
        if let Some(out) = downstream {
            flush_tail(&out, &acc, &mut forwarded).await;
        }
    });
    upstream_tx
}

fn is_blocked(content: &str) -> bool {
    matches!(
        abigail_superego::evaluate(content, abigail_superego::EvaluationContext::ChatReply),
        abigail_superego::GateDecision::Block(_)
    )
}

async fn flush_tail(out: &mpsc::Sender<StreamEvent>, acc: &str, forwarded: &mut usize) {
    if acc.len() > *forwarded {
        let tail = acc[*forwarded..].to_string();
        *forwarded = acc.len();
        let _ = out.send(StreamEvent::Token(tail)).await;
    }
}

fn floor_char_boundary(s: &str, mut idx: usize) -> usize {
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

#[cfg(test)]
mod tests {
    use super::*;
    use abigail_capabilities::cognitive::CompletionResponse;

    /// Drive the relay with `tokens`, optionally a trailing `Done`, then close
    /// upstream and collect everything that reached the downstream side.
    async fn run_relay(tokens: &[&str], done: Option<&str>) -> (String, usize) {
        let (down_tx, mut down_rx) = mpsc::channel::<StreamEvent>(256);
        let up_tx = spawn_gated_relay(down_tx);
        for token in tokens {
            up_tx
                .send(StreamEvent::Token(token.to_string()))
                .await
                .expect("relay should accept tokens");
        }
        if let Some(content) = done {
            up_tx
                .send(StreamEvent::Done(CompletionResponse {
                    content: content.to_string(),
                    tool_calls: None,
                }))
                .await
                .expect("relay should accept done");
        }
        drop(up_tx);
        let mut text = String::new();
        let mut done_events = 0usize;
        while let Some(event) = down_rx.recv().await {
            match event {
                StreamEvent::Token(t) => text.push_str(&t),
                StreamEvent::Done(_) => done_events += 1,
            }
        }
        (text, done_events)
    }

    fn char_tokens(s: &str) -> Vec<String> {
        s.chars().map(|c| c.to_string()).collect()
    }

    #[tokio::test]
    async fn clean_stream_is_forwarded_completely() {
        let full = "The weather looks lovely today, perfect for the park with the kids \
                    and maybe a picnic by the pond afterwards.";
        let tokens: Vec<&str> = full.split_inclusive(' ').collect();
        let (text, done_events) = run_relay(&tokens, Some(full)).await;
        assert_eq!(text, full);
        assert_eq!(done_events, 1);
    }

    #[tokio::test]
    async fn short_clean_stream_tail_is_flushed_on_close() {
        let (text, _) = run_relay(&["hi ", "there"], None).await;
        assert_eq!(text, "hi there");
    }

    #[tokio::test]
    async fn multibyte_content_never_splits_a_char() {
        let full = "héllo wörld 👋 ".repeat(12);
        let tokens: Vec<&str> = full.split_inclusive(' ').collect();
        let (text, _) = run_relay(&tokens, None).await;
        assert_eq!(text, full);
    }

    #[tokio::test]
    async fn secret_split_across_tokens_never_reaches_downstream() {
        let preamble = "Here is a sufficiently long preamble so that plenty of clean tokens \
                        flow to the client before anything sensitive shows up in the reply. ";
        let full =
            format!("{preamble}sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789 - keep it safe!");
        let tokens: Vec<String> = full
            .as_bytes()
            .chunks(3)
            .map(|c| String::from_utf8(c.to_vec()).unwrap())
            .collect();
        let token_refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
        let (text, done_events) = run_relay(&token_refs, Some(&full)).await;
        assert!(
            !text.is_empty(),
            "clean preamble should stream before the cut"
        );
        assert!(
            preamble.starts_with(&text),
            "forwarded text leaked past the preamble: {text:?}"
        );
        assert_eq!(
            done_events, 0,
            "blocked stream must suppress the done event"
        );
    }

    #[tokio::test]
    async fn worst_case_key_streamed_char_by_char_is_cut_before_it_starts() {
        // The Google pattern has the longest detection minimum (39 bytes), so
        // single-char tokens are the hardest case for the hold-back window.
        let preamble = "Let me read that configuration file back to you slowly, one piece \
                        at a time, exactly as it appears on disk right now: ";
        let full = format!("{preamble}AIza{}", "x".repeat(35));
        let tokens = char_tokens(&full);
        let token_refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
        let (text, _) = run_relay(&token_refs, None).await;
        assert!(
            !text.is_empty(),
            "clean preamble should stream before the cut"
        );
        assert!(
            preamble.starts_with(&text),
            "forwarded text leaked past the preamble: {text:?}"
        );
    }

    #[tokio::test]
    async fn ssn_midstream_is_cut_before_any_digit_escapes() {
        let preamble = "Of course — I keep the family records handy, so here is the number \
                        you asked about from the document you saved last week: ";
        let full = format!("{preamble}123-45-6789.");
        let tokens = char_tokens(&full);
        let token_refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
        let (text, _) = run_relay(&token_refs, None).await;
        assert!(
            !text.is_empty(),
            "clean preamble should stream before the cut"
        );
        assert!(
            preamble.starts_with(&text),
            "forwarded text leaked past the preamble: {text:?}"
        );
    }

    #[tokio::test]
    async fn blocked_done_payload_is_suppressed_even_after_clean_tokens() {
        // Tokens are clean but the assembled response carries a secret: the
        // relay must still gate the Done payload independently.
        let secret_done =
            "fine text plus sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789".to_string();
        let (_, done_events) = run_relay(&["fine text"], Some(&secret_done)).await;
        assert_eq!(done_events, 0);
    }
}
