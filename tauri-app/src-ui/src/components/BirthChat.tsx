import { invoke } from "@tauri-apps/api/core";
import { useState, useRef, useEffect, useImperativeHandle, forwardRef } from "react";

interface BirthChatResponse {
  message: string;
  stage: string;
  action: BirthAction | null;
}

interface BirthAction {
  type: "RequestApiKey" | "SoulReady" | "StageComplete";
  provider?: string;
  preview?: string;
}

interface ChatMessage {
  role: "user" | "assistant" | "system";
  content: string;
}

interface BirthChatProps {
  stage: "Connectivity" | "Genesis";
  onAction?: (action: BirthAction) => void;
  onStageAdvance: () => void;
  initialMessage?: string;
}

export interface BirthChatHandle {
  injectKeyConfirmation: (provider: string, validatedText?: string) => void;
}

const BirthChat = forwardRef<BirthChatHandle, BirthChatProps>(({ stage, onAction, onStageAdvance, initialMessage }, ref) => {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // Auto-scroll to bottom
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [messages]);

  // Send initial greeting when mounting
  useEffect(() => {
    const greeting = initialMessage || (
      stage === "Connectivity"
        ? "Hello. I just woke up."
        : "I'm ready to discover who I am."
    );
    sendMessage(greeting, true);
  }, []);

  // Focus input
  useEffect(() => {
    inputRef.current?.focus();
  }, [loading]);

  const [retryCount, setRetryCount] = useState(0);
  const [lastMessage, setLastMessage] = useState<string | null>(null);
  const MAX_RETRIES = 3;

  const sendMessage = async (text: string, isSystemGreeting = false, isRetry = false) => {
    if (!text.trim()) return;

    if (!isSystemGreeting && !isRetry) {
      setMessages(m => [...m, { role: "user", content: text }]);
    }

    setLoading(true);
    setError("");
    setLastMessage(text);

    try {
      const response = await invoke<BirthChatResponse>("birth_chat", {
        message: text,
      });

      setMessages(m => [...m, { role: "assistant", content: response.message }]);
      setRetryCount(0); // Reset retry count on success
      setLastMessage(null);

      if (response.action && onAction) {
        onAction(response.action);
      }
    } catch (e) {
      const errorMsg = String(e);
      // Check if it's a network/connection error that might benefit from retry
      const isNetworkError = errorMsg.toLowerCase().includes("connection") ||
        errorMsg.toLowerCase().includes("timeout") ||
        errorMsg.toLowerCase().includes("network") ||
        errorMsg.toLowerCase().includes("failed to fetch");

      if (isNetworkError && retryCount < MAX_RETRIES) {
        setError(`Connection error. Retrying... (${retryCount + 1}/${MAX_RETRIES})`);
        setRetryCount(prev => prev + 1);
        // Retry after a short delay
        setTimeout(() => sendMessage(text, isSystemGreeting, true), 1500);
        return;
      }

      setError(errorMsg);
      setMessages(m => [...m, { role: "system", content: `Error: ${errorMsg}` }]);
      setRetryCount(0);
    } finally {
      if (!error?.includes("Retrying")) {
        setLoading(false);
      }
    }
  };

  const handleRetry = () => {
    if (lastMessage) {
      setRetryCount(0);
      sendMessage(lastMessage, false, true);
    }
  };

  // Expose method to inject key confirmation into conversation
  useImperativeHandle(ref, () => ({
    injectKeyConfirmation: (provider: string, validatedText?: string) => {
      // Send a message that informs the LLM that the key was saved and validated
      const confirmation = `I just saved my ${provider.toUpperCase()} API key using the button above.${validatedText || ""}`;
      sendMessage(confirmation);
    }
  }));

  const handleSend = () => {
    if (!input.trim() || loading) return;
    const text = input.trim();
    setInput("");
    sendMessage(text);
  };

  const stageLabel = stage === "Connectivity"
    ? "CONNECTIVITY: Establishing Cloud Connections"
    : "GENESIS: Discovering Identity";

  return (
    <div className="flex flex-col h-full">
      <div className="px-4 py-2 border-b border-theme-border bg-theme-surface">
        <p className="text-theme-text text-xs">{stageLabel}</p>
      </div>

      <div ref={scrollRef} className="flex-1 overflow-y-auto p-4 space-y-3">
        {messages.map((msg, i) => (
          <div key={i}>
            {msg.role === "user" && (
              <div className="text-right">
                <span className="text-theme-text-dim">Mentor: </span>
                <span className="text-theme-primary-dim">{msg.content}</span>
              </div>
            )}
            {msg.role === "assistant" && (
              <div>
                <span className="text-theme-text">AO: </span>
                <span className="text-theme-text-bright">
                  {msg.content.split("\n").map((line, j) => (
                    <span key={j}>
                      {line}
                      {j < msg.content.split("\n").length - 1 && <br />}
                    </span>
                  ))}
                </span>
              </div>
            )}
            {msg.role === "system" && (
              <div className="text-red-400 text-sm">{msg.content}</div>
            )}
          </div>
        ))}
        {loading && <p className="text-theme-text-dim animate-pulse">Abigail is thinking...</p>}
      </div>

      {error && (
        <div className="px-4 py-1 text-red-400 text-xs flex items-center gap-2">
          <span>{error}</span>
          {lastMessage && !error.includes("Retrying") && (
            <button
              className="text-theme-primary hover:underline"
              onClick={handleRetry}
            >
              Retry
            </button>
          )}
        </div>
      )}

      <div className="p-4 border-t border-theme-border flex gap-2">
        <input
          ref={inputRef}
          type="text"
          className="flex-1 bg-black border border-theme-primary text-theme-text px-3 py-2 rounded text-sm"
          placeholder="Speak to AO..."
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && handleSend()}
          disabled={loading}
        />
        <button
          className="border border-theme-primary px-4 py-2 rounded hover:bg-theme-primary-glow text-sm disabled:opacity-50"
          onClick={handleSend}
          disabled={loading || !input.trim()}
        >
          Send
        </button>
      </div>

      <div className="px-4 py-2 border-t border-theme-border-dim flex justify-end">
        <button
          className="text-theme-text-dim text-xs hover:text-theme-primary-dim"
          onClick={onStageAdvance}
        >
          {stage === "Connectivity" ? "Continue to Genesis >" : "Finalize >"}
        </button>
      </div>
    </div>
  );
});

export default BirthChat;
