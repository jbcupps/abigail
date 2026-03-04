export type ChatRole = "user" | "assistant";

export interface ChatGatewaySessionMessage {
  role: ChatRole;
  content: string;
}

export interface ChatGatewayToolCall {
  skill_id: string;
  tool_name: string;
  success: boolean;
}

export interface ExecutionStep {
  provider_label: string;
  model_requested?: string;
  model_reported?: string;
  result: "success" | "error";
  error_summary?: string;
  started_at_utc: string;
  ended_at_utc: string;
}

export interface ExecutionTrace {
  turn_id: string;
  timestamp_utc: string;
  routing_mode: string;
  configured_provider?: string;
  configured_model?: string;
  configured_tier?: string;
  complexity_score?: number;
  selection_reason?: string;
  target_selected: string;
  steps: ExecutionStep[];
  final_step_index: number;
  fallback_occurred: boolean;
}

export interface ChatGatewayRequest {
  message: string;
  target?: "ID" | "EGO" | "AUTO";
  sessionMessages?: ChatGatewaySessionMessage[];
  sessionId?: string;
  modelOverride?: string;
}

export interface ChatGatewayResponse {
  reply: string;
  provider?: string;
  tool_calls_made?: ChatGatewayToolCall[];
  tier?: string;
  model_used?: string;
  complexity_score?: number;
  execution_trace?: ExecutionTrace;
  session_id?: string;
}

export type ChatGatewayErrorCode =
  | "interrupted"
  | "transport"
  | "timeout"
  | "protocol"
  | "unknown";

export interface ChatGatewayError {
  code: ChatGatewayErrorCode;
  message: string;
  interrupted: boolean;
  cause?: unknown;
}

export interface ChatGatewayCallbacks {
  onToken?: (token: string) => void;
  onDone?: (response: ChatGatewayResponse) => void;
  onError?: (error: ChatGatewayError) => void;
}

export interface ChatGatewayStream {
  cancel: () => Promise<void>;
  dispose: () => Promise<void>;
}

export interface ChatGateway {
  send(request: ChatGatewayRequest, callbacks: ChatGatewayCallbacks): Promise<ChatGatewayStream>;
}

export const INTERRUPTED_BY_USER_MESSAGE = "Interrupted by user";

export function isInterruptedByUserMessage(message: string): boolean {
  return message.toLowerCase().includes("interrupted by user");
}

export function normalizeChatGatewayResponse(payload: unknown): ChatGatewayResponse {
  if (payload && typeof payload === "object") {
    const response = payload as Partial<ChatGatewayResponse>;
    return {
      reply: typeof response.reply === "string" ? response.reply : "",
      provider: typeof response.provider === "string" ? response.provider : undefined,
      tool_calls_made: Array.isArray(response.tool_calls_made)
        ? response.tool_calls_made
            .filter((item) => item && typeof item === "object")
            .map((item) => item as ChatGatewayToolCall)
        : undefined,
      tier: typeof response.tier === "string" ? response.tier : undefined,
      model_used: typeof response.model_used === "string" ? response.model_used : undefined,
      complexity_score:
        typeof response.complexity_score === "number" ? response.complexity_score : undefined,
      execution_trace:
        response.execution_trace && typeof response.execution_trace === "object"
          ? (response.execution_trace as ExecutionTrace)
          : undefined,
      session_id: typeof response.session_id === "string" ? response.session_id : undefined,
    };
  }

  if (typeof payload === "string") {
    return { reply: payload };
  }

  return { reply: "" };
}

export function normalizeChatGatewayError(error: unknown): ChatGatewayError {
  if (typeof error === "object" && error !== null) {
    const maybeGatewayError = error as Partial<ChatGatewayError>;
    if (
      typeof maybeGatewayError.code === "string" &&
      typeof maybeGatewayError.message === "string" &&
      typeof maybeGatewayError.interrupted === "boolean"
    ) {
      return {
        code: maybeGatewayError.code as ChatGatewayErrorCode,
        message: maybeGatewayError.message,
        interrupted: maybeGatewayError.interrupted,
        cause: maybeGatewayError.cause,
      };
    }
  }

  const message =
    typeof error === "string"
      ? error
      : error instanceof Error
        ? error.message
        : "Unknown chat transport error";
  if (isInterruptedByUserMessage(message)) {
    return {
      code: "interrupted",
      message: INTERRUPTED_BY_USER_MESSAGE,
      interrupted: true,
      cause: error,
    };
  }

  const lowered = message.toLowerCase();
  if (lowered.includes("timeout")) {
    return { code: "timeout", message, interrupted: false, cause: error };
  }
  if (lowered.includes("sse") || lowered.includes("stream") || lowered.includes("protocol")) {
    return { code: "protocol", message, interrupted: false, cause: error };
  }
  if (lowered.includes("network") || lowered.includes("failed to fetch")) {
    return { code: "transport", message, interrupted: false, cause: error };
  }
  return { code: "unknown", message, interrupted: false, cause: error };
}
