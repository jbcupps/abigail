export interface ConversationMessage {
  role: "user" | "assistant";
  content: string;
}

export interface ConversationSnapshot {
  entity_id: string;
  name?: string | null;
  session_id: string;
  messages: ConversationMessage[];
}

async function read<T>(baseUrl: string, path: string, signal: AbortSignal): Promise<T> {
  const response = await fetch(`${baseUrl.replace(/\/+$/, "")}${path}`, {
    headers: { Accept: "application/json" },
    signal,
  });
  if (!response.ok) throw new Error("The Entity is not responding. Retry in a moment.");
  const envelope = await response.json() as { ok: boolean; data?: T; error?: string };
  if (!envelope.ok || envelope.data === undefined) {
    throw new Error(envelope.error || "The conversation could not be loaded. Retry in a moment.");
  }
  return envelope.data;
}

export async function loadConversation(baseUrl: string, signal: AbortSignal): Promise<ConversationSnapshot> {
  const results = await Promise.allSettled([
    read<{ entity_id: string; name?: string | null }>(baseUrl, "/v1/status", signal),
    read<{ session_id: string | null; messages: ConversationMessage[] }>(baseUrl, "/v1/chat/history?limit=200", signal),
  ]);
  const [status, history] = results;
  if (status.status === "rejected") throw status.reason;
  if (history.status === "rejected") throw history.reason;
  if (!status.value.entity_id || !Array.isArray(history.value.messages)) {
    throw new Error("The conversation could not be loaded. Retry in a moment.");
  }
  return {
    ...status.value,
    session_id: history.value.session_id || `entity-${status.value.entity_id}`,
    messages: history.value.messages.filter((message) =>
      (message.role === "user" || message.role === "assistant") && typeof message.content === "string",
    ),
  };
}
