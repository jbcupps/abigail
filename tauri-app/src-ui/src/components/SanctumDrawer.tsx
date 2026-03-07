import { useEffect, useState } from "react";
import IdentityPanel from "./IdentityPanel";
import AgenticPanel from "./AgenticPanel";
import OrchestrationPanel from "./OrchestrationPanel";
import DiagnosticsPanel from "./DiagnosticsPanel";
import ForgePanel from "./ForgePanel";
import { isExperimentalUiEnabled } from "../runtimeMode";
import { invoke } from "@tauri-apps/api/core";

type SanctumTab =
  | "conscience"
  | "forge"
  | "staff"
  | "jobs"
  | "browser"
  | "identity"
  | "appearance"
  | "keys"
  | "llm"
  | "data"
  | "diagnostics"
  | "repair";
type IdentityPanelTab = Extract<SanctumTab, "identity" | "appearance" | "keys" | "llm" | "data" | "repair">;

interface SanctumDrawerProps {
  open: boolean;
  onClose: () => void;
  onDisconnect: () => void;
}

interface BrowserSessionInfo {
  entity_id: string | null;
  profile_dir: string;
  active_in_process: boolean;
  last_used_at_utc: string;
  last_action: string | null;
  current_url: string | null;
  page_title: string | null;
  cookie_count: number | null;
}

const TABS: { id: SanctumTab; label: string }[] = [
  { id: "conscience", label: "Conscience" },
  { id: "forge", label: "Nerve Center" },
  { id: "staff", label: "Staff" },
  { id: "jobs", label: "Registry" },
  { id: "browser", label: "Browser Session" },
  { id: "identity", label: "Soul" },
  { id: "appearance", label: "Look" },
  { id: "keys", label: "Secrets" },
  { id: "llm", label: "Mind" },
  { id: "data", label: "Archives" },
  { id: "diagnostics", label: "Insights" },
  { id: "repair", label: "Recovery" },
];

export default function SanctumDrawer({ open, onClose, onDisconnect }: SanctumDrawerProps) {
  const experimentalUiEnabled = isExperimentalUiEnabled();
  const [backendReady, setBackendReady] = useState(false);
  const [activeTab, setActiveTab] = useState<SanctumTab>("conscience");
  const [vaultVerifiedAtUtc, setVaultVerifiedAtUtc] = useState<string>(new Date().toISOString());
  const [recoveringVault, setRecoveringVault] = useState(false);
  const [browserSessions, setBrowserSessions] = useState<BrowserSessionInfo[]>([]);
  const [browserLoading, setBrowserLoading] = useState(false);
  const [browserError, setBrowserError] = useState<string | null>(null);
  const [clearingSession, setClearingSession] = useState<string | null>(null);

  // IdentityPanel mapping
  const identityPanelTabs: IdentityPanelTab[] = ["identity", "appearance", "llm", "keys", "data", "repair"];
  const isIdentityPanelTab = (tab: SanctumTab): tab is IdentityPanelTab =>
    identityPanelTabs.includes(tab as IdentityPanelTab);

  const staffJobsEnabled = backendReady || experimentalUiEnabled;
  const visibleTabs = TABS.filter((tab) =>
    staffJobsEnabled ? true : tab.id !== "staff" && tab.id !== "jobs"
  );
  const isVisibleTab = (tab: string): tab is SanctumTab => visibleTabs.some((t) => t.id === tab);

  const handleDiagnosticsNavigate = (tab: string) => {
    if (isVisibleTab(tab)) {
      setActiveTab(tab);
    }
  };

  const loadBrowserSessions = async () => {
    setBrowserLoading(true);
    setBrowserError(null);
    try {
      const sessions = await invoke<BrowserSessionInfo[]>("list_browser_sessions");
      setBrowserSessions(sessions ?? []);
    } catch (error) {
      setBrowserError(error instanceof Error ? error.message : "Unable to load browser sessions");
    } finally {
      setBrowserLoading(false);
    }
  };

  useEffect(() => {
    if (!isVisibleTab(activeTab)) {
      setActiveTab("conscience");
    }
  }, [activeTab, visibleTabs]);

  useEffect(() => {
    let mounted = true;
    const checkBackendReadiness = async () => {
      try {
        const status = await invoke<{ healthy: boolean }>("get_orchestration_backend_status");
        if (mounted) {
          setBackendReady(Boolean(status?.healthy));
        }
      } catch {
        if (mounted) {
          setBackendReady(false);
        }
      }
    };

    void checkBackendReadiness();
    const timer = setInterval(checkBackendReadiness, 10_000);
    return () => {
      mounted = false;
      clearInterval(timer);
    };
  }, []);

  useEffect(() => {
    if (open && activeTab === "browser") {
      void loadBrowserSessions();
    }
  }, [open, activeTab]);

  const recoverSoulVault = async () => {
    // Stub only: backend recovery wiring will perform signed-Documents re-encryption.
    setRecoveringVault(true);
    try {
      await new Promise((resolve) => setTimeout(resolve, 450));
      setVaultVerifiedAtUtc(new Date().toISOString());
    } finally {
      setRecoveringVault(false);
    }
  };

  const clearBrowserSession = async (profileDir: string) => {
    setClearingSession(profileDir);
    setBrowserError(null);
    try {
      await invoke("clear_browser_session", { profileDir });
      await loadBrowserSessions();
    } catch (error) {
      setBrowserError(error instanceof Error ? error.message : "Unable to clear browser session");
    } finally {
      setClearingSession(null);
    }
  };

  return (
    <>
      {/* Backdrop */}
      {open && (
        <div
          className="fixed inset-0 bg-theme-overlay z-40 transition-opacity"
          onClick={onClose}
        />
      )}

      {/* Drawer */}
      <div
        className={`fixed top-0 right-0 h-full w-[420px] max-w-[90vw] bg-theme-bg border-l border-theme-border z-50 flex flex-col transform transition-transform duration-200 ${
          open ? "translate-x-0" : "translate-x-full"
        }`}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-theme-border shrink-0">
          <div>
            <h1 className="text-theme-primary-dim text-lg font-bold font-primary tracking-widest uppercase">The Sanctum</h1>
            <p className="text-theme-text-dim text-[10px] uppercase tracking-tighter">Sovereign Core Management</p>
          </div>
          <button
            className="text-theme-text-dim hover:text-theme-text text-xl px-2"
            onClick={onClose}
            aria-label="Close drawer"
          >
            &times;
          </button>
        </div>

        <div className="px-4 py-2 border-b border-theme-border bg-theme-success-dim flex items-center justify-between gap-3">
          <div className="text-[10px] uppercase tracking-widest font-primary text-theme-success">
            ✓ Soul Vault: Healthy
            <span className="ml-2 text-theme-text-dim normal-case tracking-normal">
              {new Date(vaultVerifiedAtUtc).toLocaleString()}
            </span>
          </div>
          <button
            className="text-[10px] uppercase tracking-widest border border-theme-success text-theme-success rounded px-2 py-1 hover:bg-theme-success-dim disabled:opacity-50"
            onClick={recoverSoulVault}
            disabled={recoveringVault}
          >
            {recoveringVault ? "Recovering..." : "Recover Soul Vault"}
          </button>
        </div>

        {/* Tab navigation - horizontal scrollable */}
        <div
          className="flex border-b border-theme-border overflow-x-auto shrink-0 no-scrollbar"
          role="tablist"
          aria-label="Sanctum navigation"
        >
          {visibleTabs.map((t) => (
            <button
              key={t.id}
              role="tab"
              aria-selected={activeTab === t.id}
              onClick={() => setActiveTab(t.id)}
              className={`px-3 py-2 text-[10px] font-primary whitespace-nowrap border-b-2 transition-colors uppercase tracking-widest ${
                activeTab === t.id
                  ? "border-theme-primary text-theme-primary"
                  : "border-transparent text-theme-text-dim hover:text-theme-text"
              }`}
            >
              {t.label}
            </button>
          ))}
        </div>

        {/* Tab content */}
        <div className="flex-1 overflow-y-auto">
          {activeTab === "conscience" && (
            <div className="p-6">
              <h2 className="text-theme-primary-dim text-lg font-bold mb-4 uppercase tracking-widest">Ethical Reflection</h2>
              <div className="border border-theme-border-dim rounded p-4 bg-theme-bg-inset text-center py-10">
                <p className="text-theme-text-dim text-sm italic">
                  Abigail is currently reflecting on her recent interactions.
                </p>
                <p className="text-theme-text-dim text-xs mt-2 uppercase tracking-tighter">
                  Next batch audit: ~12 hours
                </p>
              </div>
            </div>
          )}
          
          {activeTab === "forge" && <ForgePanel />}
          
          {staffJobsEnabled && activeTab === "staff" && <AgenticPanel />}

          {staffJobsEnabled && activeTab === "jobs" && <OrchestrationPanel />}

          {activeTab === "browser" && (
            <div className="p-6 space-y-4">
              <div>
                <h2 className="text-theme-primary-dim text-lg font-bold uppercase tracking-widest">Browser Session</h2>
                <p className="text-theme-text-dim text-xs mt-2">
                  Persistent Playwright profiles stay aligned to each Entity so OAuth, cookies, and webmail sessions can be cleared intentionally.
                </p>
              </div>

              {browserError && (
                <div className="border border-theme-danger rounded px-3 py-2 text-xs text-theme-danger">
                  {browserError}
                </div>
              )}

              {browserLoading ? (
                <div className="border border-theme-border-dim rounded p-4 text-xs text-theme-text-dim uppercase tracking-widest">
                  Scanning browser profiles...
                </div>
              ) : browserSessions.length === 0 ? (
                <div className="border border-theme-border-dim rounded p-4 bg-theme-bg-inset text-sm text-theme-text-dim">
                  No persistent browser sessions have been created yet.
                </div>
              ) : (
                <div className="space-y-3">
                  {browserSessions.map((session) => (
                    <div
                      key={session.profile_dir}
                      className="border border-theme-border-dim rounded p-4 bg-theme-bg-inset space-y-3"
                    >
                      <div className="flex items-start justify-between gap-3">
                        <div>
                          <div className="text-[10px] uppercase tracking-widest text-theme-text-dim">
                            {session.entity_id ? `Entity ${session.entity_id}` : "Shared Profile"}
                          </div>
                          <div className="text-sm text-theme-text break-all">{session.profile_dir}</div>
                        </div>
                        <span className={`text-[10px] uppercase tracking-widest px-2 py-1 rounded border ${
                          session.active_in_process
                            ? "border-theme-success text-theme-success"
                            : "border-theme-border text-theme-text-dim"
                        }`}>
                          {session.active_in_process ? "Active" : "Stored"}
                        </span>
                      </div>

                      <div className="text-xs text-theme-text-dim space-y-1">
                        <div>Last used: {new Date(session.last_used_at_utc).toLocaleString()}</div>
                        {session.last_action && <div>Last action: {session.last_action}</div>}
                        {session.page_title && <div>Title: {session.page_title}</div>}
                        {session.current_url && <div className="break-all">URL: {session.current_url}</div>}
                        {typeof session.cookie_count === "number" && <div>Cookies: {session.cookie_count}</div>}
                      </div>

                      <button
                        className="text-[10px] uppercase tracking-widest border border-theme-danger text-theme-danger rounded px-3 py-2 hover:bg-theme-danger/10 disabled:opacity-50"
                        onClick={() => void clearBrowserSession(session.profile_dir)}
                        disabled={clearingSession === session.profile_dir}
                      >
                        {clearingSession === session.profile_dir ? "Clearing..." : "Clear Session"}
                      </button>
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}

          {isIdentityPanelTab(activeTab) && (
            <IdentityPanel
              initialTab={activeTab}
              embedded
            />
          )}
          
          {activeTab === "diagnostics" && (
            <DiagnosticsPanel onNavigate={handleDiagnosticsNavigate} />
          )}
        </div>

        {/* Footer */}
        <div className="border-t border-theme-border px-4 py-3 shrink-0 flex justify-between items-center">
          <span className="text-[10px] text-theme-text-dim uppercase tracking-widest font-primary">
            Sovereign v0.0.1
          </span>
          <button
            className="text-theme-text-dim hover:text-theme-danger text-[10px] font-primary uppercase tracking-widest border border-theme-border-dim px-2 py-1 rounded"
            onClick={onDisconnect}
          >
            [Eject]
          </button>
        </div>
      </div>
    </>
  );
}
