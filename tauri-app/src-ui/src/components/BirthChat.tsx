import { invoke } from "@tauri-apps/api/core";
import { useState, useRef, useEffect } from "react";

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

export default function BirthChat({ stage, onAction, onStageAdvance, initialMessage }: BirthChatProps) {
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

  const sendMessage = async (text: string, isSystemGreeting = false) => {
    if (!text.trim()) return;

    if (!isSystemGreeting) {
      setMessages(m => [...m, { role: "user", content: text }]);
    }

    setLoading(true);
    setError("");

    try {
      const response = await invoke<BirthChatResponse>("birth_chat", {
        message: text,
      });

      setMessages(m => [...m, { role: "assistant", content: response.message }]);

      if (response.action && onAction) {
        onAction(response.action);
      }
    } catch (e) {
      setError(String(e));
      setMessages(m => [...m, { role: "system", content: `Error: ${String(e)}` }]);
    } finally {
      setLoading(false);
    }
  };

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
      <div className="px-4 py-2 border-b border-green-800 bg-green-950/30">
        <p className="text-green-500 text-xs">{stageLabel}</p>
      </div>

      <div ref={scrollRef} className="flex-1 overflow-y-auto p-4 space-y-3">
        {messages.map((msg, i) => (
          <div key={i}>
            {msg.role === "user" && (
              <div className="text-right">
                <span className="text-green-600">Mentor: </span>
                <span className="text-green-400">{msg.content}</span>
              </div>
            )}
            {msg.role === "assistant" && (
              <div>
                <span className="text-green-500">AO: </span>
                <span className="text-green-300">
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
        {loading && <p className="text-green-600 animate-pulse">AO is thinking...</p>}
      </div>

      {error && (
        <div className="px-4 py-1 text-red-400 text-xs">{error}</div>
      )}

      <div className="p-4 border-t border-green-800 flex gap-2">
        <input
          ref={inputRef}
          type="text"
          className="flex-1 bg-black border border-green-500 text-green-500 px-3 py-2 rounded text-sm"
          placeholder="Speak to AO..."
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && handleSend()}
          disabled={loading}
        />
        <button
          className="border border-green-500 px-4 py-2 rounded hover:bg-green-500/20 text-sm disabled:opacity-50"
          onClick={handleSend}
          disabled={loading || !input.trim()}
        >
          Send
        </button>
      </div>

      <div className="px-4 py-2 border-t border-green-900 flex justify-end">
        <button
          className="text-green-600 text-xs hover:text-green-400"
          onClick={onStageAdvance}
        >
          {stage === "Connectivity" ? "Continue to Genesis >" : "Finalize >"}
        </button>
      </div>
    </div>
  );
}
