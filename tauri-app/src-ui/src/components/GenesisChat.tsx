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
  const [isComplete, setIsComplete] = useState(false);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages]);

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
    if (!input.trim() || loading) return;
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

  const title = mode === "direct" ? "Direct Discovery" : "Soul Crystallization";

  return (
    <div className="flex flex-col h-full max-w-2xl mx-auto p-4">
      <h2 className="text-xl font-bold text-white mb-2 text-center">{title}</h2>
      <p className="text-gray-400 text-center text-sm mb-4">
        {mode === "direct"
          ? "A single conversation to discover your agent's soul."
          : "A guided exploration of identity and purpose."}
      </p>

      <div className="flex-1 overflow-y-auto space-y-3 mb-4 bg-gray-900 rounded-lg p-4">
        {messages.map((msg, i) => (
          <div
            key={i}
            className={`flex ${msg.role === "user" ? "justify-end" : "justify-start"}`}
          >
            <div
              className={`max-w-[80%] rounded-lg px-4 py-2 text-sm ${
                msg.role === "user"
                  ? "bg-blue-600 text-white"
                  : "bg-gray-800 text-gray-200"
              }`}
            >
              {msg.content.split("\n").map((line, j) => (
                <span key={j}>
                  {line}
                  {j < msg.content.split("\n").length - 1 && <br />}
                </span>
              ))}
            </div>
          </div>
        ))}
        {loading && (
          <div className="flex justify-start">
            <div className="bg-gray-800 text-gray-400 rounded-lg px-4 py-2 text-sm animate-pulse">
              Thinking...
            </div>
          </div>
        )}
        <div ref={messagesEndRef} />
      </div>

      {isComplete ? (
        <button
          className="bg-green-600 hover:bg-green-700 text-white px-8 py-3 rounded-lg mx-auto"
          onClick={onComplete}
        >
          Continue to Emergence
        </button>
      ) : (
        <div className="flex gap-2">
          <input
            type="text"
            className="flex-1 bg-gray-800 text-white rounded-lg px-4 py-2 border border-gray-700 focus:border-blue-500 outline-none"
            placeholder="Share your thoughts..."
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && send()}
            disabled={loading}
          />
          <button
            className="bg-blue-600 hover:bg-blue-700 text-white px-4 py-2 rounded-lg disabled:opacity-50"
            onClick={send}
            disabled={!input.trim() || loading}
          >
            Send
          </button>
        </div>
      )}
    </div>
  );
}
