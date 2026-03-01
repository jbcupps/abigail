import { detectRuntimeMode } from "../runtimeMode";
import { EntityHttpChatGateway } from "./EntityHttpChatGateway";
import type { ChatGateway } from "./chatGateway";
import { TauriChatGateway } from "./TauriChatGateway";

export type ChatTransportMode = "tauri" | "entity-http";

interface ChatGatewayFactoryOptions {
  transport?: ChatTransportMode;
  entityBaseUrl?: string;
}

function readQueryParam(name: string): string | null {
  try {
    const value = new URLSearchParams(window.location.search).get(name);
    return value && value.trim().length > 0 ? value.trim() : null;
  } catch {
    return null;
  }
}

function readConfiguredTransport(): ChatTransportMode | null {
  const fromQuery = readQueryParam("chatTransport");
  if (fromQuery === "tauri" || fromQuery === "entity-http") return fromQuery;

  const envValue = String(import.meta.env.VITE_CHAT_TRANSPORT ?? "").trim().toLowerCase();
  if (envValue === "tauri" || envValue === "entity-http") {
    return envValue;
  }

  return null;
}

function resolveEntityBaseUrl(override?: string): string {
  if (override && override.trim().length > 0) return override.trim();
  const fromQuery = readQueryParam("entityUrl");
  if (fromQuery) return fromQuery;
  const fromEnv = String(import.meta.env.VITE_ENTITY_DAEMON_URL ?? "").trim();
  if (fromEnv.length > 0) return fromEnv;
  return "http://127.0.0.1:3142";
}

export function resolveChatTransportMode(): ChatTransportMode {
  const configured = readConfiguredTransport();
  if (configured) return configured;

  const runtimeMode = detectRuntimeMode();
  if (runtimeMode === "native") {
    return "tauri";
  }
  return "tauri";
}

export function createChatGateway(options: ChatGatewayFactoryOptions = {}): ChatGateway {
  const mode = options.transport ?? resolveChatTransportMode();
  if (mode === "entity-http") {
    return new EntityHttpChatGateway({
      baseUrl: resolveEntityBaseUrl(options.entityBaseUrl),
    });
  }
  return new TauriChatGateway();
}
