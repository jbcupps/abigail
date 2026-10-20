import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { EntityHttpChatGateway } from "../chat/EntityHttpChatGateway";
import type { ChatGatewayStream } from "../chat/chatGateway";
import { loadConversation, type ConversationMessage } from "../lib/conversation";
import Presence from "../presence/Presence";
import type { EntityProfile } from "../presence/profile";
import ConversationEntry from "./ConversationEntry";

interface ChatPanelProps {
  baseUrl: string;
  /** The selected Entity's stable ID; runtime addresses can change or be reused. */
  expectedEntityId?: string;
  greeting?: string;
  showHeader?: boolean;
  profile?: EntityProfile;
  profileNotice?: string | null;
  onBack?: () => void;
  /** An explicit visual preview. This mode never creates a transport. */
  demo?: boolean;
}

interface Message extends ConversationMessage { id: string }

type PresenceState = "idle" | "listening" | "thinking" | "speaking" | "interrupted" | "error";

interface ActiveTurn {
  assistantId: string;
  stream: ChatGatewayStream | null;
  canceled: boolean;
}

const PREVIEW_STATES: Array<{ state: PresenceState; name: string; label: string }> = [
  { state: "idle", name: "Idle", label: "Idle · demonstration" },
  { state: "listening", name: "Listening", label: "Listening · demonstration; microphone off" },
  { state: "thinking", name: "Awaiting", label: "Awaiting response · demonstration" },
  { state: "speaking", name: "Speaking", label: "Speaking · demonstration; no audio" },
  { state: "interrupted", name: "Interrupted", label: "Interrupted · demonstration" },
  { state: "error", name: "Error", label: "Error · demonstration" },
];

const STOP_NOTICE = "Reply paused here. Stop was requested; remote work may still finish. Check the saved conversation before repeating an action.";

function lastSavedQuestion(messages: ConversationMessage[]): string | null {
  const last = [...messages].reverse().find(message => message.role === "user" && message.content.trim());
  if (!last) return null;
  const text = Array.from(last.content.trim().replace(/\s+/g, " "));
  return text.length > 180 ? `${text.slice(0, 180).join("")}…` : text.join("");
}

function cancelStream(stream: ChatGatewayStream) {
  // Start backend cancellation before disposal, without holding the UI or
  // cleanup hostage to a slow cancel response.
  void stream.cancel().catch(() => undefined);
  void stream.dispose().catch(() => undefined);
}

export default function ChatPanel({ baseUrl, expectedEntityId, greeting, showHeader = true, profile, profileNotice, onBack, demo = false }: ChatPanelProps) {
  const [messages, setMessages] = useState<Message[]>([]);
  const [runtimeName, setRuntimeName] = useState("Your Entity");
  const [loading, setLoading] = useState(!demo);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [loadAttempt, setLoadAttempt] = useState(0);
  const [input, setInput] = useState("");
  const [streaming, setStreaming] = useState(false);
  const [receivedText, setReceivedText] = useState(false);
  const [activity, setActivity] = useState<PresenceState>("idle");
  const [inputFocused, setInputFocused] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [paused, setPaused] = useState(false);
  const [textOnly, setTextOnly] = useState(false);
  const [previewState, setPreviewState] = useState<PresenceState>("idle");
  const [savedQuestion, setSavedQuestion] = useState<string | null>(null);
  const [slowResponse, setSlowResponse] = useState(false);
  const [needsRecheck, setNeedsRecheck] = useState(false);
  const [rechecking, setRechecking] = useState(false);
  const [turnNumber, setTurnNumber] = useState(0);

  // A ref closes the gap before React renders a disabled Send button. It also
  // prevents late callbacks from a canceled/replaced turn changing the UI.
  const turnRef = useRef<ActiveTurn | null>(null);
  const connectionRef = useRef({ disposed: false, ready: false });
  const sessionIdRef = useRef("");
  const entityIdRef = useRef("");
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const recheckRef = useRef<AbortController | null>(null);
  const name = profile?.name || runtimeName;

  const gateway = useMemo(
    () => demo ? null : new EntityHttpChatGateway({
      baseUrl,
      cancelPath: "/v1/chat/cancel",
      requestTimeoutMs: 180_000,
      idleTimeoutMs: 180_000,
      maxReconnectAttempts: 0,
      // Preserve the runtime's no-replay policy: a second POST may run a tool twice.
      allowNonStreamingFallback: false,
      fetchFn: globalThis.fetch.bind(globalThis),
    }),
    [baseUrl, demo],
  );

  useEffect(() => {
    const connection = { disposed: false, ready: false };
    connectionRef.current = connection;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    // A daemon may move ports after restart. Restore its persisted session and
    // transcript before sending rather than inventing a new conversation.
    sessionIdRef.current = "";
    entityIdRef.current = "";
    setMessages([]);
    setInput("");
    setRuntimeName("Your Entity");
    setLoading(!demo);
    setLoadError(null);
    setStreaming(false);
    setReceivedText(false);
    setInputFocused(false);
    setActivity("idle");
    setNotice(null);
    setSavedQuestion(null);
    setNeedsRecheck(false);
    setRechecking(false);
    if (!demo) {
      const interrupted = new Promise<never>((_, reject) => {
        controller.signal.addEventListener("abort", () => reject(controller.signal.reason), { once: true });
      });
      timer = setTimeout(() => controller.abort(new DOMException("Conversation restore timed out.", "TimeoutError")), 15_000);
      void Promise.race([loadConversation(baseUrl, controller.signal), interrupted]).then(snapshot => {
        if (connection.disposed || controller.signal.aborted) return;
        if (expectedEntityId !== undefined && snapshot.entity_id !== expectedEntityId) {
          setLoadError("This runtime belongs to a different Entity. Return to Hive and reopen the selected Entity, or retry after its connection is corrected. No message was sent.");
          setActivity("error");
          return;
        }
        setRuntimeName(snapshot.name || "Your Entity");
        sessionIdRef.current = snapshot.session_id;
        entityIdRef.current = snapshot.entity_id;
        connection.ready = true;
        setMessages(snapshot.messages.map((message, index) => ({ ...message, id: `history-${index}` })));
        setSavedQuestion(lastSavedQuestion(snapshot.messages));
      }).catch(() => {
        if (connection.disposed) return;
        setLoadError("Your conversation could not be restored. Retry to continue where you left off.");
        setActivity("error");
      }).finally(() => {
        clearTimeout(timer);
        if (!connection.disposed) setLoading(false);
      });
    }
    return () => {
      // Stopped turns no longer own the visible activity, but their delayed
      // transport handles still belong to this connection's lifetime.
      connection.disposed = true;
      connection.ready = false;
      controller.abort();
      clearTimeout(timer);
      recheckRef.current?.abort();
      recheckRef.current = null;
      const turn = turnRef.current;
      turnRef.current = null;
      if (!turn) return;
      if (turn.stream) cancelStream(turn.stream);
    };
  }, [baseUrl, expectedEntityId, demo, gateway, loadAttempt]);

  useEffect(() => {
    setSlowResponse(false);
    if (!streaming || receivedText) return;
    const timer = setTimeout(() => setSlowResponse(true), 12_000);
    return () => clearTimeout(timer);
  }, [streaming, receivedText, turnNumber]);

  useEffect(() => {
    if (messages.length === 0) return;
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight });
  }, [messages]);

  const send = useCallback(async () => {
    const text = input.trim();
    const connection = connectionRef.current;
    if (!text || turnRef.current || recheckRef.current || !gateway || connection.disposed || !connection.ready || loading || loadError) return;

    const turn: ActiveTurn = {
      assistantId: crypto.randomUUID(),
      stream: null,
      canceled: false,
    };
    turnRef.current = turn;
    setTurnNumber(number => number + 1);
    setNotice(null);
    setNeedsRecheck(false);
    setSavedQuestion(null);
    setInput("");
    setReceivedText(false);
    setMessages((prev) => [
      ...prev,
      { id: crypto.randomUUID(), role: "user", content: text },
      { id: turn.assistantId, role: "assistant", content: "" },
    ]);
    setStreaming(true);
    setActivity("thinking");

    let receivedContent = "";
    const isCurrent = () => !connection.disposed && turnRef.current === turn;
    const setAssistant = (updater: (current: string) => string) => {
      setMessages((prev) => prev.map((message) => message.id === turn.assistantId
        ? { ...message, content: updater(message.content) }
        : message));
    };
    const removeEmptyReply = () => setMessages((prev) => prev.filter(
      (message) => message.id !== turn.assistantId || message.content.trim() !== "",
    ));

    try {
      const stream = await gateway.send(
        { message: text, sessionId: sessionIdRef.current },
        {
          onToken: (token) => {
            if (!isCurrent() || !token) return;
            receivedContent += token;
            setReceivedText(true);
            setAssistant((current) => current + token);
          },
          onDone: (response) => {
            if (!isCurrent()) return;
            const reply = response.reply.trim() ? response.reply : receivedContent;
            setAssistant(() => reply);
            turnRef.current = null;
            setStreaming(false);
            setActivity(reply.trim() ? "idle" : "error");
            void turn.stream?.dispose().catch(() => undefined);
            if (!reply.trim()) {
              removeEmptyReply();
              setNeedsRecheck(true);
              setNotice("The Entity finished without a text response. Check the saved conversation before sending again.");
            }
          },
          onError: (error) => {
            if (!isCurrent()) return;
            turnRef.current = null;
            setStreaming(false);
            removeEmptyReply();
            setActivity(error.interrupted ? "interrupted" : "error");
            setNeedsRecheck(true);
            setNotice(error.interrupted ? STOP_NOTICE
              : `The reply could not be completed. Its saved result may differ from what is shown here. ${error.message}`);
            void turn.stream?.dispose().catch(() => undefined);
          },
        },
      );
      turn.stream = stream;
      if (connection.disposed || turn.canceled) cancelStream(stream);
      else if (turnRef.current !== turn) void stream.dispose().catch(() => undefined);
    } catch (error) {
      if (!isCurrent()) return;
      turnRef.current = null;
      setStreaming(false);
      setActivity("error");
      removeEmptyReply();
      setNeedsRecheck(true);
      setNotice(`The reply could not be completed. Its saved result may differ from what is shown here. ${error instanceof Error ? error.message : String(error)}`);
    }
  }, [input, gateway, loading, loadError]);

  const stop = useCallback(() => {
    const turn = turnRef.current;
    if (!turn) return;
    turnRef.current = null;
    turn.canceled = true;
    // Release the UI immediately, even if the daemon's cancel endpoint is slow
    // or the transport handle has not arrived yet. Ignore late reply callbacks.
    setStreaming(false);
    setActivity("interrupted");
    setMessages((prev) => prev.filter((message) =>
      message.id !== turn.assistantId || message.content.trim() !== ""));
    setNeedsRecheck(true);
    setNotice(STOP_NOTICE);
    if (turn.stream) cancelStream(turn.stream);
    inputRef.current?.focus();
  }, []);

  const recheckSavedConversation = useCallback(async () => {
    const connection = connectionRef.current;
    if (demo || turnRef.current || recheckRef.current || connection.disposed || !connection.ready) return;
    const controller = new AbortController();
    recheckRef.current = controller;
    setRechecking(true);
    const timer = setTimeout(() => controller.abort(), 15_000);
    const interrupted = new Promise<never>((_, reject) => {
      controller.signal.addEventListener("abort", () => reject(new Error("Saved conversation check timed out.")), { once: true });
    });
    try {
      const snapshot = await Promise.race([loadConversation(baseUrl, controller.signal), interrupted]);
      if (connection.disposed || controller.signal.aborted) return;
      // A read-only refresh must not silently switch identity or conversation.
      if (snapshot.entity_id !== entityIdRef.current
        || snapshot.session_id !== sessionIdRef.current) {
        connection.ready = false;
        setNotice(null);
        setLoadError("The saved conversation changed identity or session. Return to Hive and reopen this Entity. No message was resent.");
        setActivity("error");
        return;
      }
      setMessages(snapshot.messages.map((message, index) => ({ ...message, id: `history-${index}` })));
      setSavedQuestion(lastSavedQuestion(snapshot.messages));
      setActivity("idle");
      setNotice("Saved conversation reloaded. Nothing was resent; your draft is unchanged. Remote work may still be finishing.");
    } catch {
      if (connection.disposed) return;
      setActivity("error");
      setNotice("The saved conversation could not be checked. Your visible reply and draft are still here. Nothing was resent.");
    } finally {
      clearTimeout(timer);
      if (recheckRef.current === controller) recheckRef.current = null;
      if (!connection.disposed) setRechecking(false);
    }
  }, [baseUrl, demo]);

  const presenceState: PresenceState = demo
    ? previewState
    : activity === "idle" && inputFocused && !loading && !loadError ? "listening" : activity;
  const stateLabel = demo
    ? PREVIEW_STATES.find((item) => item.state === previewState)!.label
    : loading ? "Restoring your conversation"
    : rechecking ? "Checking saved conversation"
    : presenceState === "thinking" ? receivedText ? "Receiving text" : "Awaiting response"
    : presenceState === "listening" ? "Composing text · microphone off"
    : presenceState === "interrupted" ? "Reply paused here"
    : presenceState === "error" ? "Connection needs attention"
    : "Ready when you are";

  return (
    <div className={`presence-runtime${textOnly ? " is-text-only" : ""}`}>
      <aside className="runtime-stage" aria-label={`${name} presence`}>
        <div className="runtime-stage-top">
          {onBack && <button type="button" className="runtime-back" onClick={onBack}>← Back to Hive</button>}
          <span className="runtime-eyebrow">ABIGAIL / ENTITY SPACE</span>
        </div>
        <div className="runtime-identity">
          <span className="runtime-eyebrow">{demo ? "A presence, taking shape" : "Your space, together"}</span>
          <h1>{name}</h1>
          <p className="runtime-stage-copy">{profile?.purpose || "A familiar presence. Room for your next idea."}</p>
          {profileNotice && <p className="runtime-notice" role="status">{profileNotice}</p>}
        </div>
        {!textOnly && <Presence
          identity={profile?.identity || "aether"}
          state={presenceState}
          label={stateLabel}
          reducedMotion={paused}
        />}
        <p className="runtime-state-note" role="status">{stateLabel}</p>
        <p className="runtime-stage-copy runtime-signal-note">
          {demo ? "Demonstration only · these controls preview animation states."
            : streaming ? "A text reply is in progress. Stop is always available."
            : "Text chat · microphone off · no audio playback"}
        </p>
        {demo && <div className="runtime-preview-controls" aria-label="Animation state preview">
          {PREVIEW_STATES.map((item) => <button
            type="button"
            key={item.state}
            className="runtime-preview-state"
            aria-pressed={previewState === item.state}
            onClick={() => setPreviewState(item.state)}
          >{item.name}</button>)}
        </div>}
        <div className="runtime-stage-controls">
          <label className="runtime-control">
            <input type="checkbox" checked={paused} onChange={(event) => setPaused(event.target.checked)} />
            Pause motion
          </label>
          <label className="runtime-control">
            <input type="checkbox" checked={textOnly} onChange={(event) => setTextOnly(event.target.checked)} />
            Text only
          </label>
        </div>
      </aside>

      <section className="runtime-conversation" aria-label="Conversation">
        {showHeader && <header className="runtime-conversation-header">
          <div><span className="runtime-eyebrow">CONVERSATION</span><h2>A little room for possibility.</h2></div>
          <span className="runtime-eyebrow">{demo ? "DESIGN PREVIEW" : "TEXT CONNECTION"}</span>
        </header>}
        {demo && <p className="runtime-demo-note">Design preview · no model connected. You can compose a draft; messages are not sent.</p>}
        {!loading && !loadError && savedQuestion && <aside className="runtime-return" aria-label="Saved conversation context">
          <span className="runtime-eyebrow">YOUR LAST SAVED THREAD</span>
          <p>Last time, you asked: “{savedQuestion}”</p>
          <button type="button" className="runtime-back" onClick={() => inputRef.current?.focus()}>Pick up from here <span aria-hidden="true">↗</span></button>
        </aside>}
        <div ref={scrollRef} className="runtime-transcript" role="log" aria-label="Conversation transcript" aria-live="polite" aria-relevant="additions text" aria-busy={loading || rechecking}>
          {loading && <p className="runtime-notice" role="status">Restoring your conversation…</p>}
          {loadError && <div className="runtime-notice" role="alert"><p>{loadError}</p><button type="button" className="runtime-back" onClick={() => setLoadAttempt(attempt => attempt + 1)}>Retry</button></div>}
          {!loading && !loadError && messages.length === 0 && <div className="runtime-empty">
            <span className="runtime-eyebrow">A NEW CONVERSATION</span>
            <p>{greeting ?? `Hi, I'm ${name}. What can I help you with today?`}</p>
          </div>}
          {messages.map((message) => <article key={message.id} className={`runtime-message is-${message.role}`}>
            <span className="runtime-message-label">{message.role === "user" ? "You" : name}</span>
            <p className={message.content ? undefined : "runtime-message-pending"}>{message.content || "Waiting for the first words…"}</p>
          </article>)}
          {!loading && !loadError && !needsRecheck && !messages.some(message => message.role === "user") && <ConversationEntry
            profile={profile}
            hasDraft={input.length > 0}
            onChoose={draft => { setInput(draft); inputRef.current?.focus(); }}
          />}
          {notice && <div className="runtime-notice">
            <p role={activity === "error" ? "alert" : "status"}>{notice}</p>
            {needsRecheck && !loadError && <button type="button" className="runtime-recheck" disabled={rechecking} onClick={() => void recheckSavedConversation()}>
              {rechecking ? "Checking saved conversation…" : "Check saved conversation"}
            </button>}
          </div>}
        </div>

        {streaming && <div className="runtime-progress" role="status" aria-live="polite" aria-atomic="true">
          <span className="runtime-progress-marker" aria-hidden="true">{receivedText ? "↘" : "◇"}</span>
          <div><strong>{receivedText ? "Reply arriving" : slowResponse ? "Still waiting for a reply" : "Opening a reply"}</strong>
            <p>{receivedText ? "Text is arriving in the conversation. You can keep drafting while it comes in."
              : slowResponse ? "No text has arrived yet. You can wait, or use Stop to pause here."
              : "Waiting for the first text from your Entity. You can use Stop at any time."}</p>
          </div>
        </div>}

        <form className="runtime-composer" onSubmit={(event) => { event.preventDefault(); void send(); }}>
          <label className="runtime-eyebrow" htmlFor="entity-message">YOUR MESSAGE</label>
          <textarea
            ref={inputRef}
            id="entity-message"
            value={input}
            disabled={loading || loadError !== null}
            onChange={(event) => setInput(event.target.value)}
            onFocus={() => setInputFocused(true)}
            onBlur={() => setInputFocused(false)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
                event.preventDefault();
                void send();
              }
            }}
            rows={2}
            placeholder="Type a message…"
            aria-describedby="entity-composer-hint"
            className="runtime-input"
          />
          <div className="runtime-composer-footer">
            <span id="entity-composer-hint" className="runtime-input-hint">
              {demo ? "Preview draft · sending is unavailable" : "Enter to send · Shift + Enter for a new line"}
            </span>
            {streaming ? <button key="stop" type="button" className="runtime-send" onClick={event => { event.preventDefault(); stop(); }}>Stop</button>
              : <button key="send" type="submit" className="runtime-send" disabled={!input.trim() || demo || loading || rechecking || loadError !== null}>Send <span aria-hidden="true">↗</span></button>}
          </div>
        </form>
      </section>
    </div>
  );
}
