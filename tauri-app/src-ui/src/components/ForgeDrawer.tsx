import { useState } from "react";
import IdentityPanel from "./IdentityPanel";
import TierModelPanel from "./TierModelPanel";
import AgenticPanel from "./AgenticPanel";
import OrchestrationPanel from "./OrchestrationPanel";
import DiagnosticsPanel from "./DiagnosticsPanel";

type ForgeTab =
  | "status"
  | "llm"
  | "keys"
  | "data"
  | "identity"
  | "models"
  | "agent"
  | "jobs"
  | "diagnostics"
  | "repair";

interface ForgeDrawerProps {
  open: boolean;
  onClose: () => void;
  onDisconnect: () => void;
}

const TABS: { id: ForgeTab; label: string }[] = [
  { id: "status", label: "Status" },
  { id: "llm", label: "LLM Setup" },
  { id: "keys", label: "API Keys" },
  { id: "data", label: "Data" },
  { id: "identity", label: "Identity" },
  { id: "models", label: "Models" },
  { id: "agent", label: "Agent" },
  { id: "jobs", label: "Jobs" },
  { id: "diagnostics", label: "Diagnostics" },
  { id: "repair", label: "Repair" },
];

export default function ForgeDrawer({ open, onClose, onDisconnect }: ForgeDrawerProps) {
  const [activeTab, setActiveTab] = useState<ForgeTab>("status");

  // IdentityPanel has its own internal tabs; map our drawer tabs to its tab prop
  const identityPanelTabs = ["status", "llm", "keys", "data", "identity", "repair"] as const;
  const isIdentityPanelTab = identityPanelTabs.includes(activeTab as typeof identityPanelTabs[number]);

  const handleDiagnosticsNavigate = (tab: string) => {
    if (TABS.some((t) => t.id === tab)) {
      setActiveTab(tab as ForgeTab);
    }
  };

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
            <h1 className="text-theme-primary-dim text-lg font-bold font-mono">THE FORGE</h1>
            <p className="text-theme-text-dim text-xs">Settings & Management</p>
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
          className="flex border-b border-theme-border overflow-x-auto shrink-0"
          role="tablist"
          aria-label="Forge navigation"
        >
          {TABS.map((t) => (
            <button
              key={t.id}
              role="tab"
              aria-selected={activeTab === t.id}
              onClick={() => setActiveTab(t.id)}
              className={`px-3 py-2 text-xs font-mono whitespace-nowrap border-b-2 transition-colors ${
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
          {isIdentityPanelTab && (
            <IdentityPanel
              initialTab={activeTab as "status" | "llm" | "keys" | "data" | "identity" | "repair"}
              embedded
            />
          )}
          {activeTab === "models" && <TierModelPanel />}
          {activeTab === "agent" && <AgenticPanel />}
          {activeTab === "jobs" && <OrchestrationPanel />}
          {activeTab === "diagnostics" && (
            <DiagnosticsPanel onNavigate={handleDiagnosticsNavigate} />
          )}
        </div>

        {/* Footer */}
        <div className="border-t border-theme-border px-4 py-3 shrink-0">
          <button
            className="text-theme-text-dim hover:text-theme-text text-xs font-mono"
            onClick={onDisconnect}
          >
            [disconnect]
          </button>
        </div>
      </div>
    </>
  );
}
