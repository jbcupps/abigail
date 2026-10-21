import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import ApiKeyModal from "./ApiKeyModal";
import HelpTooltip from "./HelpTooltip";
import { API_PROVIDER_HELP, getCliProviderHelp } from "./providerHelp";
import type { CliDetection } from "../types/llm";

interface RouterStatus {
  routing_mode: string;
  ego_provider: string | null;
}

interface ProviderDrawerProps {
  onClose: () => void;
}

type Tab = "api" | "cli";

export default function ProviderDrawer({ onClose }: ProviderDrawerProps) {
  const [tab, setTab] = useState<Tab>("api");
  const [storedProviders, setStoredProviders] = useState<string[]>([]);
  const [cliDetections, setCliDetections] = useState<CliDetection[]>([]);
  const [routerStatus, setRouterStatus] = useState<RouterStatus | null>(null);
  const [activeKeyProvider, setActiveKeyProvider] = useState<string | null>(null);
  const [cliProbing, setCliProbing] = useState(false);
  const [activating, setActivating] = useState<string | null>(null);
  const [error, setError] = useState("");

  // Fetch data on mount (component is conditionally rendered)
  useEffect(() => {
    refreshAll();
  }, []);

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
    return routerStatus.ego_provider.toLowerCase() === name.toLowerCase();
  };

  const onPathCli = cliDetections.filter((d) => d.on_path);

  return (
    <>
      {/* Backdrop */}
      <div
        className="fixed inset-0 bg-theme-overlay z-40 transition-opacity"
        onClick={onClose}
        data-testid="provider-drawer-backdrop"
      />

      {/* Drawer */}
      <div
        className="fixed top-0 right-0 h-full w-[420px] max-w-[90vw] bg-theme-bg border-l border-theme-border z-50 flex flex-col"
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
              {API_PROVIDER_HELP.map((p) => {
                const hasKey = storedProviders.includes(p.id);
                const active = isActiveProvider(p.id);
                return (
                  <div
                    key={p.id}
                    className={`flex items-center justify-between px-4 py-3 border rounded bg-theme-bg-inset ${
                      active ? "border-theme-success bg-theme-success-dim" : "border-theme-border-dim"
                    }`}
                  >
                    <div className="flex items-center gap-2">
                      <span className="text-theme-text-bright font-bold uppercase text-[10px] tracking-widest">
                        {p.label}
                      </span>
                      <HelpTooltip
                        label={`${p.label} help`}
                        title={p.title}
                        description={p.description}
                        links={p.links}
                      />
                      {hasKey && (
                        <span className="text-theme-primary text-[10px] font-bold">[READY]</span>
                      )}
                      {active && (
                        <span className="text-theme-success text-[10px] font-bold">[ACTIVE]</span>
                      )}
                    </div>
                    <div className="flex items-center gap-2">
                      {hasKey && !active && (
                        <button
                          className="text-[10px] border border-theme-success text-theme-success px-2 py-1 rounded hover:bg-theme-success-dim disabled:opacity-50"
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
                <div className="border border-theme-warning bg-theme-warning-dim p-4 rounded">
                  <p className="text-theme-warning text-sm">No CLI tools detected on PATH.</p>
                  <p className="text-theme-warning text-xs mt-2">
                    Install a supported CLI tool (Claude Code, Gemini CLI, etc.), then click Re-scan.
                  </p>
                </div>
              ) : (
                <div className="space-y-2">
                  {onPathCli.map((d) => {
                    const ready = d.is_official && d.is_authenticated;
                    const active = isActiveProvider(d.provider_name);
                    const help = getCliProviderHelp(d.provider_name);
                    return (
                      <div
                        key={d.provider_name}
                        className={`px-4 py-3 border rounded ${
                          active
                            ? "border-theme-success bg-theme-success-dim"
                            : "border-theme-border-dim bg-theme-bg-inset"
                        }`}
                      >
                        <div className="flex items-center justify-between">
                          <div>
                            <div className="flex items-center gap-2">
                              <span className="text-theme-text-bright font-bold text-sm uppercase">
                                {d.provider_name.replace("-cli", "")}
                              </span>
                              {help && (
                                <HelpTooltip
                                  label={`${d.provider_name} help`}
                                  title={help.title}
                                  description={help.description}
                                  links={help.links}
                                />
                              )}
                            </div>
                            {d.version && (
                              <span className="text-theme-text-dim text-xs ml-2">{d.version}</span>
                            )}
                          </div>
                          <div className="flex items-center gap-2">
                            {d.is_official ? (
                              <span className="text-theme-success text-[10px] uppercase">Official</span>
                            ) : (
                              <span className="text-theme-warning text-[10px] uppercase">Unverified</span>
                            )}
                            {d.is_authenticated ? (
                              <span className="text-theme-success text-[10px] uppercase">Authed</span>
                            ) : (
                              <span className="text-theme-warning text-[10px] uppercase">Not Authed</span>
                            )}
                          </div>
                        </div>
                        {active && (
                          <span className="text-theme-success text-[10px] font-bold mt-1 block">[ACTIVE]</span>
                        )}
                        {ready && !active ? (
                          <button
                            className="mt-2 px-4 py-1.5 border border-theme-success text-theme-success rounded text-xs hover:bg-theme-success-dim disabled:opacity-50"
                            onClick={() => handleActivate(d.provider_name)}
                            disabled={activating !== null}
                          >
                            {activating === d.provider_name ? "Activating..." : "Activate as Primary"}
                          </button>
                        ) : !ready ? (
                          <p className="text-theme-warning text-xs mt-2">
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
            <div className="p-3 border border-theme-danger rounded bg-theme-danger-dim text-theme-danger text-sm">
              {error}
              <button
                className="ml-2 text-theme-danger underline text-xs"
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
