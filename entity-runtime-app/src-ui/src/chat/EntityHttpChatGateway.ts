import {
  type ChatGateway,
  type ChatGatewayCallbacks,
  type ChatGatewayRequest,
  type ChatGatewayResponse,
  type ChatGatewaySessionMessage,
  type ChatGatewayStream,
  INTERRUPTED_BY_USER_MESSAGE,
  normalizeChatGatewayError,
  normalizeChatGatewayResponse,
} from "./chatGateway";

interface ApiEnvelope<T> {
  ok: boolean;
  data?: T;
  error?: string;
}

interface EntityHttpChatGatewayOptions {
  baseUrl?: string;
  streamPath?: string;
  chatPath?: string;
  cancelPath?: string | null;
  requestTimeoutMs?: number;
  idleTimeoutMs?: number;
  maxReconnectAttempts?: number;
  reconnectBackoffMs?: number;
  fetchFn?: typeof fetch;
}

interface SseEvent {
  event: string;
  data: string;
}

const DEFAULT_BASE_URL = "http://127.0.0.1:3142";
const DEFAULT_STREAM_PATH = "/v1/chat/stream";
const DEFAULT_CHAT_PATH = "/v1/chat";
const DEFAULT_REQUEST_TIMEOUT_MS = 30_000;
const DEFAULT_IDLE_TIMEOUT_MS = 30_000;
const DEFAULT_MAX_RECONNECT_ATTEMPTS = 1;
const DEFAULT_RECONNECT_BACKOFF_MS = 250;

function normalizeBaseUrl(baseUrl: string): string {
  return baseUrl.replace(/\/+$/, "");
}

function makeUrl(baseUrl: string, path: string): string {
  if (/^https?:\/\//.test(path)) return path;
  return `${normalizeBaseUrl(baseUrl)}${path.startsWith("/") ? path : `/${path}`}`;
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function isAbortError(error: unknown): boolean {
  if (error instanceof DOMException) {
    return error.name === "AbortError";
  }
  if (error instanceof Error) {
    return error.name === "AbortError";
  }
  return false;
}

function normalizeSessionMessages(
  sessionMessages: ChatGatewayRequest["sessionMessages"],
): ChatGatewaySessionMessage[] | undefined {
  if (!Array.isArray(sessionMessages)) return undefined;
  return sessionMessages
    .filter((msg): msg is ChatGatewaySessionMessage => {
      return (
        Boolean(msg) &&
        typeof msg === "object" &&
        (msg.role === "user" || msg.role === "assistant") &&
        typeof msg.content === "string"
      );
    })
    .map((msg) => ({ role: msg.role, content: msg.content }));
}

function parseEnvelope<T>(payload: unknown): T {
  if (!payload || typeof payload !== "object") {
    throw new Error("Entity HTTP response was not a JSON object");
  }

  const envelope = payload as ApiEnvelope<T>;
  if (typeof envelope.ok !== "boolean") {
    throw new Error("Entity HTTP response missing envelope status");
  }
  if (!envelope.ok) {
    throw new Error(envelope.error ?? "Entity chat request failed");
  }
  if (typeof envelope.data === "undefined") {
    throw new Error("Entity HTTP response missing data payload");
  }
  return envelope.data;
}

async function readJson(response: Response): Promise<unknown> {
  const text = await response.text();
  if (text.trim().length === 0) return {};
  return JSON.parse(text);
}

export class EntityHttpChatGateway implements ChatGateway {
  private readonly baseUrl: string;
  private readonly streamPath: string;
  private readonly chatPath: string;
  private readonly cancelPath: string | null;
  private readonly requestTimeoutMs: number;
  private readonly idleTimeoutMs: number;
  private readonly maxReconnectAttempts: number;
  private readonly reconnectBackoffMs: number;
  private readonly fetchFn: typeof fetch;

  constructor(options: EntityHttpChatGatewayOptions = {}) {
    this.baseUrl = options.baseUrl ?? DEFAULT_BASE_URL;
    this.streamPath = options.streamPath ?? DEFAULT_STREAM_PATH;
    this.chatPath = options.chatPath ?? DEFAULT_CHAT_PATH;
    this.cancelPath = options.cancelPath ?? null;
    this.requestTimeoutMs = options.requestTimeoutMs ?? DEFAULT_REQUEST_TIMEOUT_MS;
    this.idleTimeoutMs = options.idleTimeoutMs ?? DEFAULT_IDLE_TIMEOUT_MS;
    this.maxReconnectAttempts = options.maxReconnectAttempts ?? DEFAULT_MAX_RECONNECT_ATTEMPTS;
    this.reconnectBackoffMs = options.reconnectBackoffMs ?? DEFAULT_RECONNECT_BACKOFF_MS;
    this.fetchFn = options.fetchFn ?? fetch;
  }

  async send(request: ChatGatewayRequest, callbacks: ChatGatewayCallbacks): Promise<ChatGatewayStream> {
    let terminal = false;
    let disposed = false;
    let userCanceled = false;
    let activeController: AbortController | null = null;
    let requestTimeoutId: ReturnType<typeof setTimeout> | null = null;
    let idleTimeoutId: ReturnType<typeof setTimeout> | null = null;

    const body = JSON.stringify({
      message: request.message,
      target: request.target,
      session_messages: normalizeSessionMessages(request.sessionMessages),
      session_id: request.sessionId,
      model_override: request.modelOverride,
    });

    const clearTimers = (): void => {
      if (requestTimeoutId) {
        clearTimeout(requestTimeoutId);
        requestTimeoutId = null;
      }
      if (idleTimeoutId) {
        clearTimeout(idleTimeoutId);
        idleTimeoutId = null;
      }
    };

    const isTimeoutAbort = (controller: AbortController, error: unknown): boolean => {
      const reason = controller.signal.reason;
      if (reason instanceof DOMException && reason.name === "TimeoutError") return true;
      if (error instanceof DOMException && error.name === "TimeoutError") return true;
      if (error instanceof Error && error.name === "TimeoutError") return true;
      return false;
    };

    const armRequestTimeout = (controller: AbortController): void => {
      clearTimers();
      requestTimeoutId = setTimeout(() => {
        controller.abort(new DOMException("Entity chat stream timeout", "TimeoutError"));
      }, this.requestTimeoutMs);
    };

    const armIdleTimeout = (controller: AbortController): void => {
      if (this.idleTimeoutMs <= 0) return;
      if (idleTimeoutId) clearTimeout(idleTimeoutId);
      idleTimeoutId = setTimeout(() => {
        controller.abort(new DOMException("Entity chat stream idle timeout", "TimeoutError"));
      }, this.idleTimeoutMs);
    };

    const finalizeDone = (response: ChatGatewayResponse): void => {
      if (terminal || disposed) return;
      terminal = true;
      clearTimers();
      callbacks.onDone?.(response);
    };

    const finalizeError = (error: unknown): void => {
      if (terminal || disposed) return;
      terminal = true;
      clearTimers();
      callbacks.onError?.(normalizeChatGatewayError(error));
    };

    const parseSseEvents = (chunk: string, currentEvent: string, currentData: string[]): {
      remainderEvent: string;
      remainderData: string[];
      dispatched: SseEvent[];
    } => {
      const dispatched: SseEvent[] = [];
      const lines = chunk.split("\n");
      let eventName = currentEvent;
      const dataLines = [...currentData];

      for (const rawLine of lines) {
        const line = rawLine.replace(/\r$/, "");
        if (line.length === 0) {
          if (dataLines.length > 0) {
            dispatched.push({
              event: eventName || "message",
              data: dataLines.join("\n"),
            });
          }
          eventName = "";
          dataLines.length = 0;
          continue;
        }
        if (line.startsWith(":")) continue;
        if (line.startsWith("event:")) {
          eventName = line.slice(6).trim();
          continue;
        }
        if (line.startsWith("data:")) {
          dataLines.push(line.slice(5).trimStart());
        }
      }

      return {
        remainderEvent: eventName,
        remainderData: dataLines,
        dispatched,
      };
    };

    const consumeSse = async (response: Response, controller: AbortController): Promise<void> => {
      if (!response.ok) {
        const payloadText = await response.text().catch(() => "");
        throw new Error(
          `Entity stream request failed (${response.status})${payloadText ? `: ${payloadText}` : ""}`,
        );
      }
      if (!response.body) {
        throw new Error("Entity stream response did not include a body");
      }

      const decoder = new TextDecoder();
      const reader = response.body.getReader();
      let buffer = "";
      let currentEvent = "";
      let currentData: string[] = [];

      try {
        while (!terminal && !disposed) {
          armIdleTimeout(controller);
          const { done, value } = await reader.read();
          if (done) break;
          if (!value) continue;
          clearTimers();
          armIdleTimeout(controller);
          buffer += decoder.decode(value, { stream: true });

          const lastNewline = buffer.lastIndexOf("\n");
          if (lastNewline === -1) continue;

          const completeChunk = buffer.slice(0, lastNewline + 1);
          buffer = buffer.slice(lastNewline + 1);

          const parsed = parseSseEvents(completeChunk, currentEvent, currentData);
          currentEvent = parsed.remainderEvent;
          currentData = parsed.remainderData;

          for (const event of parsed.dispatched) {
            if (terminal || disposed) return;
            if (event.event === "token") {
              callbacks.onToken?.(event.data);
              continue;
            }
            if (event.event === "done") {
              let parsedDone: unknown;
              try {
                parsedDone = JSON.parse(event.data);
              } catch {
                parsedDone = event.data;
              }
              finalizeDone(normalizeChatGatewayResponse(parsedDone));
              return;
            }
            if (event.event === "error") {
              throw new Error(event.data || "Entity SSE error event");
            }
          }
        }
      } finally {
        try {
          reader.releaseLock();
        } catch {
          // Ignore release errors; stream is already terminating.
        }
      }

      if (!terminal && !disposed && !userCanceled) {
        throw new Error("Entity chat stream ended before a terminal done/error event");
      }
    };

    const runNonStreamingFallback = async (): Promise<void> => {
      const controller = new AbortController();
      activeController = controller;
      armRequestTimeout(controller);
      try {
        const response = await this.fetchFn(makeUrl(this.baseUrl, this.chatPath), {
          method: "POST",
          headers: {
            "Content-Type": "application/json",
            Accept: "application/json",
          },
          body,
          signal: controller.signal,
        });
        clearTimers();
        if (!response.ok) {
          const payloadText = await response.text().catch(() => "");
          throw new Error(
            `Entity chat fallback failed (${response.status})${payloadText ? `: ${payloadText}` : ""}`,
          );
        }
        const payload = await readJson(response);
        const data = parseEnvelope<unknown>(payload);
        finalizeDone(normalizeChatGatewayResponse(data));
      } catch (error) {
        if (userCanceled) return;
        if (isAbortError(error)) {
          if (isTimeoutAbort(controller, error)) {
            finalizeError("Entity chat request timeout");
          }
          return;
        }
        finalizeError(error);
      } finally {
        clearTimers();
      }
    };

    const runStreaming = async (): Promise<void> => {
      for (let attempt = 0; attempt <= this.maxReconnectAttempts; attempt += 1) {
        if (terminal || disposed || userCanceled) return;

        const controller = new AbortController();
        activeController = controller;
        armRequestTimeout(controller);

        try {
          const response = await this.fetchFn(makeUrl(this.baseUrl, this.streamPath), {
            method: "POST",
            headers: {
              "Content-Type": "application/json",
              Accept: "text/event-stream",
            },
            body,
            signal: controller.signal,
          });

          clearTimers();
          armIdleTimeout(controller);
          await consumeSse(response, controller);
          if (terminal || disposed || userCanceled) return;
        } catch (error) {
          clearTimers();
          if (terminal || disposed || userCanceled) return;
          if (isAbortError(error)) {
            if (!isTimeoutAbort(controller, error)) {
              return;
            }
          }

          const canReconnect = attempt < this.maxReconnectAttempts;
          if (canReconnect) {
            await delay(this.reconnectBackoffMs);
            continue;
          }

          await runNonStreamingFallback();
          return;
        }
      }
    };

    void runStreaming().catch((error) => {
      if (!terminal && !disposed && !userCanceled) {
        finalizeError(error);
      }
    });

    return {
      cancel: async () => {
        if (terminal || disposed || userCanceled) return;
        userCanceled = true;
        activeController?.abort(new DOMException(INTERRUPTED_BY_USER_MESSAGE, "AbortError"));
        clearTimers();

        if (this.cancelPath) {
          try {
            await this.fetchFn(makeUrl(this.baseUrl, this.cancelPath), {
              method: "POST",
              headers: {
                "Content-Type": "application/json",
              },
              body: JSON.stringify({ session_id: request.sessionId }),
            });
          } catch {
            // Best effort only; local abort already guarantees interruption behavior.
          }
        }

        finalizeError(INTERRUPTED_BY_USER_MESSAGE);
      },
      dispose: async () => {
        if (disposed) return;
        disposed = true;
        userCanceled = true;
        activeController?.abort(new DOMException("Chat stream disposed", "AbortError"));
        clearTimers();
      },
    };
  }
}
