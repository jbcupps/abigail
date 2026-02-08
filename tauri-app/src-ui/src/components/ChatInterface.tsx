import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useState, useEffect } from "react";
import { useTheme } from "../contexts/ThemeContext";
import McpAppFrame from "./McpAppFrame";
import VaultModal, { type MissingSkillSecret } from "./VaultModal";

interface Message {
  role: "user" | "assistant";
  content: string;
  isError?: boolean;
  /** When set, render an MCP App (ui:// resource) in a sandboxed iframe below the message. */
  mcpApp?: { serverId: string; resourceUri: string; title?: string };
}

interface RouterStatus {
  id_provider: string;
  id_url: string | null;
  ego_configured: boolean;
  ego_provider: string | null;
  superego_configured: boolean;
  routing_mode: string;
}

type ConfigStep = "menu" | "ollama" | "lmstudio" | "openai" | null;

interface ChatInterfaceProps {
  target?: "ID" | "EGO";
}

export default function ChatInterface({ target = "EGO" }: ChatInterfaceProps) {
  const { agentName } = useTheme();
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState("");
  const [loading, setLoading] = useState(false);
  const [routerStatus, setRouterStatus] = useState<RouterStatus | null>(null);
  const [configStep, setConfigStep] = useState<ConfigStep>(null);
  const [configInput, setConfigInput] = useState("");
  const [configError, setConfigError] = useState("");
  const [missingSecrets, setMissingSecrets] = useState<MissingSkillSecret[]>([]);
  const [activeSecret, setActiveSecret] = useState<MissingSkillSecret | null>(null);
  const [chatStatus, setChatStatus] = useState<string | null>(null);

  const assistantLabel = agentName || "Abigail";

  const refreshRouterStatus = () => {
    invoke<RouterStatus>("get_router_status")
      .then((status) => {
        setRouterStatus(status);
        // Show config menu if no LLM configured
        if (!status.ego_configured && status.id_provider === "candle_stub") {
          setConfigStep("menu");
        } else {
          setConfigStep(null);
        }
      })
      .catch(console.error);
  };

  const refreshMissingSecrets = () => {
    invoke<MissingSkillSecret[]>("list_missing_skill_secrets")
      .then(setMissingSecrets)
      .catch(() => setMissingSecrets([]));
  };

  useEffect(() => {
    refreshRouterStatus();
    refreshMissingSecrets();

    // Listen for chat-status events from backend (e.g. tool execution)
    const unlisten = listen<{ status: string; tool: string }>("chat-status", (event) => {
      const { tool } = event.payload;
      if (tool === "web_search") {
        setChatStatus("Searching the web...");
      } else {
        setChatStatus(`Running ${tool}...`);
      }
    });
    return () => { unlisten.then((f) => f()); };
  }, []);

  const handleConfigSelect = (option: number) => {
    setConfigError("");
    setConfigInput("");
    switch (option) {
      case 1:
        setConfigStep("ollama");
        setConfigInput("11434");
        break;
      case 2:
        setConfigStep("lmstudio");
        setConfigInput("1234");
        break;
      case 3:
        setConfigStep("openai");
        break;
    }
  };

  const handleConfigSubmit = async () => {
    setConfigError("");
    try {
      if (configStep === "ollama") {
        const port = configInput.trim() || "11434";
        await invoke("set_local_llm_url", { url: `http://localhost:${port}` });
      } else if (configStep === "lmstudio") {
        const port = configInput.trim() || "1234";
        await invoke("set_local_llm_url", { url: `http://localhost:${port}` });
      } else if (configStep === "openai") {
        if (!configInput.trim()) {
          setConfigError("API key is required");
          return;
        }
        await invoke("set_api_key", { key: configInput.trim() });
      }
      setConfigInput("");
      refreshRouterStatus();
    } catch (e) {
      setConfigError(String(e));
    }
  };

  const handleConfigKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter") {
      handleConfigSubmit();
    } else if (e.key === "Escape") {
      setConfigStep("menu");
    }
  };

  const send = async () => {
    if (!input.trim() || loading) return;
    const userMessage: Message = { role: "user", content: input.trim() };
    setMessages((m) => [...m, userMessage]);
    setInput("");
    setLoading(true);

    // Add a placeholder assistant message for streaming
    setMessages((m) => [...m, { role: "assistant", content: "" }]);

    // Listen for streaming tokens
    let streamContent = "";
    const unlisten = await listen<{ token?: string; done?: boolean }>("chat-token", (event) => {
      if (event.payload.token) {
        streamContent += event.payload.token;
        setMessages((m) => {
          const updated = [...m];
          const lastAssistant = updated[updated.length - 1];
          if (lastAssistant && lastAssistant.role === "assistant") {
            updated[updated.length - 1] = { ...lastAssistant, content: streamContent };
          }
          return updated;
        });
      }
    });

    try {
      const reply = await invoke<string>("chat_stream", { message: userMessage.content, target });
      // If streaming didn't produce content (fallback), use the return value
      if (!streamContent) {
        setMessages((m) => {
          const updated = [...m];
          const lastAssistant = updated[updated.length - 1];
          if (lastAssistant && lastAssistant.role === "assistant") {
            updated[updated.length - 1] = { ...lastAssistant, content: reply };
          }
          return updated;
        });
      }
    } catch (e) {
      const errorMsg = String(e);
      let content = errorMsg;
      if (errorMsg.includes("No local LLM configured")) {
        content = "No LLM available. Please either:\n" +
          "1. Set OPENAI_API_KEY environment variable, or\n" +
          "2. Install Ollama and set LOCAL_LLM_BASE_URL=http://localhost:11434";
      }
      setMessages((m) => {
        const updated = [...m];
        const lastAssistant = updated[updated.length - 1];
        if (lastAssistant && lastAssistant.role === "assistant") {
          updated[updated.length - 1] = { ...lastAssistant, content, isError: true };
        }
        return updated;
      });
    } finally {
      unlisten();
      setLoading(false);
      setChatStatus(null);
    }
  };

  const getStatusIndicator = () => {
    if (!routerStatus) return null;

    const hasEgo = routerStatus.ego_configured;
    const hasLocal = routerStatus.id_provider === "local_http";
    const mode = routerStatus.routing_mode;

    let statusText = "";
    let statusColor = "text-yellow-500";

    const egoName = routerStatus.ego_provider || "Cloud";
    const egoLabel = egoName.charAt(0).toUpperCase() + egoName.slice(1);

    if (hasEgo && hasLocal) {
      statusText = `[${mode}] ${egoLabel} + Local`;
      statusColor = "text-theme-text";
    } else if (hasEgo) {
      statusText = `[cloud] ${egoLabel}`;
      statusColor = "text-blue-400";
    } else if (hasLocal) {
      statusText = `[local] ${routerStatus.id_url}`;
      statusColor = "text-theme-primary-dim";
    } else {
      statusText = "[no LLM] Press 1-3 to configure";
      statusColor = "text-red-400";
    }

    return (
      <div
        className={`text-xs ${statusColor} px-4 py-1 border-b border-theme-border cursor-pointer hover:bg-theme-surface`}
        onClick={() => setConfigStep("menu")}
      >
        {statusText}
      </div>
    );
  };

  const renderConfigMenu = () => {
    if (configStep === "menu") {
      return (
        <div className="p-4 border-b border-theme-border bg-theme-surface">
          <p className="text-theme-primary-dim mb-3">Configure LLM Provider:</p>
          <div className="space-y-2">
            <button
              className="block w-full text-left px-3 py-2 border border-theme-primary-faint rounded hover:bg-theme-surface"
              onClick={() => handleConfigSelect(1)}
            >
              <span className="text-theme-text-bright">[1]</span> Ollama (local, default port 11434)
            </button>
            <button
              className="block w-full text-left px-3 py-2 border border-theme-primary-faint rounded hover:bg-theme-surface"
              onClick={() => handleConfigSelect(2)}
            >
              <span className="text-theme-text-bright">[2]</span> LM Studio (local, default port 1234)
            </button>
            <button
              className="block w-full text-left px-3 py-2 border border-theme-primary-faint rounded hover:bg-theme-surface"
              onClick={() => handleConfigSelect(3)}
            >
              <span className="text-theme-text-bright">[3]</span> OpenAI (cloud, requires API key)
            </button>
          </div>
          {routerStatus && (routerStatus.ego_configured || routerStatus.id_provider === "local_http") && (
            <button
              className="mt-3 text-xs text-theme-text-dim hover:text-theme-primary-dim"
              onClick={() => setConfigStep(null)}
            >
              [ESC] Cancel
            </button>
          )}
        </div>
      );
    }

    if (configStep === "ollama" || configStep === "lmstudio") {
      const label = configStep === "ollama" ? "Ollama" : "LM Studio";
      const defaultPort = configStep === "ollama" ? "11434" : "1234";
      return (
        <div className="p-4 border-b border-theme-border bg-theme-surface">
          <p className="text-theme-primary-dim mb-2">{label} Configuration:</p>
          <div className="flex gap-2 items-center">
            <span className="text-theme-text-dim">http://localhost:</span>
            <input
              type="text"
              className="flex-1 bg-black border border-theme-primary text-theme-text px-3 py-2 rounded max-w-[100px]"
              placeholder={defaultPort}
              value={configInput}
              onChange={(e) => setConfigInput(e.target.value)}
              onKeyDown={handleConfigKeyDown}
              autoFocus
            />
            <button
              className="border border-theme-primary px-4 py-2 rounded hover:bg-theme-primary-glow"
              onClick={handleConfigSubmit}
            >
              Connect
            </button>
            <button
              className="border border-theme-primary-faint px-3 py-2 rounded hover:bg-theme-surface text-theme-text-dim"
              onClick={() => setConfigStep("menu")}
            >
              Back
            </button>
          </div>
          {configError && <p className="text-red-400 mt-2 text-sm">{configError}</p>}
        </div>
      );
    }

    if (configStep === "openai") {
      return (
        <div className="p-4 border-b border-theme-border bg-theme-surface">
          <p className="text-theme-primary-dim mb-2">OpenAI Configuration:</p>
          <div className="flex gap-2">
            <input
              type="password"
              className="flex-1 bg-black border border-theme-primary text-theme-text px-3 py-2 rounded"
              placeholder="sk-..."
              value={configInput}
              onChange={(e) => setConfigInput(e.target.value)}
              onKeyDown={handleConfigKeyDown}
              autoFocus
            />
            <button
              className="border border-theme-primary px-4 py-2 rounded hover:bg-theme-primary-glow"
              onClick={handleConfigSubmit}
            >
              Save
            </button>
            <button
              className="border border-theme-primary-faint px-3 py-2 rounded hover:bg-theme-surface text-theme-text-dim"
              onClick={() => setConfigStep("menu")}
            >
              Back
            </button>
          </div>
          {configError && <p className="text-red-400 mt-2 text-sm">{configError}</p>}
        </div>
      );
    }

    return null;
  };

  return (
    <div className="min-h-screen bg-black text-theme-text font-mono flex flex-col">
      {getStatusIndicator()}
      {renderConfigMenu()}
      {missingSecrets.length > 0 && (
        <div className="px-4 py-2 border-b border-yellow-800 bg-yellow-950/20">
          <p className="text-yellow-500 text-xs mb-1">Skills need setup:</p>
          {missingSecrets.map((s, i) => (
            <button
              key={i}
              className="text-xs text-yellow-400 hover:text-yellow-300 mr-3 underline"
              onClick={() => setActiveSecret(s)}
            >
              {s.skill_name}: {s.secret_name}
            </button>
          ))}
        </div>
      )}
      {activeSecret && (
        <VaultModal
          secret={activeSecret}
          onSaved={() => {
            setActiveSecret(null);
            refreshMissingSecrets();
          }}
          onCancel={() => setActiveSecret(null)}
        />
      )}
      <div className="flex-1 overflow-y-auto p-4 space-y-4">
        {messages.length === 0 && (
          <p className="text-theme-text-dim">Say something to {assistantLabel}.</p>
        )}
        {messages.map((msg, i) => (
          <div
            key={i}
            className={msg.role === "user" ? "text-right" : ""}
          >
            <span className={msg.isError ? "text-red-400" : "text-theme-primary-dim"}>
              {msg.role === "user" ? "You" : assistantLabel}:{" "}
            </span>
            <span className={msg.isError ? "text-red-300" : ""}>
              {msg.content.split("\n").map((line, j) => (
                <span key={j}>
                  {line}
                  {j < msg.content.split("\n").length - 1 && <br />}
                </span>
              ))}
            </span>
            {msg.mcpApp && (
              <div className="mt-2">
                <McpAppFrame
                  serverId={msg.mcpApp.serverId}
                  resourceUri={msg.mcpApp.resourceUri}
                  title={msg.mcpApp.title}
                />
              </div>
            )}
          </div>
        ))}
        {loading && <p className="text-theme-text-dim">{chatStatus || "..."}</p>}
      </div>
      <div className="p-4 border-t border-theme-border flex gap-2">
        <input
          type="text"
          className="flex-1 bg-black border border-theme-primary text-theme-text px-3 py-2 rounded"
          placeholder="Message"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && send()}
        />
        <button
          className="border border-theme-primary px-4 py-2 rounded hover:bg-theme-primary-glow"
          onClick={send}
        >
          Send
        </button>
      </div>
    </div>
  );
}
