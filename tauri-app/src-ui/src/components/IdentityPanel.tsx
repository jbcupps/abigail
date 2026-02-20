import { invoke } from "@tauri-apps/api/core";
import { useState, useEffect, useRef } from "react";
import { useTheme } from "../contexts/ThemeContext";
import LlmSetupPanel from "./LlmSetupPanel";
import ApiKeyModal from "./ApiKeyModal";
import DataSourcesPanel from "./DataSourcesPanel";

type Tab = "identity" | "keys" | "llm" | "data" | "repair";

interface RouterStatus {
  id_provider: string;
  id_url: string | null;
  ego_configured: boolean;
  routing_mode: string;
  council_providers?: number;
}

interface IdentityPanelProps {
  initialTab?: Tab;
  /** When true, hides the internal header and tab bar (used inside SanctumDrawer) */
  embedded?: boolean;
}

export default function IdentityPanel({ initialTab, embedded }: IdentityPanelProps = {}) {
  const [tab, setTab] = useState<Tab>(initialTab || "identity");
  const [routerStatus, setRouterStatus] = useState<RouterStatus | null>(null);
  const { agentName, refreshAgentName, primaryColor, avatarUrl, refreshTheme } = useTheme();

  // API Keys tab
  const [activeApiKeyProvider, setActiveApiKeyProvider] = useState<string | null>(null);
  const [storedProviders, setStoredProviders] = useState<string[]>([]);

  // Identity tab
  const [editName, setEditName] = useState(agentName || "");
  const [editPurpose, setEditPurpose] = useState("");
  const [editPersonality, setEditPersonality] = useState("");
  const [identityMessage, setIdentityMessage] = useState("");

  // Repair tab
  const [repairKey, setRepairKey] = useState("");
  const [repairMessage, setRepairMessage] = useState("");
  const [repairError, setRepairError] = useState("");
  const mountedRef = useRef(true);

  useEffect(() => {
    if (initialTab) setTab(initialTab);
  }, [initialTab]);

  useEffect(() => {
    mountedRef.current = true;
    refreshStatus();
    return () => {
      mountedRef.current = false;
    };
  }, []);

  useEffect(() => {
    if (agentName) setEditName(agentName);
  }, [agentName]);

  const refreshStatus = async () => {
    try {
      const [status, providers] = await Promise.all([
        invoke<RouterStatus>("get_router_status"),
        invoke<string[]>("get_stored_providers"),
      ]);
      
      if (!mountedRef.current) return;
      
      setRouterStatus(status);
      setStoredProviders(providers);
    } catch (e) {
      console.warn("[IdentityPanel] refreshStatus failed:", e);
    }
  };

  const handleApiKeySaved = () => {
    setActiveApiKeyProvider(null);
    refreshStatus();
  };

  const handleRecrystallize = async () => {
    if (!editName.trim()) {
      setIdentityMessage("Name is required");
      return;
    }
    setIdentityMessage("");
    try {
      await invoke<string>("crystallize_soul", {
        name: editName.trim(),
        purpose: editPurpose.trim() || "assist, retrieve, connect, and surface information",
        personality: editPersonality.trim() || "helpful, clear, and honest",
        mentorName: "", // Keeping simple for now
        primaryColor: primaryColor,
        avatarUrl: avatarUrl,
      });
      setIdentityMessage("Soul re-crystallized. Restart to apply.");
      refreshAgentName();
      refreshTheme();
    } catch (e) {
      setIdentityMessage(`Error: ${String(e)}`);
    }
  };

  const handleRepair = async () => {
    setRepairError("");
    setRepairMessage("Attempting repair...");
    try {
      await invoke("repair_identity", {
        params: { private_key: repairKey.trim(), reset: false },
      });
      setRepairKey("");
      setRepairMessage("Identity repaired. Signatures regenerated.");
    } catch (e) {
      setRepairError(String(e));
      setRepairMessage("");
    }
  };

  const handleReset = async () => {
    if (!confirm("WARNING: This will delete your identity and reset Abigail to a fresh state. Are you sure?")) return;
    setRepairError("");
    setRepairMessage("Resetting...");
    try {
      await invoke("repair_identity", { params: { private_key: null, reset: true } });
      setRepairMessage("Identity reset. Restart Abigail to begin fresh.");
    } catch (e) {
      setRepairError(String(e));
      setRepairMessage("");
    }
  };

  const tabs: { id: Tab; label: string }[] = [
    { id: "identity", label: "Soul" },
    { id: "keys", label: "Secrets" },
    { id: "llm", label: "Mind" },
    { id: "data", label: "Archives" },
    { id: "repair", label: "Recovery" },
  ];

  const hasLocalLlm = routerStatus?.id_provider === "local_http";

  return (
    <div className={`${embedded ? "" : "min-h-screen"} bg-theme-bg text-theme-text font-mono flex flex-col`}>
      {!embedded && (
        <>
          {/* Header */}
          <div className="px-4 py-3 border-b border-theme-border">
            <h1 className="text-theme-primary-dim text-lg font-bold uppercase tracking-widest">THE SANCTUM</h1>
            <p className="text-theme-text-dim text-[10px] uppercase tracking-tighter mt-1">Sovereign Core Management</p>
          </div>

          {/* Tabs */}
          <div className="flex border-b border-theme-border" role="tablist" aria-label="Identity management">
            {tabs.map((t) => (
              <button
                key={t.id}
                role="tab"
                aria-selected={tab === t.id}
                onClick={() => setTab(t.id)}
                className={`px-4 py-2 text-[10px] uppercase tracking-widest border-b-2 transition-colors ${
                  tab === t.id
                    ? "border-theme-primary text-theme-primary"
                    : "border-transparent text-theme-text-dim hover:text-theme-text"
                }`}
              >
                {t.label}
              </button>
            ))}
          </div>
        </>
      )}

      {/* Warning if no local LLM */}
      {!hasLocalLlm && (
        <div className="px-4 py-2 border-b border-yellow-800 bg-yellow-950/20">
          <p className="text-yellow-500 text-[10px] uppercase tracking-tighter">
            No local LLM configured. Id mode chat requires a local LLM.
          </p>
        </div>
      )}

      {/* Tab Content */}
      <div className="flex-1 overflow-auto">
        {/* ── SOUL (IDENTITY) ── */}
        {tab === "identity" && (
          <div className="p-6 max-w-lg space-y-6">
            <div>
              <h2 className="text-theme-primary-dim text-lg font-bold uppercase tracking-widest mb-2">Sovereign Soul</h2>
              <p className="text-theme-text-dim text-[10px] uppercase tracking-tighter mb-6">
                Refine the essence of your entity. Changes update the cryptographic soul record.
              </p>
            </div>

            <div className="space-y-4 mb-6">
              <div className="flex items-center gap-4 p-4 border border-theme-border-dim rounded bg-theme-bg-inset">
                {avatarUrl ? (
                  <img src={avatarUrl} alt="Soul Avatar" className="w-16 h-16 rounded-full border border-theme-primary shadow-glow" />
                ) : (
                  <div 
                    className="w-16 h-16 rounded-full border border-theme-primary-dim flex items-center justify-center text-theme-primary-dim text-2xl font-bold bg-theme-primary-faint"
                    style={{ color: primaryColor || undefined, borderColor: primaryColor || undefined }}
                  >
                    {editName.charAt(0) || "A"}
                  </div>
                )}
                <div>
                  <p className="text-theme-text-bright font-bold">{agentName || "Abigail"}</p>
                  <p className="text-theme-text-dim text-[10px] uppercase tracking-tighter">Sovereign Level 1</p>
                </div>
              </div>

              <div>
                <label className="block text-theme-text-dim text-[10px] uppercase tracking-tighter mb-1">Name</label>
                <input
                  type="text"
                  className="w-full bg-theme-input-bg border border-theme-border-dim text-theme-text px-3 py-2 rounded focus:border-theme-primary focus:outline-none text-sm"
                  placeholder="Abigail"
                  value={editName}
                  onChange={(e) => setEditName(e.target.value)}
                />
              </div>
              <div>
                <label className="block text-theme-text-dim text-[10px] uppercase tracking-tighter mb-1">Purpose</label>
                <input
                  type="text"
                  className="w-full bg-theme-input-bg border border-theme-border-dim text-theme-text px-3 py-2 rounded focus:border-theme-primary focus:outline-none text-sm"
                  placeholder="assist, retrieve, connect, and surface information"
                  value={editPurpose}
                  onChange={(e) => setEditPurpose(e.target.value)}
                />
              </div>
              <div>
                <label className="block text-theme-text-dim text-[10px] uppercase tracking-tighter mb-1">Personality / Tone</label>
                <input
                  type="text"
                  className="w-full bg-theme-input-bg border border-theme-border-dim text-theme-text px-3 py-2 rounded focus:border-theme-primary focus:outline-none text-sm"
                  placeholder="helpful, clear, and honest"
                  value={editPersonality}
                  onChange={(e) => setEditPersonality(e.target.value)}
                />
              </div>
            </div>
            {identityMessage && (
              <p className={`text-xs mb-4 ${identityMessage.startsWith("Error") ? "text-red-400" : "text-theme-text-bright"}`}>
                {identityMessage}
              </p>
            )}
            <button
              onClick={handleRecrystallize}
              className="w-full border border-theme-primary text-theme-primary px-6 py-2 rounded font-bold hover:bg-theme-primary-glow text-xs uppercase tracking-widest transition-all"
            >
              Re-crystallize Soul
            </button>
          </div>
        )}

        {/* ── MIND (LLM SETUP) ── */}
        {tab === "llm" && (
          <LlmSetupPanel
            onConnected={() => refreshStatus()}
          />
        )}

        {/* ── SECRETS (API KEYS) ── */}
        {tab === "keys" && (
          <div className="p-6 space-y-6">
            <div>
              <h2 className="text-theme-primary-dim text-lg font-bold uppercase tracking-widest mb-2">Hive Secrets</h2>
              <p className="text-theme-text-dim text-[10px] uppercase tracking-tighter mb-6">
                Cryptographic keys for external cognitive providers. Stored securely via DPAPI.
              </p>
            </div>
            
            <div className="space-y-2">
              {["openai", "anthropic", "perplexity", "xai", "google", "tavily"].map((p) => (
                <div key={p} className="flex items-center justify-between px-4 py-3 border border-theme-border rounded bg-theme-bg-inset">
                  <div>
                    <span className="text-theme-text-bright font-bold uppercase text-[10px] tracking-widest">{p}</span>
                    {storedProviders.includes(p) && (
                      <span className="text-theme-primary text-[10px] ml-2 font-bold">[READY]</span>
                    )}
                  </div>
                  <button
                    className="text-[10px] border border-theme-primary px-3 py-1 rounded hover:bg-theme-primary-glow uppercase tracking-widest"
                    onClick={() => setActiveApiKeyProvider(p)}
                  >
                    {storedProviders.includes(p) ? "Update" : "Add Key"}
                  </button>
                </div>
              ))}
            </div>
            {activeApiKeyProvider && (
              <ApiKeyModal
                provider={activeApiKeyProvider}
                onSaved={handleApiKeySaved}
                onCancel={() => setActiveApiKeyProvider(null)}
              />
            )}
          </div>
        )}

        {/* ── ARCHIVES (DATA) ── */}
        {tab === "data" && <DataSourcesPanel />}

        {/* ── RECOVERY (REPAIR) ── */}
        {tab === "repair" && (
          <div className="p-6 max-w-lg space-y-8">
            <div className="space-y-4">
              <div>
                <h3 className="text-theme-text font-bold uppercase text-sm tracking-widest mb-2">Soul Recovery</h3>
                <p className="text-[10px] text-theme-text-dim uppercase tracking-tighter mb-4">
                  Restore access to this sovereign entity using your private soul key.
                </p>
              </div>
              <textarea
                value={repairKey}
                onChange={(e) => setRepairKey(e.target.value)}
                placeholder="PASTE PRIVATE SOUL KEY..."
                className="w-full bg-theme-bg-inset border border-theme-primary-faint rounded p-3 text-theme-text-bright font-mono text-xs resize-none mb-2 focus:border-theme-primary focus:outline-none"
                rows={3}
              />
              <button
                onClick={handleRepair}
                disabled={!repairKey.trim()}
                className={`w-full px-4 py-2 rounded font-bold text-xs uppercase tracking-widest transition-all ${
                  repairKey.trim()
                    ? "border border-theme-primary text-theme-text hover:bg-theme-primary-glow"
                    : "border border-theme-border-dim text-theme-text-dim cursor-not-allowed"
                }`}
              >
                Recover Identity
              </button>
            </div>

            {repairMessage && <p className="text-theme-text-bright text-[10px] uppercase font-bold text-center italic">{repairMessage}</p>}
            {repairError && <p className="text-red-400 text-[10px] uppercase font-bold text-center">{repairError}</p>}

            <div className="border-t border-theme-border pt-8 space-y-4">
              <div>
                <h3 className="text-red-500 font-bold uppercase text-sm tracking-widest mb-2">Oblivion Protocol</h3>
                <p className="text-[10px] text-theme-text-dim uppercase tracking-tighter mb-4">
                  Permanently delete this entity and all its memories from the Hive. 
                  <strong className="text-red-400/80 block mt-1">WARNING: IRREVERSIBLE ACTION.</strong>
                </p>
              </div>
              <button
                onClick={handleReset}
                className="w-full px-4 py-2 rounded font-bold text-xs border border-red-700 text-red-500 hover:bg-red-900/20 uppercase tracking-widest transition-all"
              >
                Execute Hard Reset
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
