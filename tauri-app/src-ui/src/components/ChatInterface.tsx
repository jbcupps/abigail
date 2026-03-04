import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { useState, useEffect, useRef, useCallback, useMemo } from "react";
import { useTheme } from "../contexts/ThemeContext";
import McpAppFrame from "./McpAppFrame";
import ThinkingIndicator from "./ThinkingIndicator";
import VaultModal, { type MissingSkillSecret } from "./VaultModal";
import { isBrowserHarnessRuntime, isHarnessDebugEnabled } from "../runtimeMode";
import { createChatGateway } from "../chat/createChatGateway";
import {
  isInterruptedByUserMessage,
  type ChatGatewayStream,
  type ExecutionTrace,
} from "../chat/chatGateway";

interface Attachment {
  filename: string;
  content: string;
  contentType: string;
  sizeBytes: number;
  truncated: boolean;
  filePath: string;
  savedPath?: string;
}

/** Normalize raw trace/provider labels for chat-facing display.
 *  Id is a background system function, never a conversational actor. */
function normalizeProviderLabel(raw: string): string {
  if (raw === "id" || raw.startsWith("id(")) return "local";
  return raw;
}

interface Message {
  role: "user" | "assistant";
  content: string;
  provider?: string;
  isError?: boolean;
  memoryUsed?: boolean;
  /** Model quality tier: "fast", "standard", or "pro". */
  tier?: string;
  /** Actual model ID used (e.g. "gpt-4.1", "claude-sonnet-4-6"). */
  modelUsed?: string;
  /** When set, render an MCP App (ui:// resource) in a sandboxed iframe below the message. */
  mcpApp?: { serverId: string; resourceUri: string; title?: string };
  /** Authoritative execution trace for this turn. */
  executionTrace?: ExecutionTrace;
}

export interface ChatSessionSnapshot {
  messages: Message[];
  input: string;
  sessionId: string;
}

interface RouterStatus {
  id_provider: string;
  id_url: string | null;
  ego_configured: boolean;
  ego_provider: string | null;
  superego_configured: boolean;
  routing_mode: string;
}

interface OllamaStatus {
  managed: boolean;
  running: boolean;
  port: number;
  model_ready: boolean;
}

interface ChatInterfaceProps {
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

type ModelRegistryEntry = { provider: string; model_id: string; display_name: string | null };

const FALLBACK_MODELS: Record<string, string[]> = {
  openai: ["gpt-4.1", "gpt-4.1-mini", "gpt-4o", "gpt-4o-mini", "o3-mini", "o4-mini"],
  anthropic: ["claude-sonnet-4-6", "claude-haiku-4-5", "claude-opus-4-6"],
  google: ["gemini-2.5-flash", "gemini-2.5-pro", "gemini-2.0-flash"],
  xai: ["grok-3", "grok-3-mini"],
  perplexity: ["sonar", "sonar-pro", "sonar-reasoning-pro"],
};

export default function ChatInterface({
  initialSession = null,
  onSessionSnapshot,
}: ChatInterfaceProps) {
  const { agentName } = useTheme();
  const [messages, setMessages] = useState<Message[]>(() => initialSession?.messages ?? []);
  const [input, setInput] = useState(() => initialSession?.input ?? "");
  const [sessionId, setSessionId] = useState<string>(() => initialSession?.sessionId ?? crypto.randomUUID());
  const [loading, setLoading] = useState(false);
  const [interrupting, setInterrupting] = useState(false);
  const [routerStatus, setRouterStatus] = useState<RouterStatus | null>(null);
  const [missingSecrets, setMissingSecrets] = useState<MissingSkillSecret[]>([]);
  const [activeSecret, setActiveSecret] = useState<MissingSkillSecret | null>(null);
  const [chatStatus, setChatStatus] = useState<string | null>(null);
  const [ollamaStatus, setOllamaStatus] = useState<OllamaStatus | null>(null);
  const [storedProviders, setStoredProviders] = useState<string[]>([]);
  const [showRoutingDetails, setShowRoutingDetails] = useState(false);
  const [memoryDisclosureEnabled, setMemoryDisclosureEnabled] = useState(true);
  const [selectedModel, setSelectedModel] = useState<string>("");
  const [modelRegistry, setModelRegistry] = useState<ModelRegistryEntry[]>([]);
  const [headerProvider, setHeaderProvider] = useState<string>("");
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [dragOver, setDragOver] = useState(false);
  const [jobCounts, setJobCounts] = useState<{ running: number; queued: number; scheduled: number }>({ running: 0, queued: 0, scheduled: 0 });
  const showDebugTelemetry = isBrowserHarnessRuntime() && isHarnessDebugEnabled();
  const chatGateway = useMemo(() => createChatGateway(), []);

  const assistantLabel = agentName || "Abigail";
  const mountedRef = useRef(true);
  const messagesRef = useRef<Message[]>(messages);
  const inputRef = useRef(input);
  const sessionIdRef = useRef(sessionId);
  const activeChatRef = useRef<ChatGatewayStream | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const autoLocalBootstrapRef = useRef(false);

  useEffect(() => {
    messagesRef.current = messages;
  }, [messages]);

  useEffect(() => {
    inputRef.current = input;
  }, [input]);

  useEffect(() => {
    sessionIdRef.current = sessionId;
  }, [sessionId]);

  useEffect(() => {
    setMessages(initialSession?.messages ?? []);
    setInput(initialSession?.input ?? "");
    setSessionId(initialSession?.sessionId ?? crypto.randomUUID());
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

  const ingestFile = useCallback(async (filePath: string) => {
    try {
      const result = await invoke<{
        content: string;
        content_type: string;
        filename: string;
        size_bytes: number;
        truncated: boolean;
      }>("upload_chat_attachment", { filePath });

      const attachment: Attachment = {
        filename: result.filename,
        content: result.content,
        contentType: result.content_type,
        sizeBytes: result.size_bytes,
        truncated: result.truncated,
        filePath,
      };

      // Save to entity docs folder in background
      try {
        const savedPath = await invoke<string>("save_to_entity_docs", {
          sourcePath: filePath,
          filename: result.filename,
        });
        attachment.savedPath = savedPath;
      } catch (e) {
        console.warn("[ChatInterface] save_to_entity_docs failed:", e);
      }

      setAttachments((prev) => [...prev, attachment]);
    } catch (e) {
      console.error("[ChatInterface] upload_chat_attachment failed:", e);
    }
  }, []);

  const handleDrop = useCallback(
    async (e: React.DragEvent) => {
      e.preventDefault();
      e.stopPropagation();
      setDragOver(false);

      const files = e.dataTransfer?.files;
      if (!files || files.length === 0) return;

      for (let i = 0; i < files.length; i++) {
        const file = files[i];
        // Tauri exposes the native path on File objects from OS drops
        const path = (file as File & { path?: string }).path;
        if (path) {
          await ingestFile(path);
        }
      }
    },
    [ingestFile]
  );

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setDragOver(true);
  }, []);

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setDragOver(false);
  }, []);

  const openFilePicker = useCallback(async () => {
    try {
      const selected = await open({ multiple: true });
      if (!selected) return;
      const paths = Array.isArray(selected) ? selected : [selected];
      for (const filePath of paths) {
        if (typeof filePath === "string") {
          await ingestFile(filePath);
        }
      }
    } catch (e) {
      console.error("[ChatInterface] file picker failed:", e);
    }
  }, [ingestFile]);

  const removeAttachment = useCallback((index: number) => {
    setAttachments((prev) => prev.filter((_, i) => i !== index));
  }, []);

  const formatFileSize = useCallback((bytes: number) => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }, []);

  const refreshRouterStatus = () => {
    invoke<RouterStatus>("get_router_status")
      .then((status) => {
        if (!mountedRef.current) return;
        setRouterStatus(status);
      })
      .catch((err) => {
        console.error("[ChatInterface] get_router_status failed:", err);
      });
    // Also fetch Ollama status
    invoke<OllamaStatus>("get_ollama_status")
      .then(async (status) => {
        if (!mountedRef.current) return;
        setOllamaStatus(status);
        if (!status.running || autoLocalBootstrapRef.current) return;

        // Bundled/local Ollama is up: automatically wire local URL when no provider is configured.
        const current = await invoke<RouterStatus>("get_router_status").catch(() => null);
        if (!current) return;
        if (current.ego_configured || current.id_provider !== "candle_stub") return;

        autoLocalBootstrapRef.current = true;
        try {
          await invoke("set_local_llm_url", { url: "http://localhost:11434" });
          const next = await invoke<RouterStatus>("get_router_status");
          if (!mountedRef.current) return;
          setRouterStatus(next);
        } catch (e) {
          console.warn("[ChatInterface] auto local bootstrap failed:", e);
        }
      })
      .catch(() => {
        if (!mountedRef.current) return;
        setOllamaStatus(null);
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

    invoke<{ enabled: boolean }>("get_memory_disclosure_settings")
      .then((v) => {
        if (!mountedRef.current) return;
        setMemoryDisclosureEnabled(v.enabled);
      })
      .catch(() => {
        if (!mountedRef.current) return;
        setMemoryDisclosureEnabled(true);
      });

    // Fetch model registry for header dropdowns
    invoke<{ models: ModelRegistryEntry[] }>("get_model_registry")
      .then((reg) => {
        if (!mountedRef.current || !reg) return;
        setModelRegistry(reg.models ?? []);
      })
      .catch(() => {});

    return () => {
      const activeChat = activeChatRef.current;
      activeChatRef.current = null;
      if (activeChat) {
        void activeChat.dispose();
      }
      if (onSessionSnapshot) {
        onSessionSnapshot({
          messages: messagesRef.current,
          input: inputRef.current,
          sessionId: sessionIdRef.current,
        });
      }
      mountedRef.current = false;
    };
  }, [onSessionSnapshot]);

  // -- Job activity badge --
  useEffect(() => {
    const fetchStats = () => {
      invoke<{ running: number; queued: number; scheduled: number }>("get_queue_stats")
        .then((s) => { if (mountedRef.current) setJobCounts({ running: s.running, queued: s.queued, scheduled: s.scheduled }); })
        .catch(() => {});
    };
    fetchStats();
    let unlisten: (() => void) | null = null;
    listen("job-event", () => fetchStats()).then((fn) => { unlisten = fn; });
    return () => { unlisten?.(); };
  }, []);

  // -- Header model/provider dropdown logic --

  const isCliMode = routerStatus?.routing_mode === "cli_orchestrator";

  // Sync headerProvider with the active ego provider from the router.
  // When the backend provider changes (e.g. after use_stored_provider or startup),
  // we update the dropdown to match.
  useEffect(() => {
    if (!routerStatus?.ego_provider) return;
    setHeaderProvider(routerStatus.ego_provider);
  }, [routerStatus?.ego_provider]);

  const providers = storedProviders ?? [];
  const knownProviders = useMemo(() => {
    // Merge providers from model registry + vault (stored API keys).
    // CLI providers are excluded from the selector — they show as a static label.
    const fromReg = new Set(modelRegistry.map((m) => m.provider));
    const fromVault = new Set(providers.filter((p) => !p.endsWith("-cli")));
    const merged = new Set([...fromReg, ...fromVault]);
    if (merged.size > 0) {
      // Ensure the currently active non-CLI provider is always in the list
      if (headerProvider && !headerProvider.endsWith("-cli") && !merged.has(headerProvider)) {
        merged.add(headerProvider);
      }
      return [...merged].sort();
    }
    return Object.keys(FALLBACK_MODELS);
  }, [modelRegistry, providers, headerProvider]);

  const headerModels = useMemo(() => {
    if (!headerProvider) return [];
    // CLI providers don't expose models via the registry — skip
    if (headerProvider.endsWith("-cli")) return [];
    const fromReg = modelRegistry.filter((m) => m.provider === headerProvider);
    if (fromReg.length > 0) return fromReg.map((m) => m.model_id);
    return FALLBACK_MODELS[headerProvider] ?? [];
  }, [headerProvider, modelRegistry]);

  const applyChatError = (errorMsg: string) => {
    const interrupted = isInterruptedByUserMessage(errorMsg);
    if (interrupted) {
      setMessages((m) => {
        const updated = [...m];
        const last = updated[updated.length - 1];
        if (last && last.role === "assistant") {
          const hasPartial = last.content.trim().length > 0;
          updated[updated.length - 1] = {
            ...last,
            content: hasPartial ? `${last.content}\n\n[Interrupted]` : "[Interrupted]",
            isError: false,
          };
        }
        return updated;
      });
      setLoading(false);
      setChatStatus("Interrupted");
      return;
    }

    let content = errorMsg;
    if (errorMsg.includes("No local LLM configured")) {
      content =
        "No LLM available. The bundled Ollama may still be starting.\n" +
        "Please wait a moment, or configure a provider:\n" +
        "1. Set OPENAI_API_KEY environment variable, or\n" +
        "2. Install Ollama and set LOCAL_LLM_BASE_URL=http://localhost:11434";
    } else if (
      errorMsg.includes("No models loaded") ||
      errorMsg.includes("no model loaded") ||
      errorMsg.includes("model not found")
    ) {
      content =
        "Your local LLM server is running but has no model loaded.\n" +
        "Please load a model in LM Studio or run `lms load <model>`, then try again.";
    } else if (
      errorMsg.includes("Connection refused") ||
      errorMsg.includes("connection refused") ||
      errorMsg.includes("error sending request")
    ) {
      content =
        "Cannot reach the local LLM server.\n" +
        "Make sure LM Studio or Ollama is running, or configure a cloud provider in Settings.";
    }

    setMessages((m) => {
      const updated = [...m];
      const last = updated[updated.length - 1];
      if (last && last.role === "assistant") {
        updated[updated.length - 1] = { ...last, content, isError: true };
      } else {
        updated.push({ role: "assistant", content, isError: true });
      }
      return updated;
    });
    setShowRoutingDetails(true);
    setLoading(false);
    setChatStatus(null);
  };

  const interruptCurrentResponse = async () => {
    if (!loading || interrupting) return;
    const activeChat = activeChatRef.current;
    if (!activeChat) return;
    setInterrupting(true);
    try {
      await activeChat.cancel();
    } catch (e) {
      console.warn("[ChatInterface] chat cancel failed:", e);
    } finally {
      setInterrupting(false);
    }
  };

  const send = async () => {
    if ((!input.trim() && attachments.length === 0) || loading) return;

    // Build message content: prepend attachment blocks, then user text
    let messageContent = "";
    for (const att of attachments) {
      const truncNote = att.truncated ? " (truncated)" : "";
      messageContent += `[File: ${att.filename} (${formatFileSize(att.sizeBytes)})${truncNote}]\n`;
      if (att.savedPath) {
        messageContent += `[Saved to: ${att.savedPath}]\n`;
      }
      messageContent += `${att.content}\n\n`;
    }
    messageContent += input.trim();

    const userMessage: Message = { role: "user", content: messageContent };
    const sessionBeforeTurn = messagesRef.current
      .filter((m) => (m.role === "user" || m.role === "assistant") && m.content.trim().length > 0)
      .map((m) => ({ role: m.role, content: m.content }));

    const staleChat = activeChatRef.current;
    if (staleChat) {
      activeChatRef.current = null;
      await staleChat.dispose();
    }

    setMessages((m) => [...m, userMessage, { role: "assistant", content: "" }]);
    setInput("");
    setAttachments([]);
    setLoading(true);
    if (textareaRef.current) {
      textareaRef.current.style.height = "auto";
    }

    try {
      const stream = await chatGateway.send(
        {
          message: userMessage.content,
          sessionMessages: sessionBeforeTurn,
          sessionId,
          modelOverride: (!isCliMode && selectedModel) ? selectedModel : undefined,
        },
        {
          onToken: (token) => {
            if (!mountedRef.current) return;
            setMessages((m) => {
              const updated = [...m];
              const last = updated[updated.length - 1];
              if (last && last.role === "assistant") {
                updated[updated.length - 1] = { ...last, content: last.content + token };
              }
              return updated;
            });
          },
          onDone: (resp) => {
            if (!mountedRef.current) return;
            activeChatRef.current = null;
            setMessages((m) => {
              const updated = [...m];
              const last = updated[updated.length - 1];
              if (last && last.role === "assistant") {
                updated[updated.length - 1] = {
                  ...last,
                  content: last.content || resp.reply,
                  provider: resp.provider,
                  tier: resp.tier,
                  modelUsed: resp.model_used,
                  executionTrace: resp.execution_trace as ExecutionTrace | undefined,
                };
              }
              return updated;
            });
            if (resp.session_id && resp.session_id !== sessionIdRef.current) {
              setSessionId(resp.session_id);
            }
            setLoading(false);
            setChatStatus(null);
          },
          onError: (error) => {
            if (!mountedRef.current) return;
            activeChatRef.current = null;
            applyChatError(error.message);
          },
        },
      );

      if (!mountedRef.current) {
        await stream.dispose();
        return;
      }
      activeChatRef.current = stream;
    } catch (e) {
      if (!mountedRef.current) return;
      activeChatRef.current = null;
      applyChatError(String(e));
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

    if (mode === "cli_orchestrator" && hasEgo) {
      const cliLabel = egoName.replace("-cli", "").replace("_cli", "");
      statusText = `[cli orchestrator] ${cliLabel.charAt(0).toUpperCase() + cliLabel.slice(1)} Code`;
      statusColor = "text-theme-info";
    } else if (hasEgo && hasLocal) {
      statusText = `${egoLabel} + Local`;
      statusColor = "text-theme-text";
    } else if (hasEgo) {
      statusText = `[cloud] ${egoLabel}`;
      statusColor = "text-theme-info";
    } else if (hasLocal) {
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
      statusText = "[no LLM] configure in settings";
      statusColor = "text-theme-danger";
    }

    return (
      <div
        className={`text-xs ${statusColor} px-4 py-1 border-b border-theme-border`}
      >
        {!showRoutingDetails && !statusText.includes("[no LLM]") ? "[routing hidden]" : statusText}
      </div>
    );
  };

  return (
    <div
      className={`h-full bg-theme-bg text-theme-text font-primary flex flex-col relative ${dragOver ? "ring-2 ring-inset ring-theme-primary" : ""}`}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
    >
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
        <span className="mx-1 text-theme-border">|</span>
        {isCliMode ? (
          <span className="text-[11px] text-theme-text-dim px-1 font-mono">
            {headerProvider || "cli"}
          </span>
        ) : (
          <>
            <select
              className="bg-transparent text-[11px] text-theme-text-dim border border-theme-border rounded px-1 cursor-pointer"
              value={headerProvider}
              onChange={(e) => {
                const provider = e.target.value;
                setHeaderProvider(provider);
                setSelectedModel("");
                invoke("use_stored_provider", { provider }).catch((err: unknown) => {
                  console.error("[ChatInterface] use_stored_provider failed:", err);
                });
              }}
            >
              {knownProviders.map((p) => (
                <option key={p} value={p}>{p}</option>
              ))}
            </select>
            <select
              className="bg-transparent text-[11px] text-theme-text-dim border border-theme-border rounded px-1 cursor-pointer"
              value={selectedModel}
              onChange={(e) => setSelectedModel(e.target.value)}
            >
              <option value="">Auto</option>
              {headerModels.map((m) => (
                <option key={m} value={m}>{m}</option>
              ))}
            </select>
          </>
        )}
        {(jobCounts.running > 0 || jobCounts.queued > 0 || jobCounts.scheduled > 0) && (
          <span className="ml-1 inline-flex items-center gap-1 text-[10px] font-mono border border-theme-border rounded px-1.5 py-0.5 text-theme-text-dim">
            {jobCounts.running > 0 && (
              <span className="text-theme-info">● {jobCounts.running}</span>
            )}
            {jobCounts.queued > 0 && (
              <span className="text-theme-warning">○ {jobCounts.queued}</span>
            )}
            {jobCounts.scheduled > 0 && (
              <span className="text-theme-primary-dim">↻ {jobCounts.scheduled}</span>
            )}
          </span>
        )}
        {showDebugTelemetry && (
          <span className="text-theme-text-dim">
            mode: non-streaming
          </span>
        )}
      </div>
      {missingSecrets.length > 0 && (
        <div className="px-4 py-2 border-b border-theme-warning bg-theme-warning-dim">
          <p className="text-theme-warning text-xs mb-1">Skills need setup:</p>
          {missingSecrets.map((s, i) => (
            <button
              key={i}
              className="text-xs text-theme-warning hover:text-theme-warning mr-3 underline"
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
                  ? "bg-theme-danger-dim border border-theme-danger rounded-xl"
                  : msg.role === "user"
                    ? "bg-theme-bubble-user rounded-xl rounded-br-sm"
                    : "bg-theme-bubble-assistant rounded-xl rounded-bl-sm"
              }`}
            >
              <p className={`text-xs mb-1 ${msg.isError ? "text-theme-danger" : "text-theme-text-dim"}`}>
                {msg.role === "user" ? "You" : (
                  <>
                    {assistantLabel}
                    {showRoutingDetails && msg.executionTrace ? (() => {
                      const trace = msg.executionTrace!;
                      const finalStep = trace.steps[trace.final_step_index];
                      const ranLabel = normalizeProviderLabel(finalStep?.provider_label ?? msg.provider ?? "unknown");
                      const tierLabel = trace.fallback_occurred ? null : (trace.configured_tier ?? msg.tier);
                      const modelLabel = finalStep?.model_requested ?? msg.modelUsed;
                      const ts = new Date(trace.timestamp_utc).toLocaleTimeString();
                      const reason = trace.selection_reason;
                      const isPinned = reason === "pinned_model" || reason === "pinned_tier";
                      return (
                        <>
                          <span className="ml-2 opacity-50 font-normal">
                            via {ranLabel}
                          </span>
                          {modelLabel && (
                            <span className="ml-2 text-[10px] px-1.5 py-0.5 rounded bg-theme-input-bg text-theme-text-dim font-mono">
                              {tierLabel ? `${tierLabel.charAt(0).toUpperCase()}${tierLabel.slice(1)} · ` : ""}{modelLabel}
                            </span>
                          )}
                          {isPinned && (
                            <span className="ml-1 text-[10px] px-1 py-0.5 rounded bg-theme-info-dim text-theme-info font-mono">
                              pinned
                            </span>
                          )}
                          {reason === "setup_intent" && (
                            <span className="ml-1 text-[10px] px-1 py-0.5 rounded bg-theme-info-dim text-theme-info font-mono">
                              setup
                            </span>
                          )}
                          <span className="ml-2 text-[10px] opacity-40">{ts}</span>
                          {trace.fallback_occurred && (
                            <span className="ml-1 text-[10px] px-1 py-0.5 rounded bg-theme-warning-dim text-theme-warning font-mono">
                              fallback
                            </span>
                          )}
                        </>
                      );
                    })() : (
                      <>
                        {showRoutingDetails && msg.provider && <span className="ml-2 opacity-50 font-normal">via {normalizeProviderLabel(msg.provider)}</span>}
                        {msg.modelUsed && (
                          <span className="ml-2 text-[10px] px-1.5 py-0.5 rounded bg-theme-input-bg text-theme-text-dim font-mono">
                            {msg.modelUsed}
                          </span>
                        )}
                      </>
                    )}
                  </>
                )}
              </p>
              {msg.role === "assistant" && showRoutingDetails && msg.executionTrace?.fallback_occurred && (
                <div className="mb-1 text-[10px] text-theme-text-dim opacity-60 font-mono space-y-0.5">
                  {msg.executionTrace.steps.map((step, si) => (
                    <div key={si} className="flex items-center gap-1">
                      <span className={step.result === "success" ? "text-theme-success" : "text-theme-danger"}>
                        {step.result === "success" ? "\u2713" : "\u2717"}
                      </span>
                      <span>{normalizeProviderLabel(step.provider_label)}</span>
                      {step.model_requested && <span className="opacity-50">({step.model_requested})</span>}
                      {step.error_summary && <span className="text-theme-danger truncate max-w-[200px]" title={step.error_summary}>{step.error_summary}</span>}
                    </div>
                  ))}
                </div>
              )}
              <span className={msg.isError ? "text-theme-danger" : "text-theme-text-bright"}>
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
      {dragOver && (
        <div className="absolute inset-0 z-50 flex items-center justify-center bg-theme-overlay pointer-events-none">
          <div className="border-2 border-dashed border-theme-primary rounded-xl px-8 py-6 bg-theme-surface/90">
            <p className="text-theme-primary text-sm">Drop files to attach</p>
          </div>
        </div>
      )}
      {attachments.length > 0 && (
        <div className="px-4 py-2 border-t border-theme-border bg-theme-surface flex gap-2 flex-wrap items-center">
          {attachments.map((att, i) => (
            <div
              key={i}
              className="flex items-center gap-1.5 px-2.5 py-1 bg-theme-input-bg border border-theme-border-dim rounded-lg text-xs"
            >
              <span className="text-theme-text-bright truncate max-w-[150px]" title={att.filename}>
                {att.filename}
              </span>
              <span className="text-theme-text-dim">
                {formatFileSize(att.sizeBytes)}
              </span>
              {att.truncated && (
                <span className="text-theme-warning" title="File was truncated">!</span>
              )}
              <button
                className="text-theme-text-dim hover:text-theme-danger ml-0.5"
                onClick={() => removeAttachment(i)}
                title="Remove"
              >
                x
              </button>
            </div>
          ))}
        </div>
      )}
      <div className="p-4 border-t border-theme-border flex gap-2 items-end">
        <button
          aria-label="Attach file"
          className="border border-theme-border-dim px-2.5 py-2 rounded hover:bg-theme-surface hover:border-theme-primary text-theme-text-dim hover:text-theme-text"
          onClick={openFilePicker}
          disabled={loading}
          title="Attach files"
        >
          <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M21.44 11.05l-9.19 9.19a6 6 0 01-8.49-8.49l9.19-9.19a4 4 0 015.66 5.66l-9.2 9.19a2 2 0 01-2.83-2.83l8.49-8.48" />
          </svg>
        </button>
        <textarea
          ref={textareaRef}
          aria-label="Message input"
          className="flex-1 bg-theme-input-bg border border-theme-border-dim text-theme-text px-3 py-2 rounded resize-none overflow-y-auto focus:border-theme-primary focus:ring-1 focus:ring-theme-focus-ring"
          placeholder={attachments.length > 0 ? "Add a message (optional)" : "Message"}
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
          disabled={loading}
        >
          Send
        </button>
        {loading && (
          <button
            aria-label="Stop response"
            className="border border-theme-danger text-theme-danger px-4 py-2 rounded hover:bg-theme-danger-dim disabled:opacity-50"
            onClick={interruptCurrentResponse}
            disabled={interrupting}
          >
            {interrupting ? "Stopping..." : "Stop"}
          </button>
        )}
      </div>
    </div>
  );
}
