import { invoke } from "@tauri-apps/api/core";
import { useState, useEffect } from "react";
import LlmSetupPanel from "./LlmSetupPanel";
import ApiKeyModal from "./ApiKeyModal";
import DataSourcesPanel from "./DataSourcesPanel";

type Tab = "status" | "llm" | "keys" | "data" | "identity" | "repair";

interface RouterStatus {
  id_provider: string;
  id_url: string | null;
  ego_configured: boolean;
  routing_mode: string;
}

export default function IdentityPanel() {
  const [tab, setTab] = useState<Tab>("status");
  const [routerStatus, setRouterStatus] = useState<RouterStatus | null>(null);
  const [dataDir, setDataDir] = useState("");
  const [agentName, setAgentName] = useState<string | null>(null);

  // API Keys tab
  const [activeApiKeyProvider, setActiveApiKeyProvider] = useState<string | null>(null);
  const [storedProviders, setStoredProviders] = useState<string[]>([]);

  // Identity tab
  const [editName, setEditName] = useState("");
  const [editPurpose, setEditPurpose] = useState("");
  const [editPersonality, setEditPersonality] = useState("");
  const [identityMessage, setIdentityMessage] = useState("");

  // Repair tab
  const [repairKey, setRepairKey] = useState("");
  const [repairMessage, setRepairMessage] = useState("");
  const [repairError, setRepairError] = useState("");

  useEffect(() => {
    refreshStatus();
  }, []);

  const refreshStatus = async () => {
    try {
      const [status, name] = await Promise.all([
        invoke<RouterStatus>("get_router_status"),
        invoke<string | null>("get_agent_name"),
      ]);
      setRouterStatus(status);
      setAgentName(name);
      if (name) setEditName(name);

      // Get data dir from docs path (parent)
      const docs = await invoke<string>("get_docs_path");
      const parts = docs.replace(/\\/g, "/").split("/");
      parts.pop();
      setDataDir(parts.join("/"));

      // Check stored providers
      const providers: string[] = [];
      for (const p of ["openai", "anthropic", "perplexity", "xai", "google", "tavily"]) {
        try {
          const exists = await invoke<boolean>("check_secret", { key: p });
          if (exists) providers.push(p);
        } catch { /* ignore */ }
      }
      setStoredProviders(providers);
    } catch { /* ignore */ }
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
      });
      setIdentityMessage("Soul re-crystallized. Restart to apply.");
      refreshStatus();
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
    { id: "status", label: "Status" },
    { id: "llm", label: "LLM Setup" },
    { id: "keys", label: "API Keys" },
    { id: "data", label: "Data" },
    { id: "identity", label: "Identity" },
    { id: "repair", label: "Repair" },
  ];

  const hasLocalLlm = routerStatus?.id_provider === "local_http";

  return (
    <div className="min-h-screen bg-black text-theme-text font-mono flex flex-col">
      {/* Header */}
      <div className="px-4 py-3 border-b border-theme-border">
        <h1 className="text-theme-primary-dim text-lg font-bold">THE FORGE</h1>
        <p className="text-theme-text-dim text-xs mt-1">Core Identity Management</p>
      </div>

      {/* Tabs */}
      <div className="flex border-b border-theme-border">
        {tabs.map((t) => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            className={`px-4 py-2 text-sm border-b-2 transition-colors ${
              tab === t.id
                ? "border-theme-primary text-theme-primary"
                : "border-transparent text-theme-text-dim hover:text-theme-text"
            }`}
          >
            {t.label}
          </button>
        ))}
      </div>

      {/* Warning if no local LLM */}
      {!hasLocalLlm && (
        <div className="px-4 py-2 border-b border-yellow-800 bg-yellow-950/20">
          <p className="text-yellow-500 text-xs">
            No local LLM configured. Id mode chat requires a local LLM.
          </p>
        </div>
      )}

      {/* Tab Content */}
      <div className="flex-1 overflow-auto">
        {/* ── STATUS ── */}
        {tab === "status" && (
          <div className="p-6 space-y-4">
            <div>
              <span className="text-theme-text-dim text-sm">Agent Name: </span>
              <span className="text-theme-text-bright">{agentName || "Abigail (default)"}</span>
            </div>
            <div>
              <span className="text-theme-text-dim text-sm">Router: </span>
              <span className="text-theme-text-bright">
                {routerStatus ? `${routerStatus.routing_mode} | Id: ${routerStatus.id_provider}` : "Loading..."}
              </span>
            </div>
            <div>
              <span className="text-theme-text-dim text-sm">Local LLM: </span>
              <span className={hasLocalLlm ? "text-theme-text-bright" : "text-red-400"}>
                {routerStatus?.id_url || "Not configured"}
              </span>
            </div>
            <div>
              <span className="text-theme-text-dim text-sm">Ego (Cloud): </span>
              <span className={routerStatus?.ego_configured ? "text-theme-text-bright" : "text-yellow-500"}>
                {routerStatus?.ego_configured ? "Configured" : "Not configured"}
              </span>
            </div>
            <div>
              <span className="text-theme-text-dim text-sm">Data Dir: </span>
              <span className="text-theme-text-bright text-xs break-all">{dataDir}</span>
            </div>
            <div>
              <span className="text-theme-text-dim text-sm">Stored Keys: </span>
              <span className="text-theme-text-bright">
                {storedProviders.length > 0 ? storedProviders.join(", ") : "None"}
              </span>
            </div>
          </div>
        )}

        {/* ── LLM SETUP ── */}
        {tab === "llm" && (
          <LlmSetupPanel
            onConnected={() => refreshStatus()}
          />
        )}

        {/* ── API KEYS ── */}
        {tab === "keys" && (
          <div className="p-6">
            <h2 className="text-theme-primary-dim text-lg mb-4">API Keys</h2>
            <p className="text-theme-text-dim text-sm mb-6">
              Manage provider API keys. Keys are encrypted with DPAPI on your device.
            </p>
            <div className="space-y-2">
              {["openai", "anthropic", "perplexity", "xai", "google", "tavily"].map((p) => (
                <div key={p} className="flex items-center justify-between px-4 py-3 border border-theme-border rounded">
                  <div>
                    <span className="text-theme-text-bright font-bold capitalize">{p}</span>
                    {storedProviders.includes(p) && (
                      <span className="text-theme-text-dim text-xs ml-2">[saved]</span>
                    )}
                  </div>
                  <button
                    className="text-xs border border-theme-primary px-3 py-1 rounded hover:bg-theme-primary-glow"
                    onClick={() => setActiveApiKeyProvider(p)}
                  >
                    {storedProviders.includes(p) ? "Update" : "Add"}
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

        {/* ── DATA ── */}
        {tab === "data" && <DataSourcesPanel />}

        {/* ── IDENTITY ── */}
        {tab === "identity" && (
          <div className="p-6 max-w-lg">
            <h2 className="text-theme-primary-dim text-lg mb-4">Identity</h2>
            <p className="text-theme-text-dim text-sm mb-6">
              Edit the agent's core identity. Changes will re-write soul.md.
            </p>
            <div className="space-y-4 mb-6">
              <div>
                <label className="block text-theme-text text-sm mb-1">Name</label>
                <input
                  type="text"
                  className="w-full bg-black border border-theme-primary text-theme-primary-dim px-3 py-2 rounded"
                  placeholder="Abigail"
                  value={editName}
                  onChange={(e) => setEditName(e.target.value)}
                />
              </div>
              <div>
                <label className="block text-theme-text text-sm mb-1">Purpose</label>
                <input
                  type="text"
                  className="w-full bg-black border border-theme-primary text-theme-primary-dim px-3 py-2 rounded"
                  placeholder="assist, retrieve, connect, and surface information"
                  value={editPurpose}
                  onChange={(e) => setEditPurpose(e.target.value)}
                />
              </div>
              <div>
                <label className="block text-theme-text text-sm mb-1">Personality / Tone</label>
                <input
                  type="text"
                  className="w-full bg-black border border-theme-primary text-theme-primary-dim px-3 py-2 rounded"
                  placeholder="helpful, clear, and honest"
                  value={editPersonality}
                  onChange={(e) => setEditPersonality(e.target.value)}
                />
              </div>
            </div>
            {identityMessage && (
              <p className={`text-sm mb-4 ${identityMessage.startsWith("Error") ? "text-red-400" : "text-theme-text-bright"}`}>
                {identityMessage}
              </p>
            )}
            <button
              onClick={handleRecrystallize}
              className="border border-theme-primary px-6 py-3 rounded font-bold hover:bg-theme-primary-glow"
            >
              Re-crystallize Soul
            </button>
          </div>
        )}

        {/* ── REPAIR ── */}
        {tab === "repair" && (
          <div className="p-6 max-w-lg">
            <div className="mb-8">
              <h3 className="text-theme-text font-bold mb-2">Recover Identity</h3>
              <p className="text-sm text-gray-400 mb-2">
                If you have your <strong>Private Key</strong> (saved from first run),
                enter it below to re-sign the documents.
              </p>
              <textarea
                value={repairKey}
                onChange={(e) => setRepairKey(e.target.value)}
                placeholder="Paste your private key here..."
                className="w-full bg-gray-900 border border-theme-primary-faint rounded p-3 text-theme-text-bright font-mono text-sm resize-none mb-2"
                rows={3}
              />
              <button
                onClick={handleRepair}
                disabled={!repairKey.trim()}
                className={`px-4 py-2 rounded font-bold text-sm ${
                  repairKey.trim()
                    ? "bg-theme-surface border border-theme-primary text-theme-text hover:bg-theme-primary-glow"
                    : "bg-gray-900 border border-gray-700 text-gray-600 cursor-not-allowed"
                }`}
              >
                Recover Identity
              </button>
            </div>

            {repairMessage && <p className="text-theme-text-bright text-sm mb-4">{repairMessage}</p>}
            {repairError && <p className="text-red-400 text-sm mb-4">{repairError}</p>}

            <div className="border-t border-gray-800 pt-6">
              <h3 className="text-red-400 font-bold mb-2">Hard Reset</h3>
              <p className="text-sm text-gray-400 mb-4">
                If you lost your key, you must reset Abigail.{" "}
                <strong>This destroys the current trust relationship.</strong>{" "}
                You will be treated as a new mentor.
              </p>
              <button
                onClick={handleReset}
                className="px-4 py-2 rounded font-bold text-sm border border-red-700 text-red-500 hover:bg-red-900/20"
              >
                Reset Identity (Destructive)
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
