import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

const COMMON_MODELS: Record<string, string[]> = {
  openai: ["gpt-4o", "gpt-4o-mini", "o1", "o1-mini", "o3-mini"],
  anthropic: ["claude-3-5-sonnet-latest", "claude-3-5-haiku-latest", "claude-3-opus-latest"],
  google: ["gemini-2.0-flash", "gemini-1.5-pro", "gemini-1.5-flash"],
  xai: ["grok-2-latest", "grok-beta"],
  perplexity: ["sonar", "sonar-pro", "sonar-reasoning"],
};

type ViewMode = "task" | "system";
type TaskTab = "diagnose" | "configure" | "build" | "audit";
type SystemTab = "router" | "skills" | "memory" | "identity" | "ops";

type Preview = {
  changes: string[];
  risk_level: string;
  requires_confirmation: boolean;
};

type AuditEvent = {
  timestamp: string;
  actor: string;
  what_changed: string;
  risk_level: string;
  outcome: string;
};

type UndoStatus = {
  available: boolean;
  steps: number;
  window_minutes: number;
};

export default function ForgePanel() {
  const [storedProviders, setStoredProviders] = useState<string[]>([]);
  const [activeProvider, setActiveProvider] = useState<string | null>(null);
  const [routingMode, setRoutingMode] = useState<string>("tier_based");
  const [superegoMode, setSuperegoMode] = useState<string>("off");
  const [currentModel, setCurrentModel] = useState<string>("");
  const [customModel, setCustomModel] = useState("");
  const [viewMode, setViewMode] = useState<ViewMode>("task");
  const [taskTab, setTaskTab] = useState<TaskTab>("configure");
  const [systemTab, setSystemTab] = useState<SystemTab>("router");
  const [advancedMode, setAdvancedMode] = useState(false);
  const [preview, setPreview] = useState<Preview | null>(null);
  const [auditOpen, setAuditOpen] = useState(false);
  const [auditEvents, setAuditEvents] = useState<AuditEvent[]>([]);
  const [undoStatus, setUndoStatus] = useState<UndoStatus>({
    available: false,
    steps: 0,
    window_minutes: 30,
  });
  const [skillsSharing, setSkillsSharing] = useState(false);
  const [personaDirectness, setPersonaDirectness] = useState(50);
  const [personaAutonomy, setPersonaAutonomy] = useState(50);
  const [personaCreativity, setPersonaCreativity] = useState(50);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  const fetchData = async () => {
    try {
      setLoading(true);
      const [providers, active, router, forgeUi, supModeRaw, sharing, undo, audit] = await Promise.all([
        invoke<string[]>("get_stored_providers"),
        invoke<string | null>("get_active_provider"),
        invoke<any>("get_router_status"),
        invoke<{ advanced_mode: boolean }>("get_forge_ui_settings"),
        invoke<string>("get_superego_l2_mode"),
        invoke<{ skills_sharing_enabled: boolean }>("get_identity_sharing_settings"),
        invoke<UndoStatus>("get_forge_undo_status"),
        invoke<AuditEvent[]>("get_forge_audit_events"),
      ]);
      
      const coreProviders = providers.filter(p => p !== "tavily");
      setStoredProviders(coreProviders);
      
      setRoutingMode(router.routing_mode);
      setSuperegoMode(supModeRaw.replace(/"/g, ""));
      setAdvancedMode(forgeUi.advanced_mode);
      setSkillsSharing(sharing.skills_sharing_enabled);
      setUndoStatus(undo);
      setAuditEvents(audit.slice(-50).reverse());
      
      const effectiveActive = active || router.ego_provider || coreProviders[0];
      setActiveProvider(effectiveActive);
      
      if (effectiveActive) {
        const model = await invoke<string | null>("get_ego_model", { provider: effectiveActive });
        setCurrentModel(model || "");
      }
    } catch (e) {
      console.error("Failed to fetch forge data:", e);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchData();
  }, []);

  const handleProviderChange = async (provider: string) => {
    setActiveProvider(provider);
    setPreview(null);
    const model = await invoke<string | null>("get_ego_model", { provider });
    setCurrentModel(model || "");
  };

  const handlePreview = async () => {
    if (!activeProvider) return;
    try {
      const modelToSave = customModel || currentModel;
      const next = await invoke<Preview>("preview_forge_primary_intelligence", {
        provider: activeProvider,
        model: modelToSave,
        routingMode,
        superegoMode,
      });
      setPreview(next);
    } catch (e) {
      alert(String(e));
    }
  };

  const handleApply = async () => {
    if (!activeProvider) return;
    if (!preview) return;
    if (preview.requires_confirmation) {
      const ok = confirm(
        "High-risk security/policy changes detected. Apply these changes?"
      );
      if (!ok) return;
    }
    setSaving(true);
    try {
      const modelToSave = customModel || currentModel;
      await invoke("apply_forge_primary_intelligence", {
        provider: activeProvider,
        model: modelToSave,
        routingMode,
        superegoMode,
      });
      await invoke("set_identity_sharing_settings", {
        skillsSharingEnabled: skillsSharing,
      });
      setCustomModel("");
      setPreview(null);
      await fetchData();
    } finally {
      setSaving(false);
    }
  };

  const handleUndo = async () => {
    if (!undoStatus.available) return;
    const ok = confirm(
      "Undo the most recent forge change set? This uses the 30-minute undo history."
    );
    if (!ok) return;
    await invoke("forge_undo_last_change");
    await fetchData();
  };

  const renderPrimaryIntelligence = () => (
    <div className="space-y-6">
      <div className="space-y-3">
        <label className="text-[10px] text-theme-text-dim uppercase tracking-widest">Active Provider</label>
        <div className="grid grid-cols-2 gap-2">
          {storedProviders.map(p => (
            <button
              key={p}
              onClick={() => handleProviderChange(p)}
              className={`px-3 py-2 border rounded text-xs text-left transition-all ${
                activeProvider === p
                  ? "border-theme-primary bg-theme-primary-glow text-theme-text"
                  : "border-theme-border-dim text-theme-text-dim hover:border-theme-primary"
              }`}
            >
              <span className="font-bold uppercase">{p}</span>
              {activeProvider === p && <span className="float-right text-theme-primary">●</span>}
            </button>
          ))}
        </div>
      </div>

      <div className="space-y-3">
        <label className="text-[10px] text-theme-text-dim uppercase tracking-widest">Routing Strategy</label>
        <div className="grid grid-cols-1 gap-2">
          {[
            { id: "id_primary", label: "Id Only (Local)", desc: "Maximum privacy. Stays on your machine." },
            { id: "ego_primary", label: "Ego Primary (Cloud)", desc: "Maximum power. Routes all queries to cloud." },
            { id: "council", label: "Council (Collective)", desc: "Multi-agent consensus for complex tasks." },
            { id: "tier_based", label: "Tier-Based (Hybrid)", desc: "Smart routing based on prompt complexity." },
          ].map(m => (
            <button
              key={m.id}
              onClick={() => setRoutingMode(m.id)}
              className={`px-3 py-2 border rounded text-xs text-left transition-all ${
                routingMode === m.id
                  ? "border-theme-primary bg-theme-primary-glow text-theme-text"
                  : "border-theme-border-dim text-theme-text-dim hover:border-theme-primary"
              }`}
            >
              <div className="flex justify-between items-center mb-1">
                <span className="font-bold">{m.label}</span>
                {routingMode === m.id && <span className="text-theme-primary text-[10px]">ACTIVE</span>}
              </div>
              <p className="text-[10px] opacity-70">{m.desc}</p>
            </button>
          ))}
        </div>
      </div>

      {activeProvider && (
        <div className="space-y-3">
          <label className="text-[10px] text-theme-text-dim uppercase tracking-widest">Model for {activeProvider.toUpperCase()}</label>
          <div className="flex flex-wrap gap-2">
            {(COMMON_MODELS[activeProvider] || []).map(m => (
              <button
                key={m}
                onClick={() => setCurrentModel(m)}
                className={`px-2 py-1 border rounded text-[10px] transition-all ${
                  currentModel === m && !customModel
                    ? "border-theme-primary bg-theme-primary-glow text-theme-text"
                    : "border-theme-border-dim text-theme-text-dim hover:border-theme-primary"
                }`}
              >
                {m}
              </button>
            ))}
          </div>

          <div className="pt-2">
            <label className="text-[10px] text-theme-text-dim uppercase mb-1 block">Custom Model Override</label>
            <input
              type="text"
              placeholder={currentModel || "Enter model ID..."}
              value={customModel}
              onChange={e => setCustomModel(e.target.value)}
              className="w-full bg-theme-bg-inset border border-theme-border-dim rounded px-3 py-2 text-xs text-theme-primary focus:border-theme-primary outline-none"
            />
          </div>
        </div>
      )}

      <div className="space-y-2 pt-2">
        <label className="text-[10px] text-theme-text-dim uppercase tracking-widest">Superego Mode</label>
        <select
          value={superegoMode}
          onChange={(e) => setSuperegoMode(e.target.value)}
          className="w-full bg-theme-bg-inset border border-theme-border-dim rounded px-2 py-2 text-xs"
        >
          <option value="off">Off</option>
          <option value="advisory">Advisory</option>
          <option value="enforce">Enforce</option>
        </select>
      </div>
    </div>
  );

  if (loading) return <div className="p-6 animate-pulse">Forging data...</div>;

  return (
    <div className="p-6 space-y-8 font-mono">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h2 className="text-theme-primary-dim text-lg font-bold mb-2 uppercase tracking-widest border-b border-theme-border-dim pb-1">Forge Workspace</h2>
          <p className="text-theme-text-dim text-xs">Hybrid control and creation workspace with risk-aware operations.</p>
        </div>
        <div className="flex gap-2">
          <button
            onClick={async () => {
              const next = !advancedMode;
              setAdvancedMode(next);
              await invoke("set_forge_advanced_mode", { advancedMode: next });
            }}
            className="px-3 py-2 border border-theme-border-dim rounded text-xs hover:border-theme-primary"
          >
            Complexity: {advancedMode ? "Advanced" : "Basic"}
          </button>
          <button
            onClick={() => setAuditOpen(v => !v)}
            className="px-3 py-2 border border-theme-border-dim rounded text-xs hover:border-theme-primary"
          >
            {auditOpen ? "Hide Audit" : "Show Audit"}
          </button>
        </div>
      </div>

      <div className="grid grid-cols-2 gap-2">
        <button
          onClick={() => setViewMode("task")}
          className={`px-3 py-2 border rounded text-xs ${viewMode === "task" ? "border-theme-primary bg-theme-primary-glow" : "border-theme-border-dim"}`}
        >
          Task View
        </button>
        <button
          onClick={() => setViewMode("system")}
          className={`px-3 py-2 border rounded text-xs ${viewMode === "system" ? "border-theme-primary bg-theme-primary-glow" : "border-theme-border-dim"}`}
        >
          System View
        </button>
      </div>

      {viewMode === "task" && (
        <div className="space-y-4">
          <div className="grid grid-cols-4 gap-2">
            {(["diagnose", "configure", "build", "audit"] as TaskTab[]).map(t => (
              <button
                key={t}
                onClick={() => setTaskTab(t)}
                className={`px-3 py-2 border rounded text-xs capitalize ${taskTab === t ? "border-theme-primary bg-theme-primary-glow" : "border-theme-border-dim"}`}
              >
                {t}
              </button>
            ))}
          </div>
          {taskTab === "diagnose" && (
            <div className="text-xs text-theme-text-dim border border-theme-border-dim rounded p-3">
              Run diagnostics, inspect routing health, and identify drift before making changes.
            </div>
          )}
          {taskTab === "configure" && renderPrimaryIntelligence()}
          {taskTab === "build" && (
            <div className="space-y-4">
              <p className="text-xs text-theme-text-dim">Guardrailed persona controls (preset-safe ranges).</p>
              <div>
                <label className="text-[10px] uppercase text-theme-text-dim">Directness {personaDirectness}</label>
                <input type="range" min={10} max={90} value={personaDirectness} onChange={e => setPersonaDirectness(parseInt(e.target.value))} className="w-full" />
              </div>
              <div>
                <label className="text-[10px] uppercase text-theme-text-dim">Autonomy {personaAutonomy}</label>
                <input type="range" min={10} max={90} value={personaAutonomy} onChange={e => setPersonaAutonomy(parseInt(e.target.value))} className="w-full" />
              </div>
              <div>
                <label className="text-[10px] uppercase text-theme-text-dim">Creativity {personaCreativity}</label>
                <input type="range" min={10} max={90} value={personaCreativity} onChange={e => setPersonaCreativity(parseInt(e.target.value))} className="w-full" />
              </div>
            </div>
          )}
          {taskTab === "audit" && (
            <div className="text-xs text-theme-text-dim border border-theme-border-dim rounded p-3">
              Use the audit panel toggle to inspect recent operations.
            </div>
          )}
        </div>
      )}

      {viewMode === "system" && (
        <div className="space-y-4">
          <div className="grid grid-cols-5 gap-2">
            {(["router", "skills", "memory", "identity", "ops"] as SystemTab[]).map(t => (
              <button
                key={t}
                onClick={() => setSystemTab(t)}
                className={`px-3 py-2 border rounded text-xs uppercase ${systemTab === t ? "border-theme-primary bg-theme-primary-glow" : "border-theme-border-dim"}`}
              >
                {t}
              </button>
            ))}
          </div>

          {systemTab === "router" && renderPrimaryIntelligence()}
          {systemTab === "skills" && (
            <div className="space-y-2">
              <label className="text-[10px] uppercase text-theme-text-dim">Cross-Identity Sharing (Skills only)</label>
              <label className="flex items-center gap-2 text-xs">
                <input
                  type="checkbox"
                  checked={skillsSharing}
                  onChange={(e) => setSkillsSharing(e.target.checked)}
                />
                Enable skills configuration sharing across identities
              </label>
            </div>
          )}
          {systemTab === "memory" && (
            <div className="text-xs text-theme-text-dim border border-theme-border-dim rounded p-3">
              Memory system controls are managed in dedicated memory panels. Forge keeps summary-level controls.
            </div>
          )}
          {systemTab === "identity" && (
            <div className="space-y-3">
              <p className="text-xs text-theme-text-dim">High-risk identity actions require explicit confirmation.</p>
              <button
                className="w-full px-3 py-2 border border-theme-border-dim rounded text-xs hover:border-theme-primary"
                onClick={async () => {
                  const ok = confirm("Archive current identity to backups?");
                  if (!ok) return;
                  await invoke("archive_identity");
                }}
              >
                Archive Current Identity (High Risk)
              </button>
              <button
                className="w-full px-3 py-2 border border-red-800 rounded text-xs text-red-400 hover:bg-red-950/20"
                onClick={async () => {
                  const ok = confirm("Factory reset current identity? This is destructive.");
                  if (!ok) return;
                  const ok2 = confirm("Final confirmation: wipe all current identity data?");
                  if (!ok2) return;
                  await invoke("wipe_identity");
                }}
              >
                Factory Reset Current Identity (High Risk)
              </button>
            </div>
          )}
          {systemTab === "ops" && (
            <div className="text-xs text-theme-text-dim border border-theme-border-dim rounded p-3">
              Office governance controls are intentionally deferred in this repo and handled via Orion Dock patterns.
            </div>
          )}
        </div>
      )}

      <div className="space-y-2 border-t border-theme-border-dim pt-4">
        <button
          onClick={handlePreview}
          disabled={!activeProvider}
          className="w-full py-2 border border-theme-border-dim text-theme-text text-xs uppercase tracking-widest hover:border-theme-primary"
        >
          Preview Impact
        </button>
        {preview && (
          <div className="border border-theme-border-dim rounded p-3 text-xs">
            <div className="mb-2 text-theme-text-dim">
              Risk: <span className={preview.risk_level === "high" ? "text-theme-warning" : "text-theme-primary"}>{preview.risk_level.toUpperCase()}</span>
            </div>
            {preview.changes.length === 0 ? (
              <div className="text-theme-text-dim">No effective changes.</div>
            ) : (
              <ul className="space-y-1">
                {preview.changes.map((c, i) => (
                  <li key={i} className="text-theme-text-dim">- {c}</li>
                ))}
              </ul>
            )}
          </div>
        )}
        <button
          onClick={handleApply}
          disabled={saving || !activeProvider}
          className="w-full py-3 border border-theme-primary text-theme-text font-bold uppercase tracking-widest hover:bg-theme-primary-glow disabled:opacity-50"
        >
          {saving ? "APPLYING..." : "Apply Changes"}
        </button>
        <button
          onClick={handleUndo}
          disabled={!undoStatus.available}
          className="w-full py-2 border border-theme-border-dim text-theme-text-dim text-xs uppercase tracking-widest hover:border-theme-primary disabled:opacity-50"
        >
          Undo Last Change ({undoStatus.steps} steps, {undoStatus.window_minutes}m window)
        </button>
      </div>
      {auditOpen && (
        <div className="border border-theme-border-dim rounded p-3 text-xs space-y-2">
          <div className="text-theme-text uppercase tracking-wider">Audit Trail</div>
          {auditEvents.length === 0 && <div className="text-theme-text-dim">No events yet.</div>}
          {auditEvents.map((e, idx) => (
            <div key={`${e.timestamp}-${idx}`} className="border border-theme-border-dim rounded p-2">
              <div className="text-theme-text-dim">{e.timestamp} - {e.actor}</div>
              <div className="text-theme-text">Change: {e.what_changed}</div>
              <div className="text-theme-text-dim">Risk: {e.risk_level} | Outcome: {e.outcome}</div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
