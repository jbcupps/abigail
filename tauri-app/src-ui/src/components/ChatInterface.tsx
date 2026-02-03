import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";

interface Message {
  role: "user" | "assistant";
  content: string;
}

export default function ChatInterface() {
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState("");
  const [loading, setLoading] = useState(false);

  const send = async () => {
    if (!input.trim() || loading) return;
    const userMessage: Message = { role: "user", content: input.trim() };
    setMessages((m) => [...m, userMessage]);
    setInput("");
    setLoading(true);
    try {
      const reply = await invoke<string>("chat", { message: userMessage.content });
      setMessages((m) => [...m, { role: "assistant", content: reply }]);
    } catch (e) {
      setMessages((m) => [...m, { role: "assistant", content: `Error: ${e}` }]);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="min-h-screen bg-black text-green-500 font-mono flex flex-col">
      <div className="flex-1 overflow-y-auto p-4 space-y-4">
        {messages.length === 0 && (
          <p className="text-green-600">Say something to Abby.</p>
        )}
        {messages.map((msg, i) => (
          <div
            key={i}
            className={msg.role === "user" ? "text-right" : ""}
          >
            <span className="text-green-400">{msg.role === "user" ? "You" : "Abby"}: </span>
            {msg.content}
          </div>
        ))}
        {loading && <p className="text-green-600">...</p>}
      </div>
      <div className="p-4 border-t border-green-800 flex gap-2">
        <input
          type="text"
          className="flex-1 bg-black border border-green-500 text-green-500 px-3 py-2 rounded"
          placeholder="Message"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && send()}
        />
        <button
          className="border border-green-500 px-4 py-2 rounded hover:bg-green-500/20"
          onClick={send}
        >
          Send
        </button>
      </div>
    </div>
  );
}
