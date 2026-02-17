//! Agentic loop engine — autonomous multi-turn execution with real-time events.
//!
//! Replaces Orion Dock's SSE-based agentic system with Tauri event emission.
//! The engine runs a plan → execute → check loop with mentor interaction points.

use abigail_capabilities::cognitive::provider::{
    CompletionRequest, LlmProvider, Message, ToolCall, ToolDefinition,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;

/// Unique identifier for an agentic run.
pub type TaskId = String;

/// Status of an agentic run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// Run is queued but not yet started.
    Pending,
    /// Run is actively executing.
    Running,
    /// Run is paused waiting for mentor input.
    WaitingForInput,
    /// Run is paused waiting for tool confirmation.
    WaitingForConfirmation,
    /// Run completed successfully.
    Completed,
    /// Run failed.
    Failed,
    /// Run was cancelled by the user.
    Cancelled,
}

/// Events emitted during an agentic run (sent via Tauri events).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgenticEvent {
    RunStarted {
        task_id: TaskId,
        goal: String,
    },
    TurnStarted {
        task_id: TaskId,
        turn_number: u32,
    },
    LlmResponse {
        task_id: TaskId,
        content: String,
        tool_calls: Vec<ToolCallInfo>,
    },
    ToolExecuted {
        task_id: TaskId,
        tool_name: String,
        result: String,
    },
    MentorAsk {
        task_id: TaskId,
        question: String,
    },
    ToolConfirmation {
        task_id: TaskId,
        tool_name: String,
        params: String,
    },
    TurnCompleted {
        task_id: TaskId,
        turn_number: u32,
    },
    RunCompleted {
        task_id: TaskId,
        summary: String,
    },
    RunFailed {
        task_id: TaskId,
        error: String,
    },
    RunCancelled {
        task_id: TaskId,
    },
}

/// Simplified tool call info for serialization in events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallInfo {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

impl From<&ToolCall> for ToolCallInfo {
    fn from(tc: &ToolCall) -> Self {
        Self {
            id: tc.id.clone(),
            name: tc.name.clone(),
            arguments: tc.arguments.clone(),
        }
    }
}

/// Configuration for an agentic run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunConfig {
    /// What the agent should accomplish.
    pub goal: String,
    /// Maximum number of LLM turns before stopping.
    pub max_turns: u32,
    /// Whether to require confirmation for tool execution.
    pub require_confirmation: bool,
    /// System context to prepend to messages.
    pub system_context: Option<String>,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            goal: String::new(),
            max_turns: 10,
            require_confirmation: false,
            system_context: None,
        }
    }
}

/// State of a single agentic run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgenticRun {
    pub task_id: TaskId,
    pub config: RunConfig,
    pub status: RunStatus,
    pub current_turn: u32,
    pub events: Vec<AgenticEvent>,
    pub messages: Vec<Message>,
    pub created_at: String,
    pub completed_at: Option<String>,
}

impl AgenticRun {
    pub fn new(task_id: TaskId, config: RunConfig) -> Self {
        Self {
            task_id,
            config,
            status: RunStatus::Pending,
            current_turn: 0,
            events: Vec::new(),
            messages: Vec::new(),
            created_at: chrono::Utc::now().to_rfc3339(),
            completed_at: None,
        }
    }
}

/// Channel for mentor responses (answers to MentorAsk).
pub type MentorResponseTx = mpsc::Sender<String>;
pub type MentorResponseRx = mpsc::Receiver<String>;

/// Channel for tool confirmation responses.
pub type ConfirmationResponseTx = mpsc::Sender<bool>;
pub type ConfirmationResponseRx = mpsc::Receiver<bool>;

/// The agentic engine that runs autonomous multi-turn loops.
pub struct AgenticEngine {
    provider: Arc<dyn LlmProvider>,
    tools: Vec<ToolDefinition>,
    tool_executor: Arc<dyn ToolExecutor>,
}

/// Trait for executing tool calls (implemented by the Tauri app layer).
#[async_trait::async_trait]
pub trait ToolExecutor: Send + Sync {
    /// Execute a tool call and return the result as a string.
    async fn execute(&self, tool_call: &ToolCall) -> anyhow::Result<String>;
}

impl AgenticEngine {
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        tools: Vec<ToolDefinition>,
        tool_executor: Arc<dyn ToolExecutor>,
    ) -> Self {
        Self {
            provider,
            tools,
            tool_executor,
        }
    }

    /// Run the agentic loop. Emits events through the channel.
    /// Can be cancelled via the CancellationToken.
    /// Mentor input and tool confirmations are received through the provided channels.
    pub async fn run(
        &self,
        run: Arc<RwLock<AgenticRun>>,
        event_tx: mpsc::Sender<AgenticEvent>,
        mut mentor_rx: MentorResponseRx,
        mut confirm_rx: ConfirmationResponseRx,
        cancel: CancellationToken,
    ) -> anyhow::Result<()> {
        let (task_id, config, system_context) = {
            let r = run.read().await;
            (
                r.task_id.clone(),
                r.config.clone(),
                r.config.system_context.clone(),
            )
        };

        // Emit RunStarted
        let start_event = AgenticEvent::RunStarted {
            task_id: task_id.clone(),
            goal: config.goal.clone(),
        };
        {
            let mut r = run.write().await;
            r.status = RunStatus::Running;
            r.events.push(start_event.clone());
        }
        let _ = event_tx.send(start_event).await;

        // Build initial messages
        let mut messages = Vec::new();
        if let Some(ref ctx) = system_context {
            messages.push(Message::new("system", ctx));
        }
        messages.push(Message::new(
            "user",
            format!(
                "Your goal: {}\n\nWork autonomously to accomplish this goal. \
                 Use the available tools as needed. When you believe the goal is \
                 accomplished, summarize what you did.",
                config.goal
            ),
        ));

        // Main loop
        for turn in 1..=config.max_turns {
            // Check cancellation
            if cancel.is_cancelled() {
                let cancel_event = AgenticEvent::RunCancelled {
                    task_id: task_id.clone(),
                };
                {
                    let mut r = run.write().await;
                    r.status = RunStatus::Cancelled;
                    r.events.push(cancel_event.clone());
                    r.completed_at = Some(chrono::Utc::now().to_rfc3339());
                }
                let _ = event_tx.send(cancel_event).await;
                return Ok(());
            }

            // Emit TurnStarted
            let turn_start = AgenticEvent::TurnStarted {
                task_id: task_id.clone(),
                turn_number: turn,
            };
            {
                let mut r = run.write().await;
                r.current_turn = turn;
                r.events.push(turn_start.clone());
            }
            let _ = event_tx.send(turn_start).await;

            // Call LLM
            let request = CompletionRequest {
                messages: messages.clone(),
                tools: if self.tools.is_empty() {
                    None
                } else {
                    Some(self.tools.clone())
                },
            };

            let response = match self.provider.complete(&request).await {
                Ok(resp) => resp,
                Err(e) => {
                    let fail_event = AgenticEvent::RunFailed {
                        task_id: task_id.clone(),
                        error: e.to_string(),
                    };
                    {
                        let mut r = run.write().await;
                        r.status = RunStatus::Failed;
                        r.events.push(fail_event.clone());
                        r.completed_at = Some(chrono::Utc::now().to_rfc3339());
                    }
                    let _ = event_tx.send(fail_event).await;
                    return Err(e);
                }
            };

            // Emit LlmResponse
            let tool_call_infos: Vec<ToolCallInfo> = response
                .tool_calls
                .as_ref()
                .map(|tcs| tcs.iter().map(ToolCallInfo::from).collect())
                .unwrap_or_default();

            let llm_event = AgenticEvent::LlmResponse {
                task_id: task_id.clone(),
                content: response.content.clone(),
                tool_calls: tool_call_infos,
            };
            {
                let mut r = run.write().await;
                r.events.push(llm_event.clone());
            }
            let _ = event_tx.send(llm_event).await;

            // Add assistant message to conversation
            messages.push(Message::new("assistant", &response.content));

            // Execute tool calls if any
            if let Some(ref tool_calls) = response.tool_calls {
                for tc in tool_calls {
                    // Check for confirmation requirement
                    if config.require_confirmation {
                        let confirm_event = AgenticEvent::ToolConfirmation {
                            task_id: task_id.clone(),
                            tool_name: tc.name.clone(),
                            params: tc.arguments.clone(),
                        };
                        {
                            let mut r = run.write().await;
                            r.status = RunStatus::WaitingForConfirmation;
                            r.events.push(confirm_event.clone());
                        }
                        let _ = event_tx.send(confirm_event).await;

                        // Wait for confirmation
                        tokio::select! {
                            confirmed = confirm_rx.recv() => {
                                match confirmed {
                                    Some(true) => {
                                        // Proceed with execution
                                    }
                                    Some(false) | None => {
                                        // Denied — tell LLM the tool was rejected
                                        messages.push(Message::new(
                                            "user",
                                            format!("Tool '{}' was denied by the mentor.", tc.name),
                                        ));
                                        {
                                            let mut r = run.write().await;
                                            r.status = RunStatus::Running;
                                        }
                                        continue;
                                    }
                                }
                            }
                            _ = cancel.cancelled() => {
                                let cancel_event = AgenticEvent::RunCancelled {
                                    task_id: task_id.clone(),
                                };
                                {
                                    let mut r = run.write().await;
                                    r.status = RunStatus::Cancelled;
                                    r.events.push(cancel_event.clone());
                                    r.completed_at = Some(chrono::Utc::now().to_rfc3339());
                                }
                                let _ = event_tx.send(cancel_event).await;
                                return Ok(());
                            }
                        }
                        {
                            let mut r = run.write().await;
                            r.status = RunStatus::Running;
                        }
                    }

                    // Execute the tool
                    let result = match self.tool_executor.execute(tc).await {
                        Ok(r) => r,
                        Err(e) => format!("Error executing tool '{}': {}", tc.name, e),
                    };

                    let tool_event = AgenticEvent::ToolExecuted {
                        task_id: task_id.clone(),
                        tool_name: tc.name.clone(),
                        result: result.clone(),
                    };
                    {
                        let mut r = run.write().await;
                        r.events.push(tool_event.clone());
                    }
                    let _ = event_tx.send(tool_event).await;

                    // Add tool result to conversation
                    messages.push(Message::new(
                        "user",
                        format!("Tool '{}' result: {}", tc.name, result),
                    ));
                }
            } else {
                // No tool calls — LLM thinks it's done or needs input
                // Check if the response asks the mentor a question
                let content_lower = response.content.to_lowercase();
                if content_lower.contains("question for mentor")
                    || content_lower.contains("i need your input")
                    || content_lower.contains("please clarify")
                {
                    let ask_event = AgenticEvent::MentorAsk {
                        task_id: task_id.clone(),
                        question: response.content.clone(),
                    };
                    {
                        let mut r = run.write().await;
                        r.status = RunStatus::WaitingForInput;
                        r.events.push(ask_event.clone());
                    }
                    let _ = event_tx.send(ask_event).await;

                    // Wait for mentor response
                    tokio::select! {
                        answer = mentor_rx.recv() => {
                            if let Some(answer) = answer {
                                messages.push(Message::new("user", &answer));
                                {
                                    let mut r = run.write().await;
                                    r.status = RunStatus::Running;
                                }
                            } else {
                                // Channel closed — abort
                                break;
                            }
                        }
                        _ = cancel.cancelled() => {
                            let cancel_event = AgenticEvent::RunCancelled {
                                task_id: task_id.clone(),
                            };
                            {
                                let mut r = run.write().await;
                                r.status = RunStatus::Cancelled;
                                r.events.push(cancel_event.clone());
                                r.completed_at = Some(chrono::Utc::now().to_rfc3339());
                            }
                            let _ = event_tx.send(cancel_event).await;
                            return Ok(());
                        }
                    }
                } else {
                    // LLM responded without tools — likely done
                    // Check if this is the final turn or LLM indicates completion
                    if turn == config.max_turns
                        || content_lower.contains("goal accomplished")
                        || content_lower.contains("task complete")
                        || content_lower.contains("i have completed")
                    {
                        break;
                    }
                }
            }

            // Emit TurnCompleted
            let turn_end = AgenticEvent::TurnCompleted {
                task_id: task_id.clone(),
                turn_number: turn,
            };
            {
                let mut r = run.write().await;
                r.events.push(turn_end.clone());
            }
            let _ = event_tx.send(turn_end).await;
        }

        // Store final messages
        {
            let mut r = run.write().await;
            r.messages = messages.clone();
        }

        // Emit RunCompleted
        let summary = messages
            .last()
            .map(|m| m.content.clone())
            .unwrap_or_else(|| "Run completed".into());
        let complete_event = AgenticEvent::RunCompleted {
            task_id: task_id.clone(),
            summary,
        };
        {
            let mut r = run.write().await;
            r.status = RunStatus::Completed;
            r.events.push(complete_event.clone());
            r.completed_at = Some(chrono::Utc::now().to_rfc3339());
        }
        let _ = event_tx.send(complete_event).await;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abigail_capabilities::cognitive::provider::CompletionResponse;

    /// Mock LLM provider for testing.
    struct MockProvider {
        responses: std::sync::Mutex<Vec<CompletionResponse>>,
    }

    impl MockProvider {
        fn new(responses: Vec<CompletionResponse>) -> Self {
            Self {
                responses: std::sync::Mutex::new(responses),
            }
        }
    }

    #[async_trait::async_trait]
    impl LlmProvider for MockProvider {
        async fn complete(
            &self,
            _request: &CompletionRequest,
        ) -> anyhow::Result<CompletionResponse> {
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                Ok(CompletionResponse {
                    content: "Goal accomplished. Task complete.".into(),
                    tool_calls: None,
                })
            } else {
                Ok(responses.remove(0))
            }
        }

        async fn stream(
            &self,
            _request: &CompletionRequest,
            _tx: mpsc::Sender<abigail_capabilities::cognitive::provider::StreamEvent>,
        ) -> anyhow::Result<CompletionResponse> {
            self.complete(_request).await
        }
    }

    /// Mock tool executor for testing.
    struct MockToolExecutor;

    #[async_trait::async_trait]
    impl ToolExecutor for MockToolExecutor {
        async fn execute(&self, tc: &ToolCall) -> anyhow::Result<String> {
            Ok(format!("Mock result for {}", tc.name))
        }
    }

    #[test]
    fn test_agentic_run_creation() {
        let config = RunConfig {
            goal: "Test goal".into(),
            max_turns: 5,
            require_confirmation: false,
            system_context: None,
        };
        let run = AgenticRun::new("test-123".into(), config);
        assert_eq!(run.status, RunStatus::Pending);
        assert_eq!(run.current_turn, 0);
        assert!(run.events.is_empty());
    }

    #[test]
    fn test_tool_call_info_from() {
        let tc = ToolCall {
            id: "call_123".into(),
            name: "search".into(),
            arguments: r#"{"query":"test"}"#.into(),
        };
        let info = ToolCallInfo::from(&tc);
        assert_eq!(info.name, "search");
    }

    #[tokio::test]
    async fn test_agentic_engine_simple_run() {
        let provider = Arc::new(MockProvider::new(vec![CompletionResponse {
            content: "I have completed the goal. Task complete.".into(),
            tool_calls: None,
        }]));

        let engine = AgenticEngine::new(provider, vec![], Arc::new(MockToolExecutor));

        let config = RunConfig {
            goal: "Test goal".into(),
            max_turns: 3,
            require_confirmation: false,
            system_context: None,
        };
        let run = Arc::new(RwLock::new(AgenticRun::new("test-1".into(), config)));

        let (event_tx, mut event_rx) = mpsc::channel(32);
        let (_mentor_tx, mentor_rx) = mpsc::channel(1);
        let (_confirm_tx, confirm_rx) = mpsc::channel(1);
        let cancel = CancellationToken::new();

        engine
            .run(run.clone(), event_tx, mentor_rx, confirm_rx, cancel)
            .await
            .unwrap();

        // Check events were emitted
        let mut events = Vec::new();
        while let Ok(event) = event_rx.try_recv() {
            events.push(event);
        }
        assert!(!events.is_empty());

        // Check run completed
        let r = run.read().await;
        assert_eq!(r.status, RunStatus::Completed);
    }

    #[tokio::test]
    async fn test_agentic_engine_cancellation() {
        let provider = Arc::new(MockProvider::new(vec![]));
        let engine = AgenticEngine::new(provider, vec![], Arc::new(MockToolExecutor));

        let config = RunConfig {
            goal: "Test goal".into(),
            max_turns: 100,
            require_confirmation: false,
            system_context: None,
        };
        let run = Arc::new(RwLock::new(AgenticRun::new("test-cancel".into(), config)));

        let (event_tx, _event_rx) = mpsc::channel(32);
        let (_mentor_tx, mentor_rx) = mpsc::channel(1);
        let (_confirm_tx, confirm_rx) = mpsc::channel(1);
        let cancel = CancellationToken::new();

        // Cancel immediately
        cancel.cancel();

        engine
            .run(run.clone(), event_tx, mentor_rx, confirm_rx, cancel)
            .await
            .unwrap();

        let r = run.read().await;
        assert_eq!(r.status, RunStatus::Cancelled);
    }
}
