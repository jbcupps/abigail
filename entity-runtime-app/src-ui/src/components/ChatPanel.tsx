import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { EntityHttpChatGateway } from "../chat/EntityHttpChatGateway";
import type { ChatGatewayStream } from "../chat/chatGateway";

interface ChatPanelProps {
  baseUrl: string;
  greeting?: string;
}

interface Message {
  id: string;
  role: "user" | "assistant";
  content: string;
}

// A deliberately minimal chat surface for family members: just a conversation
// and an input box. No model/provider pickers, no routing details, no tiers —
// all of that lives in the Hive. Streams over the entity daemon's SSE endpoint.
export default function ChatPanel({ baseUrl, greeting }: ChatPanelProps) {
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState("");
  const [streaming, setStreaming] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  const streamRef = useRef<ChatGatewayStream | null>(null);
  const sessionIdRef = useRef<string>(crypto.randomUUID());
  const scrollRef = useRef<HTMLDivElement>(null);

  // Rebuild the gateway when the daemon URL changes (e.g. the helper restarts on
  // a new port), so chat keeps working instead of pinning the dead URL.
  const gateway = useMemo(
    () =>
      new EntityHttpChatGateway({
        baseUrl,
        cancelPath: "/v1/chat/cancel",
        requestTimeoutMs: 120_000,
        // Native fetch must be invoked with `window` as its receiver; the gateway
        // stores fetchFn on `this`, so pass a bound copy to avoid "Illegal invocation".
        fetchFn: globalThis.fetch.bind(globalThis),
      }),
    [baseUrl],
  );

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight });
  }, [messages]);

  const send = useCallback(async () => {
    const text = input.trim();
    if (!text || streaming) return;
    setNotice(null);
    setInput("");

    const userId = crypto.randomUUID();
    const assistantId = crypto.randomUUID();
    setMessages((prev) => [
      ...prev,
      { id: userId, role: "user", content: text },
      { id: assistantId, role: "assistant", content: "" },
    ]);
    setStreaming(true);

    const setAssistant = (updater: (current: string) => string) => {
      setMessages((prev) =>
        prev.map((msg) =>
          msg.id === assistantId ? { ...msg, content: updater(msg.content) } : msg,
        ),
      );
    };

    try {
      streamRef.current = await gateway.send(
        { message: text, sessionId: sessionIdRef.current },
        {
          onToken: (token) => setAssistant((current) => current + token),
          onDone: (resp) => {
            setAssistant((current) => resp.reply || current);
            setStreaming(false);
          },
          onError: (err) => {
            setStreaming(false);
            // Drop the assistant bubble if nothing streamed (interrupted before
            // the first token, or a hard error) — never leave an empty bubble.
            setMessages((prev) =>
              prev.filter((msg) => !(msg.id === assistantId && msg.content === "")),
            );
            if (err.interrupted) return; // user stopped — keep any partial reply, no error
            setNotice(err.message);
          },
        },
      );
    } catch (err) {
      setStreaming(false);
      setNotice(err instanceof Error ? err.message : String(err));
    }
  }, [input, streaming, gateway]);

  const stop = useCallback(() => {
    void streamRef.current?.cancel();
  }, []);

  return (
    <div className="theme-modern flex h-full flex-col bg-theme-bg text-theme-text font-primary">
      <div ref={scrollRef} className="flex-1 overflow-y-auto px-6 py-6">
        <div className="mx-auto flex max-w-2xl flex-col gap-4">
          {messages.length === 0 && (
            <div className="mt-16 text-center text-theme-text-dim">
              <p className="text-lg text-theme-text">
                {greeting ?? "Hi — what can I help you with today?"}
              </p>
            </div>
          )}
          {messages.map((msg) => (
            <div
              key={msg.id}
              className={msg.role === "user" ? "flex justify-end" : "flex justify-start"}
            >
              <div
                className={
                  msg.role === "user"
                    ? "max-w-[80%] whitespace-pre-wrap rounded-theme-lg bg-theme-bubble-user px-4 py-2.5 text-theme-text"
                    : "max-w-[80%] whitespace-pre-wrap rounded-theme-lg bg-theme-bubble-assistant px-4 py-2.5 text-theme-text"
                }
              >
                {msg.content || (streaming ? "…" : "")}
              </div>
            </div>
          ))}
          {notice && (
            <div className="rounded-theme-md border border-theme-border bg-theme-surface px-4 py-3 text-sm text-theme-text-dim">
              {notice}
            </div>
          )}
        </div>
      </div>

      <div className="border-t border-theme-border bg-theme-bg-elevated px-6 py-4">
        <div className="mx-auto flex max-w-2xl items-end gap-2">
          <textarea
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                void send();
              }
            }}
            rows={1}
            placeholder="Type a message…"
            className="max-h-40 flex-1 resize-none rounded-theme-md border border-theme-border bg-theme-input-bg px-3 py-2 text-theme-text outline-none focus:border-theme-primary"
          />
          {streaming ? (
            <button
              type="button"
              onClick={stop}
              className="rounded-theme-md border border-theme-border px-4 py-2 text-sm text-theme-text-dim hover:text-theme-text"
            >
              Stop
            </button>
          ) : (
            <button
              type="button"
              onClick={() => void send()}
              disabled={!input.trim()}
              className="rounded-theme-md bg-theme-primary px-4 py-2 text-sm font-medium text-white disabled:opacity-40"
            >
              Send
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
