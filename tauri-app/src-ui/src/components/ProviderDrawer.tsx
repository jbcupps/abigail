import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import ApiKeyModal from "./ApiKeyModal";

interface CliDetection {
  provider_name: string;
  binary: string;
  on_path: boolean;
  is_official: boolean;
  is_authenticated: boolean;
  version: string | null;
  auth_hint: string | null;
}

interface RouterStatus {
  routing_mode: string;
  ego_provider: string | null;
}

interface ProviderDrawerProps {
  open: boolean;
  onClose: () => void;
}

type Tab = "api" | "cli";

const API_PROVIDERS = [
  { id: "openai", label: "OpenAI" },
  { id: "anthropic", label: "Anthropic" },
  { id: "google", label: "Google (Gemini)" },
  { id: "xai", label: "X.AI (Grok)" },
];

export default function ProviderDrawer({ open, onClose }: ProviderDrawerProps) {
  const [tab, setTab] = useState<Tab>("api");
  const [storedProviders, setStoredProviders] = useState<string[]>([]);
  const [cliDetections, setCliDetections] = useState<CliDetection[]>([]);
  const [routerStatus, setRouterStatus] = useState<RouterStatus | null>(null);
  const [activeKeyProvider, setActiveKeyProvider] = useState<string | null>(null);
  const [cliProbing, setCliProbing] = useState(false);
  const [activating, setActivating] = useState<string | null>(null);
  const [error, setError] = useState("");

  // Refresh data when drawer opens
  useEffect(() => {
    if (open) {
      refreshAll();
    }
  }, [open]);

  const refreshAll = async () => {
    setError("");
    const [storedResult, routerResult, cliResult] = await Promise.allSettled([
      invoke<string[]>("get_stored_providers"),
      invoke<RouterStatus>("get_router_status"),
      invoke<CliDetection[]>("detect_cli_providers_full"),
    ]);

    if (storedResult.status === "fulfilled") setStoredProviders(storedResult.value);
    if (routerResult.status === "fulfilled") setRouterStatus(routerResult.value);
    if (cliResult.status === "fulfilled") setCliDetections(cliResult.value);
  };

  const handleRescanCli = async () => {
    setCliProbing(true);
    setError("");
    try {
      const results = await invoke<CliDetection[]>("detect_cli_providers_full");
      setCliDetections(results);
    } catch (e) {
      setError(String(e));
    } finally {
      setCliProbing(false);
    }
  };

  const handleActivate = async (provider: string) => {
    setActivating(provider);
    setError("");
    try {
      await invoke("use_stored_provider", { provider });
      const status = await invoke<RouterStatus>("get_router_status");
      setRouterStatus(status);
    } catch (e) {
      setError(String(e));
    } finally {
      setActivating(null);
    }
  };

  const handleApiKeySaved = async () => {
    setActiveKeyProvider(null);
    try {
      const stored = await invoke<string[]>("get_stored_providers");
      setStoredProviders(stored);
    } catch {
      // Non-critical
    }
  };

  const isActiveProvider = (name: string) => {
    if (!routerStatus?.ego_provider) return false;
    return routerStatus.ego_provider.toLowerCase().includes(name.toLowerCase());
  };

  const onPathCli = cliDetections.filter((d) => d.on_path);

  return (
    <>
      {/* Backdrop */}
      {open && (
        <div
          className="fixed inset-0 bg-black/50 z-40 transition-opacity"
          onClick={onClose}
          data-testid="provider-drawer-backdrop"
        />
      )}

      {/* Drawer */}
      <div
        className={`fixed top-0 right-0 h-full w-[420px] max-w-[90vw] bg-theme-bg border-l border-theme-border z-50 flex flex-col transform transition-transform duration-200 ${
          open ? "translate-x-0" : "translate-x-full"
        }`}
        data-testid="provider-drawer"
      >
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-theme-border shrink-0">
          <div>
            <h1 className="text-theme-primary-dim text-lg font-bold font-mono tracking-widest uppercase">
              Providers
            </h1>
            <p className="text-theme-text-dim text-[10px] uppercase tracking-tighter">
              LLM Provider Configuration
            </p>
          </div>
          <button
            className="text-theme-text-dim hover:text-theme-text text-xl px-2"
            onClick={onClose}
            aria-label="Close drawer"
          >
            &times;
          </button>
        </div>

        {/* Active provider indicator */}
        {routerStatus && (
          <div className="px-4 py-2 border-b border-theme-border-dim bg-theme-bg-elevated flex items-center gap-2 shrink-0">
            <span className="text-[10px] uppercase tracking-widest text-theme-text-dim">Active:</span>
            <span className="text-xs font-mono text-theme-primary-dim">
              {routerStatus.ego_provider || "None"}
            </span>
            <span className="text-[10px] text-theme-text-dim ml-auto">
              {routerStatus.routing_mode}
            </span>
          </div>
        )}

        {/* Tab bar */}
        <div className="flex border-b border-theme-border shrink-0" role="tablist" aria-label="Provider navigation">
          {(["api", "cli"] as const).map((t) => (
            <button
              key={t}
              role="tab"
              aria-selected={tab === t}
              onClick={() => setTab(t)}
              className={`flex-1 px-3 py-2 text-[10px] font-mono uppercase tracking-widest border-b-2 transition-colors ${
                tab === t
                  ? "border-theme-primary text-theme-primary"
                  : "border-transparent text-theme-text-dim hover:text-theme-text"
              }`}
            >
              {t === "api" ? "API Keys" : "CLI Tools"}
            </button>
          ))}
        </div>

        {/* Tab content */}
        <div className="flex-1 overflow-y-auto p-4 space-y-4">
          {/* API Keys tab */}
          {tab === "api" && (
            <div className="space-y-2">
              {API_PROVIDERS.map((p) => {
                const hasKey = storedProviders.includes(p.id);
                const active = isActiveProvider(p.id);
                return (
                  <div
                    key={p.id}
                    className={`flex items-center justify-between px-4 py-3 border rounded bg-theme-bg-inset ${
                      active ? "border-green-800 bg-green-950/10" : "border-theme-border-dim"
                    }`}
                  >
                    <div className="flex items-center gap-2">
                      <span className="text-theme-text-bright font-bold uppercase text-[10px] tracking-widest">
                        {p.label}
                      </span>
                      {hasKey && (
                        <span className="text-theme-primary text-[10px] font-bold">[READY]</span>
                      )}
                      {active && (
                        <span className="text-green-500 text-[10px] font-bold">[ACTIVE]</span>
                      )}
                    </div>
                    <div className="flex items-center gap-2">
                      {hasKey && !active && (
                        <button
                          className="text-[10px] border border-green-800 text-green-500 px-2 py-1 rounded hover:bg-green-950/40 disabled:opacity-50"
                          onClick={() => handleActivate(p.id)}
                          disabled={activating !== null}
                        >
                          {activating === p.id ? "..." : "Activate"}
                        </button>
                      )}
                      <button
                        className="text-[10px] border border-theme-primary px-3 py-1 rounded hover:bg-theme-primary-glow uppercase tracking-widest"
                        onClick={() => setActiveKeyProvider(p.id)}
                      >
                        {hasKey ? "Update" : "Add Key"}
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          )}

          {/* CLI Tools tab */}
          {tab === "cli" && (
            <div className="space-y-4">
              {cliProbing ? (
                <div className="text-theme-text-dim text-sm animate-pulse">Detecting CLI tools...</div>
              ) : onPathCli.length === 0 ? (
                <div className="border border-yellow-700 bg-yellow-900/20 p-4 rounded">
                  <p className="text-yellow-500 text-sm">No CLI tools detected on PATH.</p>
                  <p className="text-yellow-400/80 text-xs mt-2">
                    Install a supported CLI tool (Claude Code, Gemini CLI, etc.), then click Re-scan.
                  </p>
                </div>
              ) : (
                <div className="space-y-2">
                  {onPathCli.map((d) => {
                    const ready = d.is_official && d.is_authenticated;
                    const active = isActiveProvider(d.provider_name);
                    return (
                      <div
                        key={d.provider_name}
                        className={`px-4 py-3 border rounded ${
                          active
                            ? "border-green-600 bg-green-950/20"
                            : ready
                              ? "border-theme-border-dim bg-theme-bg-inset"
                              : "border-theme-border-dim bg-theme-bg-inset"
                        }`}
                      >
                        <div className="flex items-center justify-between">
                          <div>
                            <span className="text-theme-text-bright font-bold text-sm uppercase">
                              {d.provider_name.replace("-cli", "")}
                            </span>
                            {d.version && (
                              <span className="text-theme-text-dim text-xs ml-2">{d.version}</span>
                            )}
                          </div>
                          <div className="flex items-center gap-2">
                            {d.is_official ? (
                              <span className="text-green-500 text-[10px] uppercase">Official</span>
                            ) : (
                              <span className="text-yellow-500 text-[10px] uppercase">Unverified</span>
                            )}
                            {d.is_authenticated ? (
                              <span className="text-green-500 text-[10px] uppercase">Authed</span>
                            ) : (
                              <span className="text-yellow-500 text-[10px] uppercase">Not Authed</span>
                            )}
                          </div>
                        </div>
                        {active && (
                          <span className="text-green-500 text-[10px] font-bold mt-1 block">[ACTIVE]</span>
                        )}
                        {ready && !active ? (
                          <button
                            className="mt-2 px-4 py-1.5 border border-green-600 text-green-500 rounded text-xs hover:bg-green-950/40 disabled:opacity-50"
                            onClick={() => handleActivate(d.provider_name)}
                            disabled={activating !== null}
                          >
                            {activating === d.provider_name ? "Activating..." : "Activate as Primary"}
                          </button>
                        ) : !ready ? (
                          <p className="text-yellow-400/80 text-xs mt-2">
                            {d.auth_hint || "CLI tool needs authentication."}
                          </p>
                        ) : null}
                      </div>
                    );
                  })}
                </div>
              )}
              <button
                className="text-xs text-theme-text-dim hover:text-theme-primary underline"
                onClick={handleRescanCli}
                disabled={cliProbing}
              >
                Re-scan
              </button>
            </div>
          )}

          {/* Error */}
          {error && (
            <div className="p-3 border border-red-800 rounded bg-red-900/20 text-red-400 text-sm">
              {error}
              <button
                className="ml-2 text-red-300 underline text-xs"
                onClick={() => setError("")}
              >
                dismiss
              </button>
            </div>
          )}
        </div>
      </div>

      {/* API Key Modal */}
      {activeKeyProvider && (
        <ApiKeyModal
          provider={activeKeyProvider}
          onSaved={handleApiKeySaved}
          onCancel={() => setActiveKeyProvider(null)}
        />
      )}
    </>
  );
}
