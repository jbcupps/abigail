//! One bounded dispatch implementation for Genesis and Entity conversations.
use abigail_capabilities::cognitive::{
    CompletionRequest, CompletionResponse, LlmProvider, LocalHttpProvider, StreamEvent,
};
use abigail_core::model_routing::{ModelConnection, ResolvedModelConnection, MAX_APPROVED_MODELS};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

#[async_trait]
pub trait ApprovalGate: Send + Sync {
    async fn still_approved(&self, connection: &ModelConnection) -> anyhow::Result<bool>;
}

pub struct DispatchAttempt {
    pub connection: ModelConnection,
    pub started: chrono::DateTime<chrono::Utc>,
    pub error: Option<String>,
}

pub struct DispatchResult {
    pub response: Result<CompletionResponse, String>,
    pub attempts: Vec<DispatchAttempt>,
}

struct Candidate {
    connection: ModelConnection,
    provider: Arc<dyn LlmProvider>,
    unavailable_until: Mutex<Option<Instant>>,
}

pub struct ApprovedDispatch {
    candidates: Vec<Candidate>,
    gate: Option<Arc<dyn ApprovalGate>>,
    attempt_timeout: Duration,
}

impl ApprovedDispatch {
    pub async fn build(connections: &[ResolvedModelConnection]) -> Self {
        let mut candidates = Vec::new();
        // Invalid or oversized handoffs fail closed instead of truncating consent.
        if connections.len() <= MAX_APPROVED_MODELS {
            for resolved in connections {
                let route = &resolved.connection;
                if !resolved.ready
                    || route.validate().is_err()
                    || candidates
                        .iter()
                        .any(|candidate: &Candidate| candidate.connection == *route)
                {
                    continue;
                }
                let provider: Option<Arc<dyn LlmProvider>> = if route.provider == "local" {
                    // Keep checked local routes available for recovery even when
                    // the server is down while a runtime starts. The actual call
                    // remains inside the same timeout and approval gate.
                    Some(Arc::new(ApprovedLocal(route.clone())))
                } else {
                    let selection = crate::ProviderSelection {
                        provider: route.provider.clone(),
                        auth: resolved
                            .api_key
                            .clone()
                            .filter(|key| !key.trim().is_empty())
                            .map(crate::ProviderAuth::ApiKey)
                            .unwrap_or(crate::ProviderAuth::System),
                    };
                    crate::ProviderRegistry::build_ego_with_cli_mode(
                        Some(&selection),
                        route.model.clone(),
                        abigail_core::CliPermissionMode::AllowListOnly,
                    )
                    .provider
                };
                if let Some(provider) = provider {
                    candidates.push(Candidate {
                        connection: route.clone(),
                        provider,
                        unavailable_until: Mutex::new(None),
                    });
                }
            }
        }
        Self {
            candidates,
            gate: None,
            attempt_timeout: Duration::from_secs(30),
        }
    }

    pub fn with_gate(mut self, gate: Arc<dyn ApprovalGate>) -> Self {
        self.gate = Some(gate);
        self
    }
    pub fn set_gate(&mut self, gate: Arc<dyn ApprovalGate>) {
        self.gate = Some(gate);
    }
    pub fn primary(&self) -> Option<&ModelConnection> {
        self.candidates.first().map(|c| &c.connection)
    }
    pub fn has_local(&self) -> bool {
        self.candidates
            .iter()
            .any(|c| c.connection.provider == "local")
    }
    pub fn has_cloud(&self) -> bool {
        self.candidates
            .iter()
            .any(|c| c.connection.provider != "local")
    }
    pub fn is_ready(&self) -> bool {
        !self.candidates.is_empty()
    }

    pub async fn dispatch(
        &self,
        request: &CompletionRequest,
        output: Option<mpsc::Sender<StreamEvent>>,
        local_only: bool,
    ) -> DispatchResult {
        let mut attempts = Vec::new();
        for candidate in &self.candidates {
            if local_only && candidate.connection.provider != "local" {
                continue;
            }
            if output.as_ref().is_some_and(mpsc::Sender::is_closed) {
                return DispatchResult {
                    response: Err("Interrupted before model dispatch.".into()),
                    attempts,
                };
            }
            if let Some(gate) = &self.gate {
                match tokio::time::timeout(self.attempt_timeout, gate.still_approved(&candidate.connection)).await {
                    Ok(Ok(true)) => {},
                    Ok(Ok(false)) => continue,
                    _ => return DispatchResult { response: Err("Hive could not confirm the allowed models. Reconnect Hive and try again.".into()), attempts },
                }
            }
            if candidate
                .unavailable_until
                .lock()
                .map(|until| until.is_some_and(|until| until > Instant::now()))
                .unwrap_or(true)
            {
                continue;
            }
            let mut scoped = request.clone();
            // Caller/model-generated overrides never widen the approved model choice.
            scoped.model_override = candidate.connection.model.clone();
            let started = chrono::Utc::now();
            let (result, emitted) = self
                .attempt(candidate.provider.as_ref(), &scoped, output.as_ref())
                .await;
            match result {
                Ok(response)
                    if !response.content.trim().is_empty()
                        || response
                            .tool_calls
                            .as_ref()
                            .is_some_and(|calls| !calls.is_empty()) =>
                {
                    attempts.push(DispatchAttempt {
                        connection: candidate.connection.clone(),
                        started,
                        error: None,
                    });
                    return DispatchResult {
                        response: Ok(response),
                        attempts,
                    };
                }
                _ => {
                    if let Ok(mut until) = candidate.unavailable_until.lock() {
                        *until = Some(Instant::now() + Duration::from_secs(30));
                    }
                    attempts.push(DispatchAttempt {
                        connection: candidate.connection.clone(),
                        started,
                        error: Some(
                            "The approved model was unavailable or did not complete in time."
                                .into(),
                        ),
                    });
                    if emitted {
                        return DispatchResult { response: Err("The model stopped after beginning its reply. Retry explicitly; no other model received this turn.".into()), attempts };
                    }
                }
            }
        }
        DispatchResult {
            response: Err(
                "No approved model is ready. Check Model connections in Settings.".into(),
            ),
            attempts,
        }
    }

    async fn attempt(
        &self,
        provider: &dyn LlmProvider,
        request: &CompletionRequest,
        output: Option<&mpsc::Sender<StreamEvent>>,
    ) -> (anyhow::Result<CompletionResponse>, bool) {
        let Some(output) = output else {
            return (
                tokio::time::timeout(self.attempt_timeout, provider.complete(request))
                    .await
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("Model timeout"))),
                false,
            );
        };
        let (tx, mut rx) = mpsc::channel(16);
        let call = provider.stream(request, tx);
        tokio::pin!(call);
        let deadline = tokio::time::sleep(self.attempt_timeout);
        tokio::pin!(deadline);
        let mut emitted = false;
        loop {
            tokio::select! {
                biased;
                _ = output.closed() => return (Err(anyhow::anyhow!("Interrupted")), true),
                _ = &mut deadline => return (Err(anyhow::anyhow!("Model timeout")), emitted),
                Some(event) = rx.recv() => {
                    if let StreamEvent::Token(token) = event {
                        if !token.is_empty() {
                            emitted = true;
                            tokio::select! {
                                result = output.send(StreamEvent::Token(token)) => if result.is_err() { return (Err(anyhow::anyhow!("Interrupted")), true); },
                                _ = &mut deadline => return (Err(anyhow::anyhow!("Model timeout")), true),
                            }
                        }
                    }
                    // Only the returned success is allowed to complete the stream.
                },
                result = &mut call => {
                    // A provider can enqueue tokens and fail in the same poll.
                    // Drain them before deciding whether fallback is still allowed.
                    while let Ok(event) = rx.try_recv() {
                        if let StreamEvent::Token(token) = event {
                            if !token.is_empty() {
                                emitted = true;
                                tokio::select! {
                                    result = output.send(StreamEvent::Token(token)) => if result.is_err() { return (Err(anyhow::anyhow!("Interrupted")), true); },
                                    _ = &mut deadline => return (Err(anyhow::anyhow!("Model timeout")), true),
                                }
                            }
                        }
                    }
                    if let Ok(response) = &result {
                        if !response.content.trim().is_empty() || response.tool_calls.as_ref().is_some_and(|calls| !calls.is_empty()) {
                            tokio::select! {
                                sent = output.send(StreamEvent::Done(response.clone())) => if sent.is_err() { return (Err(anyhow::anyhow!("Interrupted")), true); },
                                _ = &mut deadline => return (Err(anyhow::anyhow!("Model timeout")), true),
                            }
                        }
                    }
                    return (result, emitted);
                },
            }
        }
    }
}

#[async_trait]
impl LlmProvider for ApprovedDispatch {
    async fn complete(&self, request: &CompletionRequest) -> anyhow::Result<CompletionResponse> {
        self.dispatch(request, None, false)
            .await
            .response
            .map_err(anyhow::Error::msg)
    }
    async fn stream(
        &self,
        request: &CompletionRequest,
        tx: mpsc::Sender<StreamEvent>,
    ) -> anyhow::Result<CompletionResponse> {
        self.dispatch(request, Some(tx), false)
            .await
            .response
            .map_err(anyhow::Error::msg)
    }
}

/// Local servers may be offline during runtime construction. Never guess an
/// endpoint or replace an explicitly approved model during recovery.
struct ApprovedLocal(ModelConnection);
impl ApprovedLocal {
    async fn provider(&self) -> anyhow::Result<LocalHttpProvider> {
        let url = self.0.local_url.as_deref().unwrap_or_default();
        match &self.0.model {
            Some(model) => LocalHttpProvider::new(url, model),
            // Legacy local choices did not persist a model name. Their explicit
            // endpoint remains the approved choice until checked again in setup.
            None => LocalHttpProvider::connect(url).await,
        }
    }
}
#[async_trait]
impl LlmProvider for ApprovedLocal {
    async fn complete(&self, request: &CompletionRequest) -> anyhow::Result<CompletionResponse> {
        self.provider().await?.complete(request).await
    }
    async fn stream(
        &self,
        request: &CompletionRequest,
        tx: mpsc::Sender<StreamEvent>,
    ) -> anyhow::Result<CompletionResponse> {
        self.provider().await?.stream(request, tx).await
    }
}

/// Disables the old router's implicit synthetic/local fallback under a policy.
pub struct NoApprovedModel;
#[async_trait]
impl LlmProvider for NoApprovedModel {
    async fn complete(&self, _: &CompletionRequest) -> anyhow::Result<CompletionResponse> {
        anyhow::bail!("No approved model is ready. Choose a connection in Settings.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Fixture {
        behavior: &'static str,
        calls: AtomicUsize,
        models: Mutex<Vec<Option<String>>>,
    }
    #[async_trait]
    impl LlmProvider for Fixture {
        async fn complete(
            &self,
            request: &CompletionRequest,
        ) -> anyhow::Result<CompletionResponse> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.models
                .lock()
                .unwrap()
                .push(request.model_override.clone());
            if self.behavior == "hang" {
                std::future::pending::<()>().await;
            }
            if self.behavior != "ok" {
                anyhow::bail!("fixture unavailable");
            }
            Ok(CompletionResponse {
                content: "fixture reply".into(),
                tool_calls: None,
            })
        }
        async fn stream(
            &self,
            request: &CompletionRequest,
            tx: mpsc::Sender<StreamEvent>,
        ) -> anyhow::Result<CompletionResponse> {
            if self.behavior == "partial" {
                tx.send(StreamEvent::Token("partial fixture".into()))
                    .await
                    .unwrap();
            }
            self.complete(request).await
        }
    }
    struct Gate {
        allowed: Vec<String>,
        unavailable: bool,
    }
    #[async_trait]
    impl ApprovalGate for Gate {
        async fn still_approved(&self, route: &ModelConnection) -> anyhow::Result<bool> {
            if self.unavailable {
                anyhow::bail!("Hive unavailable");
            }
            Ok(self.allowed.contains(&route.provider))
        }
    }
    fn fixture(behavior: &'static str) -> Arc<Fixture> {
        Arc::new(Fixture {
            behavior,
            calls: AtomicUsize::new(0),
            models: Mutex::new(vec![]),
        })
    }
    fn dispatcher(items: &[(&str, Arc<Fixture>)]) -> ApprovedDispatch {
        ApprovedDispatch {
            candidates: items
                .iter()
                .map(|(name, provider)| Candidate {
                    connection: ModelConnection {
                        provider: (*name).into(),
                        model: Some(format!("{name}-approved-model")),
                        local_url: None,
                    },
                    provider: provider.clone(),
                    unavailable_until: Mutex::new(None),
                })
                .collect(),
            gate: None,
            attempt_timeout: Duration::from_millis(10),
        }
    }
    fn request() -> CompletionRequest {
        CompletionRequest::simple(vec![]).with_model("unapproved-caller-model")
    }

    #[tokio::test]
    async fn approved_fallback_is_bounded_attributed_and_pins_each_model() {
        let failed = fixture("fail");
        let success = fixture("ok");
        let unused = fixture("ok");
        let dispatch = dispatcher(&[
            ("openai", failed.clone()),
            ("anthropic", success.clone()),
            ("google", unused.clone()),
        ]);
        let result = dispatch.dispatch(&request(), None, false).await;
        assert!(result.response.is_ok());
        assert_eq!(result.attempts.len(), 2);
        assert_eq!(result.attempts[1].connection.provider, "anthropic");
        assert_eq!(unused.calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            success.models.lock().unwrap()[0].as_deref(),
            Some("anthropic-approved-model")
        );
        dispatch.dispatch(&request(), None, false).await;
        assert_eq!(failed.calls.load(Ordering::SeqCst), 1); // cooldown skips failed connection
    }
    #[tokio::test]
    async fn revoked_unready_and_unknown_connections_never_receive_a_request() {
        let revoked = fixture("ok");
        let allowed = fixture("ok");
        let dispatch = dispatcher(&[("openai", revoked.clone()), ("google", allowed.clone())])
            .with_gate(Arc::new(Gate {
                allowed: vec!["google".into()],
                unavailable: false,
            }));
        assert!(dispatch
            .dispatch(&request(), None, false)
            .await
            .response
            .is_ok());
        assert_eq!(revoked.calls.load(Ordering::SeqCst), 0);
        let empty = ApprovedDispatch::build(&[ResolvedModelConnection {
            connection: ModelConnection {
                provider: "openai".into(),
                model: None,
                local_url: None,
            },
            api_key: None,
            ready: false,
        }])
        .await;
        assert!(!empty.is_ready());
        assert!(empty.complete(&request()).await.is_err());
    }
    #[tokio::test]
    async fn unavailable_hive_fails_closed_and_local_only_never_uses_cloud() {
        let cloud = fixture("ok");
        let dispatch = dispatcher(&[("openai", cloud.clone())]).with_gate(Arc::new(Gate {
            allowed: vec![],
            unavailable: true,
        }));
        assert!(dispatch
            .dispatch(&request(), None, false)
            .await
            .response
            .is_err());
        assert!(dispatcher(&[("openai", cloud.clone())])
            .dispatch(&request(), None, true)
            .await
            .response
            .is_err());
        assert_eq!(cloud.calls.load(Ordering::SeqCst), 0);
    }
    #[tokio::test]
    async fn timeout_tries_only_the_next_approved_connection_once() {
        let hanging = fixture("hang");
        let fallback = fixture("ok");
        let dispatch = dispatcher(&[("openai", hanging.clone()), ("local", fallback.clone())]);
        assert!(dispatch
            .dispatch(&request(), None, false)
            .await
            .response
            .is_ok());
        assert_eq!(hanging.calls.load(Ordering::SeqCst), 1);
        assert_eq!(fallback.calls.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn partial_stream_and_closed_channel_never_fallback() {
        let partial = fixture("partial");
        let fallback = fixture("ok");
        let dispatch = dispatcher(&[("openai", partial.clone()), ("local", fallback.clone())]);
        let (tx, mut rx) = mpsc::channel(8);
        assert!(dispatch
            .dispatch(&request(), Some(tx), false)
            .await
            .response
            .is_err());
        assert!(matches!(rx.recv().await, Some(StreamEvent::Token(_))));
        assert_eq!(fallback.calls.load(Ordering::SeqCst), 0);
        let (tx, rx) = mpsc::channel(1);
        drop(rx);
        assert!(dispatch
            .dispatch(&request(), Some(tx), false)
            .await
            .response
            .is_err());
        assert_eq!(fallback.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn all_approved_failures_stop_after_one_attempt_each() {
        let a = fixture("fail");
        let b = fixture("fail");
        let c = fixture("fail");
        let dispatch = dispatcher(&[
            ("openai", a.clone()),
            ("anthropic", b.clone()),
            ("google", c.clone()),
        ]);
        let result = dispatch.dispatch(&request(), None, false).await;
        assert!(result.response.is_err());
        assert_eq!(result.attempts.len(), 3);
        for provider in [a, b, c] {
            assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        }
    }
    #[tokio::test]
    async fn checked_local_route_can_start_offline_and_recover_with_exact_model() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let connection = ModelConnection {
            provider: "local".into(),
            model: Some("approved-local-model".into()),
            local_url: Some(format!("http://{address}")),
        };
        let dispatch = ApprovedDispatch::build(&[ResolvedModelConnection {
            connection: connection.clone(),
            api_key: None,
            ready: true,
        }])
        .await;
        // Construction neither probes an account nor permanently discards a
        // checked local route merely because its server has not started yet.
        assert!(dispatch.is_ready());
        assert!(dispatch.complete(&request()).await.is_err());
        *dispatch.candidates[0].unavailable_until.lock().unwrap() = None;
        let listener = tokio::net::TcpListener::bind(address).await.unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0u8; 2048];
            loop {
                let count = stream.read(&mut buffer).await.unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&buffer[..count]);
                if let Some(split) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..split]);
                    let length: usize = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse().unwrap())
                        })
                        .unwrap();
                    if bytes.len() >= split + 4 + length {
                        assert!(headers.starts_with("POST /v1/chat/completions "));
                        let body: serde_json::Value =
                            serde_json::from_slice(&bytes[split + 4..split + 4 + length]).unwrap();
                        assert_eq!(body["model"], "approved-local-model");
                        break;
                    }
                }
            }
            let body =
                r#"{"choices":[{"message":{"role":"assistant","content":"local fixture reply"}}]}"#;
            let reply = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
            stream.write_all(reply.as_bytes()).await.unwrap();
        });
        let result = dispatch.dispatch(&request(), None, false).await;
        assert_eq!(result.response.unwrap().content, "local fixture reply");
        assert_eq!(result.attempts[0].connection, connection);
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
    }
}
