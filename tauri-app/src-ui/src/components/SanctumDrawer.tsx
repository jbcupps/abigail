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
  | "identity"
  | "keys"
  | "llm"
  | "data"
  | "diagnostics"
  | "repair";

interface SanctumDrawerProps {
  open: boolean;
  onClose: () => void;
  onDisconnect: () => void;
}

const TABS: { id: SanctumTab; label: string }[] = [
  { id: "conscience", label: "Conscience" },
  { id: "forge", label: "Forge" },
  { id: "staff", label: "Staff" },
  { id: "jobs", label: "Registry" },
  { id: "identity", label: "Soul" },
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

  // IdentityPanel mapping
  const identityPanelTabs = ["identity", "llm", "keys", "data", "repair"] as const;
  const isIdentityPanelTab = identityPanelTabs.includes(activeTab as any);

  const handleDiagnosticsNavigate = (tab: string) => {
    if (TABS.some((t) => t.id === tab)) {
      setActiveTab(tab as SanctumTab);
    }
  };

  const staffJobsEnabled = backendReady || experimentalUiEnabled;
  const visibleTabs = TABS.filter((tab) =>
    staffJobsEnabled ? true : tab.id !== "staff" && tab.id !== "jobs"
  );

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

  return (
    <>
      {/* Backdrop */}
      {open && (
        <div
          className="fixed inset-0 bg-black/50 z-40 transition-opacity"
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
            <h1 className="text-theme-primary-dim text-lg font-bold font-mono tracking-widest uppercase">The Sanctum</h1>
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
              className={`px-3 py-2 text-[10px] font-mono whitespace-nowrap border-b-2 transition-colors uppercase tracking-widest ${
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

          {isIdentityPanelTab && (
            <IdentityPanel
              initialTab={activeTab === "identity" ? "identity" : activeTab as any}
              embedded
            />
          )}
          
          {activeTab === "diagnostics" && (
            <DiagnosticsPanel onNavigate={handleDiagnosticsNavigate} />
          )}
        </div>

        {/* Footer */}
        <div className="border-t border-theme-border px-4 py-3 shrink-0 flex justify-between items-center">
          <span className="text-[10px] text-theme-text-dim uppercase tracking-widest font-mono">
            Sovereign v0.0.1
          </span>
          <button
            className="text-theme-text-dim hover:text-theme-danger text-[10px] font-mono uppercase tracking-widest border border-theme-border-dim px-2 py-1 rounded"
            onClick={onDisconnect}
          >
            [Eject]
          </button>
        </div>
      </div>
    </>
  );
}
