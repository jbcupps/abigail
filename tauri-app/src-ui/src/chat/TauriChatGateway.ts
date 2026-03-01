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
      await registerListener<string>("chat-token", (event) => {
        if (terminal || disposed) return;
        if (typeof event.payload !== "string") return;
        callbacks.onToken?.(event.payload);
      });

      await registerListener<unknown>("chat-done", (event) => {
        void finalizeDone(event.payload);
      });

      await registerListener<unknown>("chat-error", (event) => {
        void finalizeError(event.payload);
      });

      await this.invokeFn("chat_stream", {
        message: request.message,
        target: request.target,
        sessionMessages: request.sessionMessages,
        sessionId: request.sessionId,
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
