import { describe, expect, it, vi } from "vitest";
import { EntityHttpChatGateway } from "../EntityHttpChatGateway";
import type {
  ChatGateway,
  ChatGatewayError,
  ChatGatewayRequest,
  ChatGatewayResponse,
  ChatGatewayStream,
} from "../chatGateway";
import { TauriChatGateway } from "../TauriChatGateway";

interface GatewayRun {
  stream: ChatGatewayStream;
  tokens: string[];
  doneCalls: number;
  errorCalls: number;
  done?: ChatGatewayResponse;
  error?: ChatGatewayError;
  waitForTerminal: () => Promise<void>;
}

interface TauriMockHarness {
  invokeFn: (command: string, args?: Record<string, unknown>) => Promise<unknown>;
  listenFn: (eventName: string, callback: (event: { payload: unknown }) => void) => Promise<() => Promise<void>>;
  emit: (eventName: string, payload: unknown) => void;
  activeListeners: Set<string>;
  unlistenCalls: string[];
}

function withTimeout<T>(promise: Promise<T>, timeoutMs = 1_000): Promise<T> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`Timed out after ${timeoutMs}ms`)), timeoutMs);
    promise
      .then((value) => {
        clearTimeout(timer);
        resolve(value);
      })
      .catch((error) => {
        clearTimeout(timer);
        reject(error);
      });
  });
}

async function startGatewayRun(gateway: ChatGateway, request: ChatGatewayRequest): Promise<GatewayRun> {
  const tokens: string[] = [];
  let done: ChatGatewayResponse | undefined;
  let error: ChatGatewayError | undefined;
  let doneCalls = 0;
  let errorCalls = 0;
  let resolved = false;

  let resolveTerminal = () => undefined;
  const terminal = new Promise<void>((resolve) => {
    resolveTerminal = () => {
      if (resolved) return;
      resolved = true;
      resolve();
    };
  });

  const stream = await gateway.send(request, {
    onToken: (token) => {
      tokens.push(token);
    },
    onDone: (response) => {
      doneCalls += 1;
      done = response;
      resolveTerminal();
    },
    onError: (gatewayError) => {
      errorCalls += 1;
      error = gatewayError;
      resolveTerminal();
    },
  });

  return {
    stream,
    tokens,
    get doneCalls() {
      return doneCalls;
    },
    get errorCalls() {
      return errorCalls;
    },
    get done() {
      return done;
    },
    get error() {
      return error;
    },
    waitForTerminal: () => withTimeout(terminal),
  };
}

function makeSseResponse(events: Array<{ event: string; data: string }>): Response {
  const payload = events
    .map(({ event, data }) => `event: ${event}\ndata: ${data}\n\n`)
    .join("");
  const encoder = new TextEncoder();
  return new Response(
    new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(encoder.encode(payload));
        controller.close();
      },
    }),
    {
      status: 200,
      headers: { "Content-Type": "text/event-stream" },
    },
  );
}

function createTauriMockHarness(): TauriMockHarness {
  const listeners = new Map<string, (event: { payload: unknown }) => void>();
  const activeListeners = new Set<string>();
  const unlistenCalls: string[] = [];

  const listenFn = async (
    eventName: string,
    callback: (event: { payload: unknown }) => void,
  ): Promise<() => Promise<void>> => {
    listeners.set(eventName, callback);
    activeListeners.add(eventName);
    return async () => {
      unlistenCalls.push(eventName);
      listeners.delete(eventName);
      activeListeners.delete(eventName);
    };
  };

  return {
    invokeFn: async (command: string): Promise<unknown> => {
      if (command === "cancel_chat_stream") {
        return true;
      }
      return null;
    },
    listenFn,
    emit: (eventName: string, payload: unknown) => {
      listeners.get(eventName)?.({ payload });
    },
    activeListeners,
    unlistenCalls,
  };
}

const requestFixture: ChatGatewayRequest = {
  message: "hello gateway",
  target: "EGO",
  sessionMessages: [{ role: "user", content: "previous message" }],
  sessionId: "session-123",
};

const responseFixture: ChatGatewayResponse = {
  reply: "Hello world",
  provider: "openai",
  tool_calls_made: [{ skill_id: "builtin.clipboard", tool_name: "read_clipboard", success: true }],
  tier: "standard",
  model_used: "gpt-4.1",
  complexity_score: 42,
  session_id: "session-123",
  execution_trace: {
    turn_id: "turn-1",
    timestamp_utc: "2026-03-01T12:00:00Z",
    routing_mode: "tier_based",
    configured_provider: "openai",
    configured_model: "gpt-4.1",
    configured_tier: "standard",
    complexity_score: 42,
    selection_reason: "complexity",
    target_selected: "ego",
    steps: [
      {
        provider_label: "openai",
        model_requested: "gpt-4.1",
        result: "success",
        started_at_utc: "2026-03-01T12:00:00Z",
        ended_at_utc: "2026-03-01T12:00:01Z",
      },
    ],
    final_step_index: 0,
    fallback_occurred: false,
  },
};

describe("Chat gateway parity", () => {
  it("returns equivalent functional and telemetry output for tauri and entity adapters", async () => {
    const tauriHarness = createTauriMockHarness();
    tauriHarness.invokeFn = async (command: string): Promise<unknown> => {
      if (command === "chat_stream") {
        queueMicrotask(() => {
          tauriHarness.emit("chat-token", "Hello ");
          tauriHarness.emit("chat-token", "world");
          tauriHarness.emit("chat-done", responseFixture);
        });
        return null;
      }
      if (command === "cancel_chat_stream") return true;
      return null;
    };

    const tauriGateway = new TauriChatGateway({
      invokeFn: tauriHarness.invokeFn as never,
      listenFn: tauriHarness.listenFn as never,
    });

    const tauriRun = await startGatewayRun(tauriGateway, requestFixture);
    await tauriRun.waitForTerminal();

    const fetchFn = vi.fn(async () => {
      return makeSseResponse([
        { event: "token", data: "Hello " },
        { event: "token", data: "world" },
        { event: "done", data: JSON.stringify(responseFixture) },
      ]);
    });

    const entityGateway = new EntityHttpChatGateway({
      baseUrl: "http://entity.local",
      fetchFn: fetchFn as never,
      maxReconnectAttempts: 0,
    });
    const entityRun = await startGatewayRun(entityGateway, requestFixture);
    await entityRun.waitForTerminal();

    expect(tauriRun.tokens).toEqual(entityRun.tokens);
    expect(tauriRun.done).toEqual(entityRun.done);
    expect(tauriRun.error).toBeUndefined();
    expect(entityRun.error).toBeUndefined();
    expect(entityRun.done).toEqual(responseFixture);
    expect(tauriHarness.activeListeners.size).toBe(0);
    expect(tauriHarness.unlistenCalls.sort()).toEqual(["chat-done", "chat-error", "chat-token"]);
  });

  it("returns equivalent interruption semantics across adapters on cancel", async () => {
    const tauriHarness = createTauriMockHarness();
    tauriHarness.invokeFn = async (command: string): Promise<unknown> => {
      if (command === "cancel_chat_stream") return true;
      return null;
    };

    const tauriGateway = new TauriChatGateway({
      invokeFn: tauriHarness.invokeFn as never,
      listenFn: tauriHarness.listenFn as never,
    });

    const entityFetch = vi.fn(async () => {
      return new Response(
        new ReadableStream<Uint8Array>({
          start() {
            // Intentionally keep stream open; cancellation should terminate the request.
          },
        }),
        { status: 200, headers: { "Content-Type": "text/event-stream" } },
      );
    });

    const entityGateway = new EntityHttpChatGateway({
      baseUrl: "http://entity.local",
      fetchFn: entityFetch as never,
      maxReconnectAttempts: 0,
    });

    const tauriRun = await startGatewayRun(tauriGateway, requestFixture);
    const entityRun = await startGatewayRun(entityGateway, requestFixture);

    await tauriRun.stream.cancel();
    await entityRun.stream.cancel();

    await tauriRun.waitForTerminal();
    await entityRun.waitForTerminal();

    expect(tauriRun.done).toBeUndefined();
    expect(entityRun.done).toBeUndefined();
    expect(tauriRun.error?.interrupted).toBe(true);
    expect(entityRun.error?.interrupted).toBe(true);
    expect(tauriRun.error?.message).toContain("Interrupted by user");
    expect(entityRun.error?.message).toContain("Interrupted by user");
    expect(tauriHarness.activeListeners.size).toBe(0);
  });

  it("falls back to HTTP chat when SSE stream times out", async () => {
    const fetchFn = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      if (url.endsWith("/v1/chat/stream")) {
        return new Promise<Response>((_resolve, reject) => {
          const signal = init?.signal as AbortSignal | undefined;
          signal?.addEventListener("abort", () => {
            reject(signal.reason ?? new DOMException("Aborted", "AbortError"));
          });
        });
      }
      if (url.endsWith("/v1/chat")) {
        return new Response(JSON.stringify({ ok: true, data: responseFixture }), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      throw new Error(`Unexpected URL: ${url}`);
    });

    const gateway = new EntityHttpChatGateway({
      baseUrl: "http://entity.local",
      fetchFn: fetchFn as never,
      requestTimeoutMs: 1,
      maxReconnectAttempts: 0,
      reconnectBackoffMs: 0,
    });

    const run = await startGatewayRun(gateway, requestFixture);
    await run.waitForTerminal();

    expect(run.error).toBeUndefined();
    expect(run.done).toEqual(responseFixture);
    expect(fetchFn).toHaveBeenCalledTimes(2);
    expect(String(fetchFn.mock.calls[0]?.[0])).toContain("/v1/chat/stream");
    expect(String(fetchFn.mock.calls[1]?.[0])).toContain("/v1/chat");
  });

  it("guards terminal state and cleans tauri listeners without leaks", async () => {
    const tauriHarness = createTauriMockHarness();
    tauriHarness.invokeFn = async (command: string): Promise<unknown> => {
      if (command === "chat_stream") {
        queueMicrotask(() => {
          tauriHarness.emit("chat-done", responseFixture);
          tauriHarness.emit("chat-error", "late error should be ignored");
        });
      }
      return null;
    };

    const gateway = new TauriChatGateway({
      invokeFn: tauriHarness.invokeFn as never,
      listenFn: tauriHarness.listenFn as never,
    });

    const run = await startGatewayRun(gateway, requestFixture);
    await run.waitForTerminal();

    expect(run.doneCalls).toBe(1);
    expect(run.errorCalls).toBe(0);
    expect(run.done).toEqual(responseFixture);
    expect(tauriHarness.activeListeners.size).toBe(0);
    expect(tauriHarness.unlistenCalls.sort()).toEqual(["chat-done", "chat-error", "chat-token"]);
  });
});
