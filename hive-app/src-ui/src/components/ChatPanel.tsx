import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { EntityHttpChatGateway } from "../chat/EntityHttpChatGateway";
import type { ChatGatewayStream } from "../chat/chatGateway";
import { loadConversation, type ConversationMessage } from "../lib/conversation";

interface ChatPanelProps {
  baseUrl: string;
  greeting?: string;
  showHeader?: boolean;
}
interface Message extends ConversationMessage { id: string }

export default function ChatPanel({ baseUrl, greeting, showHeader = true }: ChatPanelProps) {
  const [messages, setMessages] = useState<Message[]>([]);
  const [name, setName] = useState("Your Entity");
  const [input, setInput] = useState("");
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [loadAttempt, setLoadAttempt] = useState(0);
  const [streaming, setStreaming] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const streamRef = useRef<ChatGatewayStream | null>(null);
  const sessionIdRef = useRef("");
  const inFlight = useRef(false);
  const stopPending = useRef(false);
  const activeRequest = useRef<string | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const alive = useRef(true);

  const gateway = useMemo(() => new EntityHttpChatGateway({
    baseUrl, cancelPath: "/v1/chat/cancel",
    requestTimeoutMs: 180_000, idleTimeoutMs: 180_000,
    maxReconnectAttempts: 0,
    // A repeated POST could execute a tool twice. Let the family retry explicitly.
    allowNonStreamingFallback: false,
    fetchFn: globalThis.fetch.bind(globalThis),
  }), [baseUrl]);

  useEffect(() => {
    alive.current = true;
    let current = true;
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), 15_000);
    setLoading(true);
    setLoadError(null);
    setNotice(null);
    setMessages([]);
    setStreaming(false);
    setStopping(false);
    stopPending.current = false;
    void loadConversation(baseUrl, controller.signal).then((snapshot) => {
      if (!current) return;
      setName(snapshot.name || "Your Entity");
      sessionIdRef.current = snapshot.session_id;
      setMessages(snapshot.messages.map((message, index) => ({ ...message, id: `history-${index}` })));
    }).catch(() => {
      if (current) setLoadError("Your conversation could not be restored. Retry to continue where you left off.");
    }).finally(() => {
      clearTimeout(timer);
      if (current) setLoading(false);
    });
    return () => {
      current = false;
      alive.current = false;
      controller.abort();
      clearTimeout(timer);
      activeRequest.current = null;
      inFlight.current = false;
      const stream = streamRef.current;
      streamRef.current = null;
      if (stream) void stream.cancel().finally(() => stream.dispose());
    };
  }, [baseUrl, loadAttempt]);

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight });
  }, [messages]);

  const send = useCallback(async () => {
    const text = input.trim();
    if (!text || loading || loadError || inFlight.current) return;
    inFlight.current = true;
    stopPending.current = false;
    setNotice(null);
    setInput("");
    const requestId = crypto.randomUUID();
    const assistantId = `${requestId}-assistant`;
    activeRequest.current = requestId;
    setMessages((previous) => [
      ...previous,
      { id: `${requestId}-user`, role: "user", content: text },
      { id: assistantId, role: "assistant", content: "" },
    ]);
    setStreaming(true);
    const isCurrent = () => alive.current && activeRequest.current === requestId;
    const setAssistant = (updater: (current: string) => string) => {
      if (!isCurrent()) return;
      setMessages((previous) => previous.map((message) =>
        message.id === assistantId ? { ...message, content: updater(message.content) } : message,
      ));
    };
    const finish = () => {
      if (!isCurrent()) return;
      activeRequest.current = null;
      inFlight.current = false;
      setStreaming(false);
      setStopping(false);
    };
    const fail = (interrupted: boolean) => {
      if (!isCurrent()) return;
      setMessages((previous) => previous.filter((message) =>
        message.id !== assistantId || message.content !== "",
      ));
      setNotice(interrupted
        ? "Reply stopped. You can continue the conversation."
        : "The reply could not be completed. Check the model connection in Abigail Hive, then try again.");
      finish();
    };
    try {
      const stream = await gateway.send({ message: text, sessionId: sessionIdRef.current }, {
        onToken: (token) => setAssistant((current) => current + token),
        onDone: (response) => { setAssistant((current) => response.reply || current); finish(); },
        onError: (error) => fail(error.interrupted),
      });
      if (!isCurrent()) { await stream.cancel(); await stream.dispose(); return; }
      streamRef.current = stream;
      if (stopPending.current) await stream.cancel();
    } catch { fail(false); }
  }, [input, loading, loadError, gateway]);

  const stop = useCallback(async () => {
    if (stopPending.current) return;
    stopPending.current = true;
    setStopping(true);
    await streamRef.current?.cancel();
  }, []);

  return (
    <div className="theme-modern flex h-full flex-col bg-theme-bg text-theme-text font-primary">
      {showHeader && (
        <header className="border-b border-theme-border bg-theme-bg-elevated px-6 py-4">
          <h1 className="text-lg font-semibold text-theme-text-bright">{name}</h1>
          <p className="text-xs text-theme-text-dim">Memory stays on this computer. Manage models in Abigail Hive.</p>
        </header>
      )}
      <div ref={scrollRef} className="flex-1 overflow-y-auto px-6 py-6" role="log" aria-label="Conversation" aria-busy={loading || streaming}>
        <div className="mx-auto flex max-w-2xl flex-col gap-4">
          {loading && <p role="status" className="text-center text-sm text-theme-text-dim">Restoring your conversation…</p>}
          {loadError && (
            <div role="alert" className="rounded-theme-md border border-theme-border bg-theme-surface p-4 text-sm">
              <p>{loadError}</p>
              <button type="button" onClick={() => setLoadAttempt((attempt) => attempt + 1)} className="mt-3 text-theme-primary">Retry</button>
            </div>
          )}
          {!loading && !loadError && messages.length === 0 && (
            <div className="mt-16 text-center text-theme-text-dim">
              <p className="text-lg text-theme-text">{greeting ?? `Hi, I'm ${name}. What can I help you with today?`}</p>
            </div>
          )}
          {messages.map((message) => (
            <div key={message.id} className={message.role === "user" ? "flex justify-end" : "flex justify-start"}>
              <div className={`max-w-[80%] whitespace-pre-wrap break-words rounded-theme-lg px-4 py-2.5 text-theme-text ${message.role === "user" ? "bg-theme-bubble-user" : "bg-theme-bubble-assistant"}`}>
                {message.content || (streaming ? "…" : "")}
              </div>
            </div>
          ))}
          {notice && <p role="status" className="rounded-theme-md border border-theme-border bg-theme-surface px-4 py-3 text-sm text-theme-text-dim">{notice}</p>}
        </div>
      </div>
      <div className="border-t border-theme-border bg-theme-bg-elevated px-6 py-4">
        <div className="mx-auto flex max-w-2xl items-end gap-2">
          <textarea
            aria-label="Message" value={input} onChange={(event) => setInput(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
                event.preventDefault(); void send();
              }
            }}
            disabled={loading || loadError !== null} rows={1} placeholder="Type a message…"
            className="max-h-40 flex-1 resize-none rounded-theme-md border border-theme-border bg-theme-input-bg px-3 py-2 text-theme-text outline-none focus:border-theme-primary disabled:opacity-40"
          />
          {streaming ? (
            <button type="button" onClick={() => void stop()} disabled={stopping} className="rounded-theme-md border border-theme-border px-4 py-2 text-sm text-theme-text-dim disabled:opacity-40">
              {stopping ? "Stopping…" : "Stop"}
            </button>
          ) : (
            <button type="button" onClick={() => void send()} disabled={!input.trim() || loading || loadError !== null} className="rounded-theme-md bg-theme-primary px-4 py-2 text-sm font-medium text-white disabled:opacity-40">Send</button>
          )}
        </div>
      </div>
    </div>
  );
}
