import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Event } from "@tauri-apps/api/event";
import {
  type ChatGateway,
  type ChatGatewayCallbacks,
  type ChatGatewayRequest,
  type ChatGatewayResponse,
  type ChatGatewayStream,
  INTERRUPTED_BY_USER_MESSAGE,
  normalizeChatGatewayError,
  normalizeChatGatewayResponse,
} from "./chatGateway";

type InvokeFn = typeof invoke;
type ListenFn = typeof listen;
const INTERNAL_EVENT_NAME = "chat-internal-envelope";

type InternalChatEnvelopeKind = "request" | "metadata" | "token" | "done" | "error";

interface InternalChatEnvelope {
  kind: InternalChatEnvelopeKind;
  correlation_id?: string;
  token?: string;
  done?: unknown;
  error?: string;
}

interface TauriChatGatewayDeps {
  invokeFn?: InvokeFn;
  listenFn?: ListenFn;
}

function toResponse(payload: unknown): ChatGatewayResponse {
  if (typeof payload === "string") {
    try {
      return normalizeChatGatewayResponse(JSON.parse(payload));
    } catch {
      return normalizeChatGatewayResponse(payload);
    }
  }
  return normalizeChatGatewayResponse(payload);
}

export class TauriChatGateway implements ChatGateway {
  private readonly invokeFn: InvokeFn;
  private readonly listenFn: ListenFn;

  constructor(deps: TauriChatGatewayDeps = {}) {
    this.invokeFn = deps.invokeFn ?? invoke;
    this.listenFn = deps.listenFn ?? listen;
  }

  async send(request: ChatGatewayRequest, callbacks: ChatGatewayCallbacks): Promise<ChatGatewayStream> {
    let terminal = false;
    let disposed = false;
    let activeCorrelationId: string | null = null;
    const unlisteners: UnlistenFn[] = [];

    const cleanup = async (): Promise<void> => {
      const pending = unlisteners.splice(0, unlisteners.length);
      await Promise.all(
        pending.map(async (unlisten) => {
          try {
            await unlisten();
          } catch {
            // Ignore cleanup failures; listener state must still be considered disposed.
          }
        }),
      );
    };

    const finalizeDone = async (payload: unknown): Promise<void> => {
      if (terminal || disposed) return;
      terminal = true;
      const response = toResponse(payload);
      await cleanup();
      callbacks.onDone?.(response);
    };

    const finalizeError = async (error: unknown): Promise<void> => {
      if (terminal || disposed) return;
      terminal = true;
      const normalized = normalizeChatGatewayError(error);
      await cleanup();
      callbacks.onError?.(normalized);
    };

    const registerListener = async <T>(
      eventName: string,
      handler: (event: Event<T>) => void,
    ): Promise<void> => {
      const unlisten = await this.listenFn<T>(eventName, handler);
      unlisteners.push(unlisten);
    };

    try {
      await registerListener<InternalChatEnvelope>(INTERNAL_EVENT_NAME, (event: Event<InternalChatEnvelope>) => {
        if (terminal || disposed) return;
        const envelope = event.payload;
        if (!envelope || typeof envelope !== "object" || typeof envelope.kind !== "string") return;

        const envelopeCorrelation =
          typeof envelope.correlation_id === "string" ? envelope.correlation_id : null;
        if (envelope.kind === "request" && envelopeCorrelation && !activeCorrelationId) {
          activeCorrelationId = envelopeCorrelation;
          return;
        }
        if (!activeCorrelationId && envelopeCorrelation) {
          activeCorrelationId = envelopeCorrelation;
        }
        if (
          activeCorrelationId &&
          envelopeCorrelation &&
          envelopeCorrelation !== activeCorrelationId
        ) {
          return;
        }

        switch (envelope.kind) {
          case "token":
            if (typeof envelope.token === "string") {
              callbacks.onToken?.(envelope.token);
            }
            break;
          case "done":
            void finalizeDone(envelope.done);
            break;
          case "error":
            void finalizeError(envelope.error ?? "Unknown chat stream error");
            break;
          default:
            break;
        }
      });

      void this.invokeFn("chat_stream", {
        message: request.message,
        sessionMessages: request.sessionMessages,
        sessionId: request.sessionId,
        modelOverride: request.modelOverride,
      }).catch((error) => {
        void finalizeError(error);
      });
    } catch (error) {
      await finalizeError(error);
    }

    return {
      cancel: async () => {
        if (terminal || disposed) return;
        try {
          const canceled = await this.invokeFn<boolean>("cancel_chat_stream");
          if (canceled) {
            await finalizeError(INTERRUPTED_BY_USER_MESSAGE);
          }
        } catch (error) {
          await finalizeError(error);
        }
      },
      dispose: async () => {
        if (disposed) return;
        disposed = true;
        await cleanup();
      },
    };
  }
}
