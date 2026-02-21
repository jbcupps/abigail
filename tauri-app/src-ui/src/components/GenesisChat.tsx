import { useState, useRef, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

interface Message {
  role: "user" | "assistant";
  content: string;
}

interface Props {
  mode: "direct" | "crystallization";
  onComplete: () => void;
}

export default function GenesisChat({ mode, onComplete }: Props) {
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState("");
  const [loading, setLoading] = useState(false);
  const [completing, setCompleting] = useState(false);
  const [isComplete, setIsComplete] = useState(false);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
    if (!loading && !isComplete) {
      inputRef.current?.focus();
    }
  }, [messages, loading, isComplete]);

  // Start with an introductory message
  useEffect(() => {
    const introMessage: Message = {
      role: "assistant",
      content:
        mode === "direct"
          ? "Let's discover your soul through conversation. Tell me about what matters most to you — your values, your vision, your purpose."
          : "Welcome to soul crystallization. Through a series of questions, we'll explore the depths of your identity. Let's begin — what draws you to create an agent companion?",
    };
    setMessages([introMessage]);
  }, [mode]);

  const send = async () => {
    if (!input.trim() || loading || completing) return;
    const userMsg: Message = { role: "user", content: input.trim() };
    setMessages((prev) => [...prev, userMsg]);
    setInput("");
    setLoading(true);

    try {
      const response = await invoke<{ message: string; complete: boolean }>("genesis_chat", {
        message: userMsg.content,
      });

      setMessages((prev) => [
        ...prev,
        { role: "assistant", content: response.message },
      ]);

      if (response.complete) {
        setIsComplete(true);
      }
    } catch (e) {
      setMessages((prev) => [
        ...prev,
        { role: "assistant", content: `Error: ${e}` },
      ]);
    } finally {
      setLoading(false);
    }
  };

  const handleFinish = async () => {
    setCompleting(true);
    try {
      await onComplete();
    } finally {
      setCompleting(false);
    }
  };

  const title = mode === "direct" ? "Direct Discovery" : "Soul Crystallization";

  return (
    <div className="flex flex-col h-full max-w-2xl mx-auto p-4">
      <h2 className="text-xl font-bold text-theme-text-bright mb-2 text-center">{title}</h2>
      <p className="text-theme-text-dim text-center text-sm mb-4">
        {mode === "direct"
          ? "A single conversation to discover your agent's soul."
          : "A guided exploration of identity and purpose."}
      </p>

      <div className="flex-1 overflow-y-auto space-y-3 mb-4 bg-theme-bg-inset rounded-lg p-4 min-h-[300px]">
        {messages.map((msg, i) => (
          <div
            key={i}
            className={`flex ${msg.role === "user" ? "justify-end" : "justify-start"}`}
          >
            <div
              className={`max-w-[80%] px-4 py-2.5 text-sm animate-fade-in-up ${
                msg.role === "user"
                  ? "bg-theme-bubble-user rounded-xl rounded-br-sm text-theme-text-bright"
                  : "bg-theme-bubble-assistant rounded-xl rounded-bl-sm text-theme-text-bright"
              }`}
            >
              {(msg.content || "").split("\n").map((line, j) => (
                <span key={j}>
                  {line}
                  {j < (msg.content || "").split("\n").length - 1 && <br />}
                </span>
              ))}
            </div>
          </div>
        ))}
        {loading && (
          <div className="flex justify-start">
            <div className="bg-theme-bubble-assistant text-theme-text-dim rounded-xl rounded-bl-sm px-4 py-2 text-sm animate-pulse">
              Thinking...
            </div>
          </div>
        )}
        <div ref={messagesEndRef} />
      </div>

      <div className="relative z-10 bg-theme-bg pt-2">
        {isComplete ? (
          <button
            className="w-full border border-theme-success text-theme-success hover:bg-theme-success-dim px-8 py-4 rounded-lg transition-all font-bold uppercase tracking-widest flex items-center justify-center gap-3 disabled:opacity-50"
            onClick={handleFinish}
            disabled={completing}
          >
            {completing ? (
              <span className="animate-spin text-xl">&#9696;</span>
            ) : null}
            {completing ? "Extracting Identity..." : "Continue to Review"}
          </button>
        ) : (
          <div className="space-y-4">
            <div className="flex gap-2">
              <input
                ref={inputRef}
                type="text"
                className="flex-1 bg-theme-input-bg text-theme-text rounded-lg px-4 py-2 border border-theme-border-dim focus:border-theme-primary focus:ring-1 focus:ring-theme-focus-ring outline-none"
                placeholder="Share your thoughts..."
                value={input}
                onChange={(e) => setInput(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && send()}
                disabled={loading || completing}
              />
              <button
                className="border border-theme-primary text-theme-primary hover:bg-theme-primary-glow px-6 py-2 rounded-lg disabled:opacity-50 transition-colors font-bold"
                onClick={send}
                disabled={!input.trim() || loading || completing}
              >
                Send
              </button>
            </div>
            {messages.length > 2 && (
              <div className="text-center">
                <button
                  className="text-theme-text-dim text-[10px] hover:text-theme-primary underline uppercase tracking-tighter disabled:opacity-50"
                  onClick={handleFinish}
                  disabled={completing}
                >
                  I'm satisfied with this identity. Proceed.
                </button>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
