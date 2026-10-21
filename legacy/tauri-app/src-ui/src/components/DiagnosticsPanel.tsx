import { useState, useEffect, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";

interface DiagnosticsPanelProps {
  onNavigate?: (tab: string) => void;
}

interface CliServerStatus {
  running: boolean;
  port?: number;
  token?: string;
}

interface LogEntry {
  index: number;
  timestamp: string;
  level: string;
  target: string;
  message: string;
}

interface CapturedLogs {
  entries: LogEntry[];
  next_index: number;
}

const LEVEL_COLORS: Record<string, string> = {
  ERROR: "text-theme-danger",
  WARN: "text-theme-warning",
  INFO: "text-theme-success",
  DEBUG: "text-theme-info",
  TRACE: "text-theme-text-dim",
};

export default function DiagnosticsPanel({ onNavigate }: DiagnosticsPanelProps) {
  // --- System diagnostics ---
  const [report, setReport] = useState<string | null>(null);
  const [diagLoading, setDiagLoading] = useState(false);
  const [diagError, setDiagError] = useState<string | null>(null);

  // --- Troubleshooting mode (CLI server) ---
  const [cliStatus, setCliStatus] = useState<CliServerStatus>({ running: false });
  const [cliPort, setCliPort] = useState("8080");
  const [cliLoading, setCliLoading] = useState(false);
  const [cliError, setCliError] = useState<string | null>(null);

  // --- Debug logging ---
  const [logLevel, setLogLevel] = useState("info");
  const [debugEnabled, setDebugEnabled] = useState(false);
  const [customFilter, setCustomFilter] = useState("");
  const [logEntries, setLogEntries] = useState<LogEntry[]>([]);
  const [nextIndex, setNextIndex] = useState(0);
  const [polling, setPolling] = useState(false);
  const [autoScroll, setAutoScroll] = useState(true);

  const logEndRef = useRef<HTMLDivElement>(null);
  const logContainerRef = useRef<HTMLDivElement>(null);
  const mountedRef = useRef(true);
  const pollingRef = useRef(false);

  useEffect(() => {
    mountedRef.current = true;
    return () => { mountedRef.current = false; };
  }, []);

  // --- Fetch initial states ---
  useEffect(() => {
    invoke<CliServerStatus>("get_cli_server_status").then((s) => {
      if (mountedRef.current) {
        setCliStatus(s);
        if (s.running && s.port) setCliPort(String(s.port));
      }
    }).catch(() => {});

    invoke<string>("get_log_level").then((lvl) => {
      if (mountedRef.current) {
        setLogLevel(lvl);
        setDebugEnabled(lvl.includes("debug") || lvl.includes("trace"));
      }
    }).catch(() => {});
  }, []);

  // --- CLI server toggle ---
  const toggleCliServer = async () => {
    setCliLoading(true);
    setCliError(null);
    try {
      if (cliStatus.running) {
        await invoke("stop_cli_server");
        setCliStatus({ running: false });
      } else {
        const port = parseInt(cliPort) || 8080;
        const status = await invoke<CliServerStatus>("start_cli_server", { port });
        setCliStatus(status);
      }
    } catch (e) {
      setCliError(String(e));
    } finally {
      setCliLoading(false);
    }
  };

  // --- Debug toggle ---
  const toggleDebug = async () => {
    const newLevel = debugEnabled ? "info" : "debug";
    try {
      await invoke("set_log_level", { level: newLevel });
      setDebugEnabled(!debugEnabled);
      setLogLevel(newLevel);
      setCustomFilter("");
    } catch (e) {
      setCliError(String(e));
    }
  };

  const applyCustomFilter = async () => {
    if (!customFilter.trim()) return;
    try {
      await invoke("set_log_level", { level: customFilter.trim() });
      setLogLevel(customFilter.trim());
      setDebugEnabled(customFilter.includes("debug") || customFilter.includes("trace"));
    } catch (e) {
      setCliError(String(e));
    }
  };

  // --- Log polling ---
  const pollLogs = useCallback(async () => {
    if (!pollingRef.current || !mountedRef.current) return;
    try {
      const result = await invoke<CapturedLogs>("get_captured_logs", { sinceIndex: nextIndex });
      if (!mountedRef.current) return;
      if (result.entries.length > 0) {
        setLogEntries(prev => {
          const combined = [...prev, ...result.entries];
          return combined.length > 2000 ? combined.slice(-2000) : combined;
        });
        setNextIndex(result.next_index);
      }
    } catch {
      // ignore polling errors
    }
  }, [nextIndex]);

  useEffect(() => {
    pollingRef.current = polling;
    if (!polling) return;

    const interval = setInterval(() => {
      if (pollingRef.current && mountedRef.current) {
        pollLogs();
      }
    }, 2000);

    pollLogs();
    return () => clearInterval(interval);
  }, [polling, pollLogs]);

  // --- Auto-scroll ---
  useEffect(() => {
    if (autoScroll && logEndRef.current) {
      logEndRef.current.scrollIntoView({ behavior: "smooth" });
    }
  }, [logEntries, autoScroll]);

  const handleLogScroll = () => {
    const el = logContainerRef.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
    setAutoScroll(atBottom);
  };

  // --- Log actions ---
  const clearLogs = async () => {
    try {
      await invoke("clear_captured_logs");
      setLogEntries([]);
      setNextIndex(0);
    } catch {
      // ignore
    }
  };

  const exportLogs = async () => {
    try {
      const filePath = await save({
        defaultPath: `abigail-logs-${new Date().toISOString().slice(0, 19).replace(/:/g, "-")}.txt`,
        filters: [{ name: "Text", extensions: ["txt", "log"] }],
      });
      if (filePath) {
        await invoke("save_logs_to_file", { path: filePath });
      }
    } catch {
      // ignore export errors
    }
  };

  // --- System diagnostics ---
  const runDiagnostics = async () => {
    setDiagLoading(true);
    setDiagError(null);
    try {
      const result = await invoke<string>("get_system_diagnostics");
      setReport(result);
    } catch (e) {
      setDiagError(String(e));
    } finally {
      setDiagLoading(false);
    }
  };

  const copyToClipboard = (text: string) => {
    navigator.clipboard.writeText(text).catch(() => {});
  };

  // --- Runtime mode ---
  const [runtimeMode, setRuntimeMode] = useState<string>("in_process");
  const [modeLoading, setModeLoading] = useState(false);

  useEffect(() => {
    invoke<string>("get_runtime_mode").then((m) => {
      if (mountedRef.current) {
        try {
          setRuntimeMode(JSON.parse(m));
        } catch {
          setRuntimeMode(m);
        }
      }
    }).catch(() => {});
  }, []);

  const toggleRuntimeMode = async () => {
    setModeLoading(true);
    const next = runtimeMode === "in_process" ? "daemon" : "in_process";
    try {
      await invoke("set_runtime_mode", { mode: next });
      setRuntimeMode(next);
    } catch (err) {
      console.error("Failed to set runtime mode:", err);
    } finally {
      setModeLoading(false);
    }
  };

  return (
    <div className="p-6 space-y-6">
      {/* ── RUNTIME MODE ── */}
      <section>
        <h2 className="text-xs font-semibold text-theme-text tracking-wider uppercase mb-2">
          Runtime Mode
        </h2>
        <div className="flex items-center gap-3">
          <button
            className={`px-3 py-1.5 text-xs rounded border font-mono transition-colors ${
              runtimeMode === "in_process"
                ? "border-theme-primary text-theme-primary bg-theme-primary/10"
                : "border-theme-border-dim text-theme-text-dim hover:border-theme-text-dim"
            }`}
            onClick={runtimeMode !== "in_process" ? toggleRuntimeMode : undefined}
            disabled={modeLoading}
          >
            In-Process
          </button>
          <button
            className={`px-3 py-1.5 text-xs rounded border font-mono transition-colors ${
              runtimeMode === "daemon"
                ? "border-theme-primary text-theme-primary bg-theme-primary/10"
                : "border-theme-border-dim text-theme-text-dim hover:border-theme-text-dim"
            }`}
            onClick={runtimeMode !== "daemon" ? toggleRuntimeMode : undefined}
            disabled={modeLoading}
          >
            Daemon
          </button>
          {modeLoading && (
            <span className="text-[10px] text-theme-text-dim animate-pulse">switching...</span>
          )}
        </div>
        <p className="text-[10px] text-theme-text-dim mt-1">
          {runtimeMode === "daemon"
            ? "Chat, skills, and memory delegate to hive-daemon + entity-daemon over HTTP."
            : "All subsystems run inside the desktop app process."}
        </p>
      </section>

      {/* ── TROUBLESHOOTING MODE ── */}
      <section>
        <div className="flex items-center justify-between mb-3">
          <div>
            <h2 className="text-theme-primary-dim text-sm font-bold uppercase tracking-widest">
              Troubleshooting Mode
            </h2>
            <p className="text-theme-text-dim text-[10px] mt-0.5">
              Opens a local REST API for IDE agents, CLI, and curl access.
            </p>
          </div>
          <button
            onClick={toggleCliServer}
            disabled={cliLoading}
            className={`relative w-12 h-6 rounded-full transition-colors ${
              cliStatus.running
                ? "bg-theme-success"
                : "bg-theme-border-dim"
            }`}
            aria-label="Toggle troubleshooting mode"
          >
            <span
              className={`absolute top-0.5 w-5 h-5 rounded-full bg-theme-bg-elevated transition-transform ${
                cliStatus.running ? "translate-x-6" : "translate-x-0.5"
              }`}
            />
          </button>
        </div>

        {!cliStatus.running && (
          <div className="flex items-center gap-2 mb-2">
            <span className="text-theme-text-dim text-[10px] uppercase tracking-wider">Port:</span>
            <input
              type="text"
              className="w-20 bg-theme-input-bg border border-theme-border-dim text-theme-text px-2 py-1 rounded text-xs focus:border-theme-primary outline-none"
              value={cliPort}
              onChange={(e) => setCliPort(e.target.value)}
            />
          </div>
        )}

        {cliError && (
          <p className="text-theme-danger text-xs mb-2">{cliError}</p>
        )}

        {cliStatus.running && (
          <div className="bg-theme-overlay border border-theme-border-dim rounded p-3 space-y-2">
            <div className="flex items-center gap-2">
              <span className="w-2 h-2 rounded-full bg-theme-success animate-pulse" />
              <span className="text-theme-success text-xs font-bold">
                API Active — http://localhost:{cliStatus.port}
              </span>
            </div>

            <div className="flex items-center gap-2">
              <span className="text-theme-text-dim text-[10px] uppercase">Token:</span>
              <code className="text-theme-text-bright text-[10px] select-all flex-1 break-all">
                {cliStatus.token}
              </code>
              <button
                className="text-[9px] border border-theme-border-dim px-2 py-0.5 rounded hover:bg-theme-bg-inset text-theme-text-dim"
                onClick={() => copyToClipboard(cliStatus.token ?? "")}
              >
                Copy
              </button>
            </div>

            <div className="border-t border-theme-border-dim pt-2 space-y-1.5">
              <p className="text-theme-text-dim text-[9px] uppercase tracking-wider">Endpoints:</p>
              <div className="space-y-1">
                <code className="block text-[9px] text-theme-text-bright bg-theme-overlay px-2 py-1 rounded break-all">
                  curl -H "Authorization: Bearer {cliStatus.token}" http://localhost:{cliStatus.port}/status
                </code>
                <code className="block text-[9px] text-theme-text-bright bg-theme-overlay px-2 py-1 rounded break-all">
                  curl -H "Authorization: Bearer {cliStatus.token}" -X POST -H "Content-Type: application/json" -d '{"{"}\"message\":\"Hello\"{"}"}'  http://localhost:{cliStatus.port}/chat
                </code>
              </div>
              <p className="text-theme-primary-faint text-[9px] mt-1">
                Use this endpoint from your IDE or agentic coding tool to interact with the entity.
              </p>
            </div>
          </div>
        )}
      </section>

      {/* ── DEBUG LOGGING ── */}
      <section className="border-t border-theme-border pt-5">
        <div className="flex items-center justify-between mb-3">
          <div>
            <h2 className="text-theme-primary-dim text-sm font-bold uppercase tracking-widest">
              Debug Logging
            </h2>
            <p className="text-theme-text-dim text-[10px] mt-0.5">
              Live log capture with adjustable verbosity. Current: <span className="text-theme-text-bright">{logLevel}</span>
            </p>
          </div>
          <button
            onClick={toggleDebug}
            className={`relative w-12 h-6 rounded-full transition-colors ${
              debugEnabled ? "bg-theme-info" : "bg-theme-border-dim"
            }`}
            aria-label="Toggle debug logging"
          >
            <span
              className={`absolute top-0.5 w-5 h-5 rounded-full bg-theme-bg-elevated transition-transform ${
                debugEnabled ? "translate-x-6" : "translate-x-0.5"
              }`}
            />
          </button>
        </div>

        {/* Advanced filter */}
        <div className="flex items-center gap-2 mb-3">
          <input
            type="text"
            className="flex-1 bg-theme-input-bg border border-theme-border-dim text-theme-text px-2 py-1 rounded text-[10px] focus:border-theme-primary outline-none font-mono"
            placeholder="e.g. abigail_router=trace,abigail_skills=debug"
            value={customFilter}
            onChange={(e) => setCustomFilter(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter") applyCustomFilter(); }}
          />
          <button
            className="text-[10px] border border-theme-primary px-2 py-1 rounded hover:bg-theme-primary-glow text-theme-primary disabled:opacity-50"
            onClick={applyCustomFilter}
            disabled={!customFilter.trim()}
          >
            Apply
          </button>
        </div>

        {/* Controls */}
        <div className="flex items-center gap-2 mb-2">
          <button
            onClick={() => setPolling(!polling)}
            className={`text-[10px] border px-3 py-1 rounded uppercase tracking-wider ${
              polling
                ? "border-theme-success text-theme-success hover:bg-theme-success-dim"
                : "border-theme-primary text-theme-primary hover:bg-theme-primary-glow"
            }`}
          >
            {polling ? "Pause" : "Stream"}
          </button>
          <button
            onClick={clearLogs}
            className="text-[10px] border border-theme-border-dim text-theme-text-dim px-3 py-1 rounded hover:bg-theme-bg-inset uppercase tracking-wider"
          >
            Clear
          </button>
          <button
            onClick={exportLogs}
            className="text-[10px] border border-theme-border-dim text-theme-text-dim px-3 py-1 rounded hover:bg-theme-bg-inset uppercase tracking-wider"
          >
            Export
          </button>
          <span className="flex-1" />
          <span className="text-[9px] text-theme-text-dim">
            {logEntries.length} entries
          </span>
        </div>

        {/* Log viewer */}
        <div
          ref={logContainerRef}
          onScroll={handleLogScroll}
          className="bg-theme-overlay border border-theme-border-dim rounded font-mono text-[10px] leading-relaxed overflow-auto"
          style={{ height: 280 }}
        >
          {logEntries.length === 0 ? (
            <div className="flex items-center justify-center h-full text-theme-text-dim text-xs">
              {polling ? "Waiting for log events..." : "Press Stream to begin capturing logs."}
            </div>
          ) : (
            <div className="p-2">
              {logEntries.map((e) => (
                <div key={e.index} className="flex gap-1.5 hover:bg-white/5">
                  <span className="text-theme-text-dim shrink-0">{e.timestamp.slice(11, 23)}</span>
                  <span className={`shrink-0 w-12 text-right ${LEVEL_COLORS[e.level] ?? "text-theme-text"}`}>
                    {e.level}
                  </span>
                  <span className="text-theme-primary-faint shrink-0 max-w-[140px] truncate">
                    {e.target}
                  </span>
                  <span className="text-theme-text-bright break-all">{e.message}</span>
                </div>
              ))}
              <div ref={logEndRef} />
            </div>
          )}
        </div>
      </section>

      {/* ── SYSTEM DIAGNOSTICS ── */}
      <section className="border-t border-theme-border pt-5">
        <div className="flex items-center justify-between mb-3">
          <div>
            <h2 className="text-theme-primary-dim text-sm font-bold uppercase tracking-widest">
              System Diagnostics
            </h2>
            <p className="text-theme-text-dim text-[10px] mt-0.5">
              Run a comprehensive check of router, skills, and integrations.
            </p>
          </div>
          <button
            className="text-[10px] border border-theme-primary px-3 py-1.5 rounded hover:bg-theme-primary-glow text-theme-primary disabled:opacity-50 uppercase tracking-wider"
            onClick={runDiagnostics}
            disabled={diagLoading}
          >
            {diagLoading ? "Running..." : "Run"}
          </button>
        </div>

        {diagError && (
          <div className="bg-theme-danger-dim border border-theme-danger rounded p-2 text-theme-danger text-xs mb-2">
            {diagError}
          </div>
        )}

        {report && (
          <div className="bg-theme-bg-inset border border-theme-border-dim rounded p-3 overflow-auto max-h-[30vh]">
            <pre className="text-[10px] text-theme-text-bright whitespace-pre-wrap font-mono">{report}</pre>
          </div>
        )}
      </section>

      {/* ── Navigation shortcuts ── */}
      {onNavigate && (
        <div className="flex gap-2 pt-1">
          <button
            className="text-[10px] border border-theme-border-dim text-theme-text-dim hover:text-theme-text px-3 py-1.5 rounded uppercase tracking-wider"
            onClick={() => onNavigate("keys")}
          >
            API Keys
          </button>
          <button
            className="text-[10px] border border-theme-border-dim text-theme-text-dim hover:text-theme-text px-3 py-1.5 rounded uppercase tracking-wider"
            onClick={() => onNavigate("llm")}
          >
            LLM Setup
          </button>
        </div>
      )}
    </div>
  );
}
