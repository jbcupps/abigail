import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useState, useEffect, useRef, useCallback } from "react";
import { useTheme } from "../contexts/ThemeContext";
import McpAppFrame from "./McpAppFrame";
import ThinkingIndicator from "./ThinkingIndicator";
import VaultModal, { type MissingSkillSecret } from "./VaultModal";
import { isBrowserHarnessRuntime, isHarnessDebugEnabled } from "../runtimeMode";

interface Message {
  role: "user" | "assistant";
  content: string;
  provider?: string;
  isError?: boolean;
  memoryUsed?: boolean;
  /** When set, render an MCP App (ui:// resource) in a sandboxed iframe below the message. */
  mcpApp?: { serverId: string; resourceUri: string; title?: string };
}

export interface ChatSessionSnapshot {
  messages: Message[];
  input: string;
}

interface RouterStatus {
  id_provider: string;
  id_url: string | null;
  ego_configured: boolean;
  ego_provider: string | null;
  superego_configured: boolean;
  routing_mode: string;
  council_providers?: number;
}

interface OllamaStatus {
  managed: boolean;
  running: boolean;
  port: number;
  model_ready: boolean;
}

type ConfigStep = "menu" | "ollama" | "lmstudio" | "openai" | "claude-cli" | "gemini-cli" | "codex-cli" | "grok-cli" | null;

interface ChatInterfaceProps {
  target?: "ID" | "EGO";
  initialSession?: ChatSessionSnapshot | null;
  onSessionSnapshot?: (snapshot: ChatSessionSnapshot) => void;
}

/** Defense-in-depth: redact common API key patterns before rendering. */
function redactApiKeys(text: string): string {
  return text.replace(
    /(?:sk-(?:ant-|proj-)?[A-Za-z0-9_-]{10,})|(?:xai-[A-Za-z0-9_-]{10,})|(?:pplx-[A-Za-z0-9_-]{10,})|(?:AIza[A-Za-z0-9_-]{10,})|(?:tvly-[A-Za-z0-9_-]{10,})/g,
    (match) => {
      const dashIdx = match.indexOf("-");
      const visible = dashIdx >= 0 ? match.slice(0, dashIdx + 4) : match.slice(0, 4);
      return `${visible}***`;
    }
  );
}

export default function ChatInterface({
  target = "EGO",
  initialSession = null,
  onSessionSnapshot,
}: ChatInterfaceProps) {
  const { agentName } = useTheme();
  const [messages, setMessages] = useState<Message[]>(() => initialSession?.messages ?? []);
  const [input, setInput] = useState(() => initialSession?.input ?? "");
  const [loading, setLoading] = useState(false);
  const [routerStatus, setRouterStatus] = useState<RouterStatus | null>(null);
  const [configStep, setConfigStep] = useState<ConfigStep>(null);
  const [configInput, setConfigInput] = useState("");
  const [configError, setConfigError] = useState("");
  const [missingSecrets, setMissingSecrets] = useState<MissingSkillSecret[]>([]);
  const [activeSecret, setActiveSecret] = useState<MissingSkillSecret | null>(null);
  const [chatStatus, setChatStatus] = useState<string | null>(null);
  const [ollamaStatus, setOllamaStatus] = useState<OllamaStatus | null>(null);
  const [lmStudioStatus, setLmStudioStatus] = useState<boolean>(false);
  const [storedProviders, setStoredProviders] = useState<string[]>([]);
  const [cliServerStatus, setCliServerStatus] = useState<{ running: boolean, port?: number, token?: string }>({ running: false });
  const [cliPortInput, setCliPortInput] = useState("8080");
  const [showRoutingDetails, setShowRoutingDetails] = useState(false);
  const [memoryDisclosureEnabled, setMemoryDisclosureEnabled] = useState(true);
  const [lastDebugTraceId, setLastDebugTraceId] = useState<string | null>(null);
  const showDebugTelemetry = isBrowserHarnessRuntime() && isHarnessDebugEnabled();
  const memoryUsedTurnRef = useRef(false);

  const assistantLabel = agentName || "Abigail";
  const mountedRef = useRef(true);
  const messagesRef = useRef<Message[]>(messages);
  const inputRef = useRef(input);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    messagesRef.current = messages;
  }, [messages]);

  useEffect(() => {
    inputRef.current = input;
  }, [input]);

  useEffect(() => {
    setMessages(initialSession?.messages ?? []);
    setInput(initialSession?.input ?? "");
    if (textareaRef.current) {
      textareaRef.current.style.height = "auto";
    }
  }, [initialSession]);

  const autoGrow = useCallback(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = "auto";
    // Clamp to ~6 rows max (approx 144px at default line-height)
    el.style.height = Math.min(el.scrollHeight, 144) + "px";
  }, []);

  const refreshRouterStatus = () => {
    invoke<RouterStatus>("get_router_status")
      .then((status) => {
        if (!mountedRef.current) return;
        setRouterStatus(status);
        // Show config menu if no LLM configured
        if (!status.ego_configured && status.id_provider === "candle_stub") {
          setConfigStep("menu");
        } else {
          setConfigStep(null);
        }
      })
      .catch((err) => {
        console.error("[ChatInterface] get_router_status failed:", err);
      });
    // Also fetch Ollama status
    invoke<OllamaStatus>("get_ollama_status")
      .then((status) => {
        if (!mountedRef.current) return;
        setOllamaStatus(status);
      })
      .catch(() => {
        if (!mountedRef.current) return;
        setOllamaStatus(null);
      });

    // Fetch LM Studio status (using probe_local_llm)
    invoke<{ detected: { name: string; url: string; reachable: boolean }[] }>("probe_local_llm")
      .then((res) => {
        if (!mountedRef.current) return;
        setLmStudioStatus(res.detected.some((d) => d.name === "LM Studio" && d.reachable));
      })
      .catch(() => {
        if (!mountedRef.current) return;
        setLmStudioStatus(false);
      });

    // Fetch stored providers
    invoke<string[]>("get_stored_providers")
      .then((providers) => {
        if (!mountedRef.current) return;
        setStoredProviders(providers);
      })
      .catch(() => {
        if (!mountedRef.current) return;
        setStoredProviders([]);
      });

    // Fetch CLI server status
    invoke<{ running: boolean, port?: number, token?: string }>("get_cli_server_status")
      .then((status) => {
        if (!mountedRef.current) return;
        setCliServerStatus(status);
        if (status.port) setCliPortInput(status.port.toString());
      })
      .catch(() => {
        if (!mountedRef.current) return;
        setCliServerStatus({ running: false });
      });
  };

  const refreshMissingSecrets = () => {
    invoke<MissingSkillSecret[]>("list_missing_skill_secrets")
      .then((secrets) => {
        if (!mountedRef.current) return;
        setMissingSecrets(secrets);
      })
      .catch(() => {
        if (!mountedRef.current) return;
        setMissingSecrets([]);
      });
  };

  useEffect(() => {
    refreshRouterStatus();
    refreshMissingSecrets();

    // Listen for chat-status events from backend (e.g. tool execution)
    const unlisten = listen<{ status: string; tool: string; duration_ms?: number; error?: string; trace_id?: string }>("chat-status", (event) => {
      const { status, tool, duration_ms } = event.payload;
      if (event.payload.trace_id) {
        setLastDebugTraceId(event.payload.trace_id);
      }
      if (tool === "recall" && (status === "executing" || status === "done")) {
        memoryUsedTurnRef.current = true;
      }
      if (status === "done") {
        const dur = duration_ms ? ` (${(duration_ms / 1000).toFixed(1)}s)` : "";
        setChatStatus(`${tool} complete${dur}`);
        // Auto-clear after a brief display
        setTimeout(() => setChatStatus(null), 2000);
        return;
      }
      if (status === "error") {
        setChatStatus(`${tool} failed`);
        setShowRoutingDetails(true);
        setTimeout(() => setChatStatus(null), 3000);
        return;
      }
      // tool_executing status — show contextual messages
      const toolMessages: Record<string, string> = {
        web_search: "Searching the web...",
        perplexity_search: "Searching with Perplexity...",
        read_file: "Reading file...",
        write_file: "Writing file...",
        list_directory: "Listing directory...",
        http_get: "Fetching URL...",
        http_post: "Sending HTTP request...",
        execute: "Running command...",
      };
      setChatStatus(toolMessages[tool] || `Running ${tool}...`);
    });
    const unlistenRouting = listen<{ provider?: string; fallback_used?: boolean; safety_blocked?: boolean; error?: boolean; trace_id?: string }>(
      "chat-routing",
      (event) => {
        const payload = event.payload;
        if (payload.trace_id) {
          setLastDebugTraceId(payload.trace_id);
        }
        if (payload.fallback_used || payload.safety_blocked || payload.error) {
          setShowRoutingDetails(true);
        }
      }
    );
    invoke<{ enabled: boolean }>("get_memory_disclosure_settings")
      .then((v) => {
        if (!mountedRef.current) return;
        setMemoryDisclosureEnabled(v.enabled);
      })
      .catch(() => {
        if (!mountedRef.current) return;
        setMemoryDisclosureEnabled(true);
      });
    return () => {
      if (onSessionSnapshot) {
        onSessionSnapshot({
          messages: messagesRef.current,
          input: inputRef.current,
        });
      }
      mountedRef.current = false;
      unlisten.then((f) => f()).catch((e) => {
        console.warn("[ChatInterface] failed to unregister chat-status listener:", e);
      });
      unlistenRouting.then((f) => f()).catch((e) => {
        console.warn("[ChatInterface] failed to unregister chat-routing listener:", e);
      });
    };
  }, [onSessionSnapshot]);

  const handleConfigSelect = async (option: number) => {
    setConfigError("");
    setConfigInput("");
    
    const cliToVault: Record<string, string> = {
      "claude-cli": "anthropic",
      "gemini-cli": "google",
      "codex-cli": "openai",
      "grok-cli": "xai",
    };

    const checkAndUseStored = async (step: ConfigStep) => {
      if (!step) return false;
      const vaultKey = cliToVault[step];
      if (vaultKey && storedProviders.includes(vaultKey)) {
        try {
          await invoke("use_stored_provider", { provider: vaultKey });
          refreshRouterStatus();
          return true;
        } catch (e) {
          console.error("Failed to use stored provider:", e);
        }
      }
      return false;
    };

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
      case 4:
        if (!(await checkAndUseStored("claude-cli"))) {
          setConfigStep("claude-cli");
        }
        break;
      case 5:
        if (!(await checkAndUseStored("gemini-cli"))) {
          setConfigStep("gemini-cli");
        }
        break;
      case 6:
        if (!(await checkAndUseStored("codex-cli"))) {
          setConfigStep("codex-cli");
        }
        break;
      case 7:
        if (!(await checkAndUseStored("grok-cli"))) {
          setConfigStep("grok-cli");
        }
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
      } else if (configStep === "claude-cli" || configStep === "gemini-cli" || configStep === "codex-cli" || configStep === "grok-cli") {
        if (!configInput.trim()) {
          setConfigError("API key is required");
          return;
        }
        const res = await invoke<{ success: boolean, error: string }>("store_provider_key", { provider: configStep, key: configInput.trim(), validate: true });
        if (!res.success) {
          setConfigError(res.error || "Failed to store key");
          return;
        }
      }
      setConfigInput("");
      refreshRouterStatus();
    } catch (e) {
      setConfigError(String(e));
    }
  };

  const handleUseSystemAuth = async () => {
    if (!configStep) return;
    setConfigError("");
    try {
      const res = await invoke<{ success: boolean, error: string }>("store_provider_key", { 
        provider: configStep, 
        key: "system", 
        validate: false 
      });
      if (!res.success) {
        setConfigError(res.error || "Failed to set system auth");
        return;
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
    const sessionBeforeTurn = messagesRef.current
      .filter((m) => (m.role === "user" || m.role === "assistant") && m.content.trim().length > 0)
      .map((m) => ({ role: m.role, content: m.content }));
    memoryUsedTurnRef.current = false;
    setMessages((m) => [...m, userMessage]);
    setInput("");
    setLoading(true);
    // Reset textarea height after send
    if (textareaRef.current) {
      textareaRef.current.style.height = "auto";
    }

    // Add a placeholder assistant message for streaming
    setMessages((m) => [...m, { role: "assistant", content: "" }]);

        // Listen for streaming tokens. Coalesce UI updates to animation frames
        // so very fast token streams do not trigger visible flicker.
        let streamContent = "";
        let streamProvider = "";
        let unlisten: (() => void) | null = null;
        let rafId: number | null = null;
        let streamFlushTimerId: number | null = null;

        const flushStreamToUi = () => {
          setMessages((m) => {
            const updated = [...m];
            const lastAssistant = updated[updated.length - 1];
            if (lastAssistant && lastAssistant.role === "assistant") {
              updated[updated.length - 1] = {
                ...lastAssistant,
                content: streamContent,
                provider: streamProvider,
                memoryUsed: memoryUsedTurnRef.current,
              };
            }
            return updated;
          });
        };

        const scheduleStreamFlush = () => {
          // Throttle streaming paints to avoid rapid visual flashing in the chat list.
          if (streamFlushTimerId !== null) return;
          streamFlushTimerId = window.setTimeout(() => {
            streamFlushTimerId = null;
            if (rafId !== null) return;
            rafId = window.requestAnimationFrame(() => {
              rafId = null;
              flushStreamToUi();
            });
          }, 45);
        };
        try {
          unlisten = await listen<{ token?: string; provider?: string; done?: boolean }>("chat-token", (event) => {
            if (event.payload.token) {
              streamContent += event.payload.token;
              if (event.payload.provider) {
                streamProvider = event.payload.provider;
              }
              scheduleStreamFlush();
            }
          });
    
    } catch (listenErr) {
      console.warn("[ChatInterface] listen() failed:", listenErr);
    }

    try {
      const reply = await invoke<string>("chat_stream", {
        message: userMessage.content,
        target,
        sessionMessages: sessionBeforeTurn,
      });
      if (!mountedRef.current) return;
      // If streaming didn't produce content (fallback), use the return value
      if (!streamContent) {
        setMessages((m) => {
          const updated = [...m];
          const lastAssistant = updated[updated.length - 1];
          if (lastAssistant && lastAssistant.role === "assistant") {
            updated[updated.length - 1] = {
              ...lastAssistant,
              content: reply,
              memoryUsed: memoryUsedTurnRef.current,
            };
          }
          return updated;
        });
      } else {
        // Ensure final buffered tokens are rendered immediately.
        flushStreamToUi();
      }
    } catch (e) {
      if (!mountedRef.current) return;
      const errorMsg = String(e);
      let content = errorMsg;
      if (errorMsg.includes("No local LLM configured")) {
        content = "No LLM available. The bundled Ollama may still be starting.\n" +
          "Please wait a moment, or configure a provider:\n" +
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
      setShowRoutingDetails(true);
    } finally {
      if (streamFlushTimerId !== null) {
        window.clearTimeout(streamFlushTimerId);
      }
      if (rafId !== null) {
        window.cancelAnimationFrame(rafId);
      }
      try { if (unlisten) unlisten(); } catch { /* ignore */ }
      if (!mountedRef.current) return;
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
    let statusColor = "text-theme-warning";

    const egoName = routerStatus.ego_provider || "Cloud";
    const egoLabel = egoName.charAt(0).toUpperCase() + egoName.slice(1);

    const councilCount = routerStatus.council_providers || 0;

    if (mode === "tier_based" && hasEgo && hasLocal) {
      statusText = `[tier] ${egoLabel} + Local`;
      statusColor = "text-theme-text";
    } else if (mode === "tier_based" && hasEgo) {
      statusText = `[tier] ${egoLabel}`;
      statusColor = "text-theme-info";
    } else if (mode === "tier_based" && hasLocal) {
      statusText = "[tier] Local";
      statusColor = "text-theme-primary-dim";
    } else if (mode === "council" && councilCount > 1) {
      statusText = `[council: ${councilCount} providers]`;
      statusColor = "text-purple-400";
    } else if (hasEgo && hasLocal) {
      statusText = `[${mode}] ${egoLabel} + Local`;
      statusColor = "text-theme-text";
    } else if (hasEgo) {
      statusText = `[cloud] ${egoLabel}`;
      statusColor = "text-theme-info";
    } else if (hasLocal) {
      // Show "bundled" label when the local LLM is from managed Ollama
      if (ollamaStatus?.managed && ollamaStatus?.running) {
        statusText = "[local: bundled]";
      } else {
        statusText = `[local] ${routerStatus.id_url}`;
      }
      statusColor = "text-theme-primary-dim";
    } else if (ollamaStatus?.managed && !ollamaStatus?.model_ready) {
      statusText = "Starting local AI...";
      statusColor = "text-theme-warning";
    } else {
      statusText = "[no LLM] Press 1-7 to configure";
      statusColor = "text-theme-danger";
    }

    return (
      <div
        className={`text-xs ${statusColor} px-4 py-1 border-b border-theme-border cursor-pointer hover:bg-theme-surface`}
        onClick={() => setConfigStep("menu")}
      >
        {!showRoutingDetails && !statusText.includes("[no LLM]") ? "[routing hidden]" : statusText}
      </div>
    );
  };

  const renderConfigMenu = () => {
    if (configStep === "menu") {
      const isOllamaAvailable = ollamaStatus?.running;
      const isLmStudioAvailable = lmStudioStatus;
      
      const getHighlightClass = (available: boolean, authenticated: boolean) => {
        if (authenticated) return "border-green-600 bg-green-950/20";
        if (available) return "border-theme-primary bg-theme-primary-glow";
        return "border-theme-primary-faint";
      };

      return (
        <div className="p-4 border-b border-theme-border bg-theme-surface">
          <p className="text-theme-primary-dim mb-3">Configure LLM Provider:</p>
          <div className="space-y-2">
            <div className="flex gap-2">
              <button
                className={`flex-1 text-left px-3 py-2 border rounded hover:bg-theme-surface ${getHighlightClass(!!isOllamaAvailable, false)}`}
                onClick={() => handleConfigSelect(1)}
              >
                <span className="text-theme-text-bright">[1]</span> Ollama (local, default port 11434)
                {isOllamaAvailable && <span className="ml-2 text-xs text-green-500 font-bold">● Running</span>}
              </button>
              <a href="https://ollama.com" target="_blank" rel="noreferrer" className="px-3 py-2 border border-theme-border-dim rounded hover:text-theme-primary text-xs flex items-center">Docs</a>
            </div>

            <div className="flex gap-2">
              <button
                className={`flex-1 text-left px-3 py-2 border rounded hover:bg-theme-surface ${getHighlightClass(isLmStudioAvailable, false)}`}
                onClick={() => handleConfigSelect(2)}
              >
                <span className="text-theme-text-bright">[2]</span> LM Studio (local, default port 1234)
                {isLmStudioAvailable && <span className="ml-2 text-xs text-green-500 font-bold">● Running</span>}
              </button>
              <a href="https://lmstudio.ai" target="_blank" rel="noreferrer" className="px-3 py-2 border border-theme-border-dim rounded hover:text-theme-primary text-xs flex items-center">Docs</a>
            </div>

            <div className="flex gap-2">
              <button
                className={`flex-1 text-left px-3 py-2 border rounded hover:bg-theme-surface ${getHighlightClass(false, storedProviders.includes("openai"))}`}
                onClick={() => handleConfigSelect(3)}
              >
                <span className="text-theme-text-bright">[3]</span> OpenAI (cloud, requires API key)
                {storedProviders.includes("openai") && <span className="ml-2 text-xs text-green-500">✓ Auth</span>}
              </button>
              <a href="https://platform.openai.com" target="_blank" rel="noreferrer" className="px-3 py-2 border border-theme-border-dim rounded hover:text-theme-primary text-xs flex items-center">Docs</a>
            </div>

            <div className="border-t border-theme-border-dim my-2 pt-2">
              <p className="text-theme-primary-dim text-xs mb-2 uppercase tracking-wider">CLI / API Access (Local Server):</p>
              <div className="flex gap-2 items-center mb-2">
                <span className="text-theme-text-dim text-xs">Port:</span>
                <input
                  type="text"
                  className="w-16 bg-theme-input-bg border border-theme-border-dim text-theme-text px-2 py-1 rounded text-xs focus:border-theme-primary outline-none"
                  value={cliPortInput}
                  onChange={(e) => setCliPortInput(e.target.value)}
                  disabled={cliServerStatus.running}
                />
                <button
                  className={`px-3 py-1 rounded text-xs border ${
                    cliServerStatus.running
                      ? "border-red-600 text-red-400 hover:bg-red-950/20"
                      : "border-theme-primary text-theme-primary hover:bg-theme-primary-glow"
                  }`}
                  onClick={async () => {
                    if (cliServerStatus.running) {
                      await invoke("stop_cli_server");
                    } else {
                      try {
                        await invoke("start_cli_server", { port: parseInt(cliPortInput) || 8080 });
                      } catch (e) {
                        alert(String(e));
                      }
                    }
                    refreshRouterStatus();
                  }}
                >
                  {cliServerStatus.running ? "Stop Server" : "Start Server"}
                </button>
              </div>
              {cliServerStatus.running && (
                <div className="bg-black/40 p-2 rounded text-[10px] space-y-1 border border-theme-border-dim">
                  <p className="text-green-500 font-bold">● API Active at http://localhost:{cliServerStatus.port}</p>
                  <p className="text-theme-text-dim">Token: <span className="text-theme-text-bright select-all">{cliServerStatus.token}</span></p>
                  <div className="pt-1 text-theme-primary-dim">
                    Example: <code className="text-theme-text-bright break-all">curl -H &quot;Authorization: Bearer {cliServerStatus.token}&quot; -X POST -H &quot;Content-Type: application/json&quot; -d &#123;&quot;message&quot;:&quot;Hello&quot;&#125; http://localhost:{cliServerStatus.port}/chat</code>
                  </div>
                </div>
              )}
            </div>

            <div className="border-t border-theme-border-dim my-2 pt-2">
              <p className="text-theme-text-dim text-xs mb-2 uppercase tracking-wider">CLI Providers (via external tools):</p>
            </div>

            <div className="flex gap-2">
              <button
                className={`flex-1 text-left px-3 py-2 border rounded hover:bg-theme-surface ${getHighlightClass(false, storedProviders.includes("anthropic"))}`}
                onClick={() => handleConfigSelect(4)}
              >
                <span className="text-theme-text-bright">[4]</span> Claude Code CLI (Anthropic key)
                {storedProviders.includes("anthropic") && <span className="ml-2 text-xs text-green-500 font-bold">✓ Ready</span>}
              </button>
              <a href="https://docs.anthropic.com/en/docs/claude-code" target="_blank" rel="noreferrer" className="px-3 py-2 border border-theme-border-dim rounded hover:text-theme-primary text-xs flex items-center">Docs</a>
            </div>

            <div className="flex gap-2">
              <button
                className={`flex-1 text-left px-3 py-2 border rounded hover:bg-theme-surface ${getHighlightClass(false, storedProviders.includes("google"))}`}
                onClick={() => handleConfigSelect(5)}
              >
                <span className="text-theme-text-bright">[5]</span> Gemini CLI (Google key)
                {storedProviders.includes("google") && <span className="ml-2 text-xs text-green-500 font-bold">✓ Ready</span>}
              </button>
              <a href="https://github.com/google-gemini/gemini-cli" target="_blank" rel="noreferrer" className="px-3 py-2 border border-theme-border-dim rounded hover:text-theme-primary text-xs flex items-center">Docs</a>
            </div>

            <div className="flex gap-2">
              <button
                className={`flex-1 text-left px-3 py-2 border rounded hover:bg-theme-surface ${getHighlightClass(false, storedProviders.includes("openai"))}`}
                onClick={() => handleConfigSelect(6)}
              >
                <span className="text-theme-text-bright">[6]</span> Codex CLI (OpenAI key)
                {storedProviders.includes("openai") && <span className="ml-2 text-xs text-green-500 font-bold">✓ Ready</span>}
              </button>
              <a href="https://github.com/openai/codex" target="_blank" rel="noreferrer" className="px-3 py-2 border border-theme-border-dim rounded hover:text-theme-primary text-xs flex items-center">Docs</a>
            </div>

            <div className="flex gap-2">
              <button
                className={`flex-1 text-left px-3 py-2 border rounded hover:bg-theme-surface ${getHighlightClass(false, storedProviders.includes("xai"))}`}
                onClick={() => handleConfigSelect(7)}
              >
                <span className="text-theme-text-bright">[7]</span> Grok CLI (xAI key)
                {storedProviders.includes("xai") && <span className="ml-2 text-xs text-green-500 font-bold">✓ Ready</span>}
              </button>
              <a href="https://docs.x.ai/docs/grok-cli" target="_blank" rel="noreferrer" className="px-3 py-2 border border-theme-border-dim rounded hover:text-theme-primary text-xs flex items-center">Docs</a>
            </div>
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
              className="flex-1 bg-theme-input-bg border border-theme-border-dim text-theme-text px-3 py-2 rounded max-w-[100px] focus:border-theme-primary focus:ring-1 focus:ring-theme-focus-ring"
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
              className="flex-1 bg-theme-input-bg border border-theme-border-dim text-theme-text px-3 py-2 rounded focus:border-theme-primary focus:ring-1 focus:ring-theme-focus-ring"
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

    if (configStep === "claude-cli" || configStep === "gemini-cli" || configStep === "codex-cli" || configStep === "grok-cli") {
      const cliLabels: Record<string, { label: string; placeholder: string }> = {
        "claude-cli": { label: "Claude Code CLI", placeholder: "sk-ant-..." },
        "gemini-cli": { label: "Gemini CLI", placeholder: "AIza..." },
        "codex-cli": { label: "Codex CLI", placeholder: "sk-..." },
        "grok-cli": { label: "Grok CLI", placeholder: "xai-..." },
      };
      const cli = cliLabels[configStep];
      return (
        <div className="p-4 border-b border-theme-border bg-theme-surface">
          <p className="text-theme-primary-dim mb-2">{cli.label} Configuration:</p>
          <p className="text-theme-text-dim text-xs mb-2">Uses the same API key as the cloud provider.</p>
          <div className="flex gap-2">
            <input
              type="password"
              className="flex-1 bg-theme-input-bg border border-theme-border-dim text-theme-text px-3 py-2 rounded focus:border-theme-primary focus:ring-1 focus:ring-theme-focus-ring"
              placeholder={cli.placeholder}
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
              className="border border-theme-primary-faint px-3 py-2 rounded hover:bg-theme-surface text-theme-text-dim text-xs"
              onClick={handleUseSystemAuth}
              title="Use the CLI's internal authentication (OAuth)"
            >
              Use OAuth
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
    <div className="h-full bg-theme-bg text-theme-text font-mono flex flex-col">
      {getStatusIndicator()}
      <div className="px-4 py-1 text-[11px] border-b border-theme-border flex items-center gap-3 bg-theme-bg-elevated">
        <button
          className="text-theme-text-dim hover:text-theme-text"
          onClick={() => setShowRoutingDetails((v) => !v)}
        >
          {showRoutingDetails ? "Hide routing details" : "Show routing details"}
        </button>
        <button
          className="text-theme-text-dim hover:text-theme-text"
          onClick={async () => {
            const next = !memoryDisclosureEnabled;
            setMemoryDisclosureEnabled(next);
            try {
              await invoke("set_memory_disclosure_settings", { enabled: next });
            } catch {
              // keep UI responsive even if persistence fails
            }
          }}
        >
          Memory disclosure: {memoryDisclosureEnabled ? "On" : "Off"}
        </button>
        {showDebugTelemetry && (
          <span className="text-theme-text-dim">
            trace: {lastDebugTraceId ?? "none"}
          </span>
        )}
      </div>
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
            className={`flex ${msg.role === "user" ? "justify-end" : "justify-start"}`}
          >
            <div
              className={`max-w-[80%] px-4 py-2.5 text-sm ${
                msg.isError
                  ? "bg-theme-danger-dim border border-red-800 rounded-xl"
                  : msg.role === "user"
                    ? "bg-theme-bubble-user rounded-xl rounded-br-sm"
                    : "bg-theme-bubble-assistant rounded-xl rounded-bl-sm"
              }`}
            >
              <p className={`text-xs mb-1 ${msg.isError ? "text-red-400" : "text-theme-text-dim"}`}>
                {msg.role === "user" ? "You" : (
                  <>
                    {assistantLabel}
                    {showRoutingDetails && msg.provider && <span className="ml-2 opacity-50 font-normal">via {msg.provider}</span>}
                  </>
                )}
              </p>
              <span className={msg.isError ? "text-red-300" : "text-theme-text-bright"}>
                {redactApiKeys(msg.content || "").split("\n").map((line, j) => (
                  <span key={j}>
                    {line}
                    {j < (msg.content || "").split("\n").length - 1 && <br />}
                  </span>
                ))}
              </span>
              {memoryDisclosureEnabled && msg.role === "assistant" && msg.memoryUsed && (
                <p className="text-[10px] mt-2 text-theme-text-dim">
                  Memory disclosure: this response used recall context.
                </p>
              )}
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
          </div>
        ))}
        {loading && <ThinkingIndicator status={chatStatus} label={assistantLabel} />}
      </div>
      <div className="p-4 border-t border-theme-border flex gap-2 items-end">
        <textarea
          ref={textareaRef}
          aria-label="Message input"
          className="flex-1 bg-theme-input-bg border border-theme-border-dim text-theme-text px-3 py-2 rounded resize-none overflow-y-auto focus:border-theme-primary focus:ring-1 focus:ring-theme-focus-ring"
          placeholder="Message"
          rows={1}
          value={input}
          onChange={(e) => {
            setInput(e.target.value);
            autoGrow();
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              send();
            }
          }}
        />
        <button
          aria-label="Send message"
          className="border border-theme-primary px-4 py-2 rounded hover:bg-theme-primary-glow"
          onClick={send}
        >
          Send
        </button>
      </div>
    </div>
  );
}
