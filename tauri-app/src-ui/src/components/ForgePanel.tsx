import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState, useCallback } from "react";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

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

type ApplyStatus = {
  kind: "success" | "error";
  message: string;
} | null;

type ModelRegistryEntry = {
  provider: string;
  model_id: string;
  display_name: string | null;
};

type ModelRegistrySummary = {
  providers: string[];
  total_models: number;
  models: ModelRegistryEntry[];
};

type TierModelAssignments = {
  fast: Record<string, string>;
  standard: Record<string, string>;
  pro: Record<string, string>;
};

type ForceOverride = {
  pinned_model: string | null;
  pinned_tier: string | null;
  pinned_provider: string | null;
};

type TierThresholds = {
  fast_ceiling: number;
  pro_floor: number;
};

// Hardcoded fallback models (used when registry is empty)
const FALLBACK_MODELS: Record<string, string[]> = {
  openai: ["gpt-4.1", "gpt-4.1-mini", "gpt-5.2", "o3-mini"],
  anthropic: ["claude-sonnet-4-6", "claude-haiku-4-5", "claude-opus-4-6"],
  google: ["gemini-2.5-flash", "gemini-2.5-flash-lite", "gemini-2.5-pro"],
  xai: ["grok-4-1-fast-reasoning", "grok-4-1-fast-non-reasoning", "grok-4-0709"],
  perplexity: ["sonar", "sonar-pro", "sonar-reasoning-pro"],
};

const TIER_LABELS: Record<string, { label: string; desc: string; color: string }> = {
  fast: { label: "Fast", desc: "Cheapest, fastest responses", color: "text-green-400" },
  standard: { label: "Standard", desc: "Balanced quality/speed", color: "text-blue-400" },
  pro: { label: "Pro", desc: "Highest quality output", color: "text-purple-400" },
};

export default function ForgePanel() {
  // Core state
  const [storedProviders, setStoredProviders] = useState<string[]>([]);
  const [activeProvider, setActiveProvider] = useState<string | null>(null);
  const [routingMode, setRoutingMode] = useState<string>("tier_based");
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
  const [applyStatus, setApplyStatus] = useState<ApplyStatus>(null);

  // Tier routing state
  const [modelRegistry, setModelRegistry] = useState<ModelRegistrySummary | null>(null);
  const [tierModels, setTierModels] = useState<TierModelAssignments | null>(null);
  const [forceOverride, setForceOverride] = useState<ForceOverride>({
    pinned_model: null,
    pinned_tier: null,
    pinned_provider: null,
  });
  const [tierThresholds, setTierThresholds] = useState<TierThresholds>({
    fast_ceiling: 35,
    pro_floor: 70,
  });
  const [registryRefreshing, setRegistryRefreshing] = useState(false);

  // Compute models available for a provider (registry first, fallback to hardcoded)
  const getModelsForProvider = useCallback(
    (provider: string): string[] => {
      if (modelRegistry) {
        const registryModels = modelRegistry.models
          .filter((m) => m.provider === provider)
          .map((m) => m.model_id);
        if (registryModels.length > 0) return registryModels;
      }
      return FALLBACK_MODELS[provider] || [];
    },
    [modelRegistry]
  );

  const fetchData = async () => {
    try {
      setLoading(true);
      const [providers, active, router, forgeUi, sharing, undo, audit, registry, tModels, fOverride, tThresholds] =
        await Promise.all([
          invoke<string[]>("get_stored_providers"),
          invoke<string | null>("get_active_provider"),
          invoke<any>("get_router_status"),
          invoke<{ advanced_mode: boolean }>("get_forge_ui_settings"),
          invoke<{ skills_sharing_enabled: boolean }>("get_identity_sharing_settings"),
          invoke<UndoStatus>("get_forge_undo_status"),
          invoke<AuditEvent[]>("get_forge_audit_events"),
          invoke<ModelRegistrySummary>("get_model_registry").catch(() => null),
          invoke<TierModelAssignments>("get_tier_models").catch(() => null),
          invoke<ForceOverride>("get_force_override").catch(() => ({
            pinned_model: null,
            pinned_tier: null,
            pinned_provider: null,
          })),
          invoke<TierThresholds>("get_tier_thresholds").catch(() => ({
            fast_ceiling: 35,
            pro_floor: 70,
          })),
        ]);

      const coreProviders = providers.filter((p) => p !== "tavily");
      setStoredProviders(coreProviders);

      setRoutingMode(router.routing_mode);
      setAdvancedMode(forgeUi.advanced_mode);
      setSkillsSharing(sharing.skills_sharing_enabled);
      setUndoStatus(undo);
      setAuditEvents(audit.slice(-50).reverse());

      if (registry) setModelRegistry(registry);
      if (tModels) setTierModels(tModels);
      setForceOverride(fOverride);
      setTierThresholds(tThresholds);

      const effectiveActive = active || router.ego_provider || coreProviders[0];
      setActiveProvider(effectiveActive);

      if (effectiveActive) {
        const model = await invoke<string | null>("get_ego_model", {
          provider: effectiveActive,
        });
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
    setApplyStatus(null);
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
        superegoMode: undefined,
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
      const result = await invoke<{
        success: boolean;
        changes_applied: string[];
      }>("apply_forge_primary_intelligence", {
        provider: activeProvider,
        model: modelToSave,
        routingMode,
        superegoMode: undefined,
      });
      await invoke("set_identity_sharing_settings", {
        skillsSharingEnabled: skillsSharing,
      });
      setCustomModel("");
      setPreview(null);
      await fetchData();
      const changeCount = result?.changes_applied?.length ?? 0;
      setApplyStatus({
        kind: "success",
        message:
          changeCount > 0
            ? `Applied ${changeCount} change${changeCount === 1 ? "" : "s"} (active: ${activeProvider}).`
            : `No effective changes. Active provider remains ${activeProvider}.`,
      });
      window.setTimeout(() => {
        setApplyStatus((prev) => (prev?.kind === "success" ? null : prev));
      }, 2500);
    } catch (e) {
      setApplyStatus({ kind: "error", message: String(e) });
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

  // --- Tier model handlers ---

  const handleSetTierModel = async (
    provider: string,
    tier: string,
    model: string
  ) => {
    try {
      await invoke("set_tier_model", { provider, tier, model });
      const updated = await invoke<TierModelAssignments>("get_tier_models");
      setTierModels(updated);
    } catch (e) {
      console.error("Failed to set tier model:", e);
    }
  };

  const handleResetTierModels = async () => {
    try {
      await invoke("reset_tier_models");
      const updated = await invoke<TierModelAssignments>("get_tier_models");
      setTierModels(updated);
    } catch (e) {
      console.error("Failed to reset tier models:", e);
    }
  };

  // --- Force override handlers ---

  const handleSetForceOverride = async (override_: ForceOverride) => {
    try {
      await invoke("set_force_override", { forceOverride: override_ });
      setForceOverride(override_);
    } catch (e) {
      console.error("Failed to set force override:", e);
    }
  };

  const handleClearForceOverride = () => {
    handleSetForceOverride({
      pinned_model: null,
      pinned_tier: null,
      pinned_provider: null,
    });
  };

  // --- Tier threshold handlers ---

  const handleSetTierThresholds = async (thresholds: TierThresholds) => {
    try {
      await invoke("set_tier_thresholds", { tierThresholds: thresholds });
      setTierThresholds(thresholds);
    } catch (e) {
      console.error("Failed to set tier thresholds:", e);
    }
  };

  // --- Registry refresh ---

  const handleRefreshRegistry = async () => {
    setRegistryRefreshing(true);
    try {
      const updated = await invoke<ModelRegistrySummary>(
        "refresh_model_registry"
      );
      setModelRegistry(updated);
    } catch (e) {
      console.error("Failed to refresh model registry:", e);
    } finally {
      setRegistryRefreshing(false);
    }
  };

  // Determine the override mode for the radio group
  const getOverrideMode = (): string => {
    if (forceOverride.pinned_model) return "pin_model";
    if (forceOverride.pinned_tier && forceOverride.pinned_provider) return "pin_provider_tier";
    if (forceOverride.pinned_tier) return "pin_tier";
    return "auto";
  };

  // ---------------------------------------------------------------------------
  // Sub-renders
  // ---------------------------------------------------------------------------

  /** Tier model assignment grid — 3 columns (Fast/Standard/Pro) per provider. */
  const renderTierAssignment = () => {
    if (!tierModels) return null;
    const tiers: ("fast" | "standard" | "pro")[] = ["fast", "standard", "pro"];

    return (
      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <label className="text-[10px] text-theme-text-dim uppercase tracking-widest">
            Model Tier Assignment
          </label>
          <div className="flex gap-2">
            <button
              onClick={handleRefreshRegistry}
              disabled={registryRefreshing}
              className="px-2 py-1 border border-theme-border-dim rounded text-[10px] text-theme-text-dim hover:border-theme-primary disabled:opacity-50"
            >
              {registryRefreshing ? "Refreshing..." : "Refresh Models"}
            </button>
            <button
              onClick={handleResetTierModels}
              className="px-2 py-1 border border-theme-border-dim rounded text-[10px] text-theme-text-dim hover:border-theme-primary"
            >
              Reset Defaults
            </button>
          </div>
        </div>

        {/* Registry stats */}
        {modelRegistry && (
          <div className="text-[10px] text-theme-text-dim">
            Registry: {modelRegistry.total_models} model(s) across{" "}
            {modelRegistry.providers.length} provider(s)
          </div>
        )}

        {/* Column headers */}
        <div className="grid grid-cols-4 gap-2 text-[10px] uppercase tracking-wider text-theme-text-dim">
          <div>Provider</div>
          {tiers.map((t) => (
            <div key={t} className={TIER_LABELS[t].color}>
              {TIER_LABELS[t].label}
            </div>
          ))}
        </div>

        {/* Per-provider rows */}
        {storedProviders.map((provider) => {
          const available = getModelsForProvider(provider);
          return (
            <div
              key={provider}
              className="grid grid-cols-4 gap-2 items-center"
            >
              <div className="text-xs font-bold uppercase text-theme-text-bright">
                {provider}
              </div>
              {tiers.map((tier) => {
                const currentVal =
                  tierModels[tier]?.[provider] || "";
                return (
                  <select
                    key={`${provider}-${tier}`}
                    value={currentVal}
                    onChange={(e) =>
                      handleSetTierModel(provider, tier, e.target.value)
                    }
                    className="bg-theme-bg-inset border border-theme-border-dim rounded px-1 py-1 text-[10px] text-theme-primary focus:border-theme-primary outline-none truncate"
                  >
                    {currentVal && !available.includes(currentVal) && (
                      <option value={currentVal}>{currentVal}</option>
                    )}
                    {available.map((m) => (
                      <option key={m} value={m}>
                        {m}
                      </option>
                    ))}
                    {available.length === 0 && (
                      <option value="">No models</option>
                    )}
                  </select>
                );
              })}
            </div>
          );
        })}
      </div>
    );
  };

  /** Force override controls (Advanced Mode). */
  const renderForceOverride = () => {
    const mode = getOverrideMode();
    const allModels =
      modelRegistry?.models.map((m) => m.model_id) ||
      Object.values(FALLBACK_MODELS).flat();

    return (
      <div className="space-y-3">
        <label className="text-[10px] text-theme-text-dim uppercase tracking-widest">
          Force Override
        </label>

        <div className="grid grid-cols-2 gap-2">
          {[
            {
              id: "auto",
              label: "Auto",
              desc: "Complexity-based tier selection",
            },
            { id: "pin_tier", label: "Pin Tier", desc: "Force a specific tier" },
            {
              id: "pin_model",
              label: "Pin Model",
              desc: "Force an exact model",
            },
            {
              id: "pin_provider_tier",
              label: "Provider+Tier",
              desc: "Pin provider and tier",
            },
          ].map((opt) => (
            <button
              key={opt.id}
              onClick={() => {
                if (opt.id === "auto") {
                  handleClearForceOverride();
                } else if (opt.id === "pin_tier") {
                  handleSetForceOverride({
                    pinned_model: null,
                    pinned_tier: "standard",
                    pinned_provider: null,
                  });
                } else if (opt.id === "pin_model") {
                  handleSetForceOverride({
                    pinned_model: currentModel || "",
                    pinned_tier: null,
                    pinned_provider: null,
                  });
                } else if (opt.id === "pin_provider_tier") {
                  handleSetForceOverride({
                    pinned_model: null,
                    pinned_tier: "standard",
                    pinned_provider: activeProvider || storedProviders[0] || "",
                  });
                }
              }}
              className={`px-2 py-2 border rounded text-[10px] text-left transition-all ${
                mode === opt.id
                  ? "border-theme-primary bg-theme-primary-glow text-theme-text-bright"
                  : "border-theme-border-dim text-theme-text-dim hover:border-theme-primary"
              }`}
            >
              <div className="font-bold">{opt.label}</div>
              <div className="text-theme-text-bright/60">{opt.desc}</div>
            </button>
          ))}
        </div>

        {/* Conditional inputs based on mode */}
        {mode === "pin_tier" && (
          <div className="flex gap-2">
            {(["fast", "standard", "pro"] as const).map((t) => (
              <button
                key={t}
                onClick={() =>
                  handleSetForceOverride({
                    ...forceOverride,
                    pinned_tier: t,
                    pinned_model: null,
                  })
                }
                className={`flex-1 px-2 py-2 border rounded text-[10px] ${
                  forceOverride.pinned_tier === t
                    ? "border-theme-primary bg-theme-primary-glow text-theme-text-bright"
                    : "border-theme-border-dim text-theme-text-dim hover:border-theme-primary"
                }`}
              >
                <span className={TIER_LABELS[t].color}>
                  {TIER_LABELS[t].label}
                </span>
              </button>
            ))}
          </div>
        )}

        {mode === "pin_model" && (
          <div>
            <input
              type="text"
              list="model-suggestions"
              value={forceOverride.pinned_model || ""}
              onChange={(e) =>
                handleSetForceOverride({
                  ...forceOverride,
                  pinned_model: e.target.value || null,
                })
              }
              placeholder="Enter model ID (e.g. gpt-4.1)"
              className="w-full bg-theme-bg-inset border border-theme-border-dim rounded px-3 py-2 text-xs text-theme-primary focus:border-theme-primary outline-none"
            />
            <datalist id="model-suggestions">
              {allModels.map((m) => (
                <option key={m} value={m} />
              ))}
            </datalist>
          </div>
        )}

        {mode === "pin_provider_tier" && (
          <div className="flex gap-2">
            <select
              value={forceOverride.pinned_provider || ""}
              onChange={(e) =>
                handleSetForceOverride({
                  ...forceOverride,
                  pinned_provider: e.target.value || null,
                  pinned_model: null,
                })
              }
              className="flex-1 bg-theme-bg-inset border border-theme-border-dim rounded px-2 py-2 text-xs text-theme-primary focus:border-theme-primary outline-none"
            >
              {storedProviders.map((p) => (
                <option key={p} value={p}>
                  {p.toUpperCase()}
                </option>
              ))}
            </select>
            <select
              value={forceOverride.pinned_tier || "standard"}
              onChange={(e) =>
                handleSetForceOverride({
                  ...forceOverride,
                  pinned_tier: e.target.value,
                  pinned_model: null,
                })
              }
              className="flex-1 bg-theme-bg-inset border border-theme-border-dim rounded px-2 py-2 text-xs text-theme-primary focus:border-theme-primary outline-none"
            >
              <option value="fast">Fast</option>
              <option value="standard">Standard</option>
              <option value="pro">Pro</option>
            </select>
          </div>
        )}

        {mode !== "auto" && (
          <div className="text-[10px] text-theme-text-dim border border-theme-border-dim rounded p-2">
            Override active:{" "}
            {mode === "pin_tier" && (
              <span className="text-theme-primary">
                All queries routed to{" "}
                <strong>{forceOverride.pinned_tier?.toUpperCase()}</strong> tier
              </span>
            )}
            {mode === "pin_model" && (
              <span className="text-theme-primary">
                All queries use model{" "}
                <strong>{forceOverride.pinned_model}</strong>
              </span>
            )}
            {mode === "pin_provider_tier" && (
              <span className="text-theme-primary">
                All queries use{" "}
                <strong>
                  {forceOverride.pinned_provider?.toUpperCase()}
                </strong>{" "}
                at{" "}
                <strong>{forceOverride.pinned_tier?.toUpperCase()}</strong>{" "}
                tier
              </span>
            )}
          </div>
        )}
      </div>
    );
  };

  /** Tier thresholds (Advanced Mode). */
  const renderTierThresholds = () => {
    const { fast_ceiling, pro_floor } = tierThresholds;
    const standardRange = pro_floor - fast_ceiling;

    return (
      <div className="space-y-3">
        <label className="text-[10px] text-theme-text-dim uppercase tracking-widest">
          Tier Thresholds (Complexity Score 5-95)
        </label>

        {/* Visual bar */}
        <div className="relative h-6 rounded overflow-hidden border border-theme-border-dim">
          <div
            className="absolute inset-y-0 left-0 bg-green-900/40"
            style={{ width: `${((fast_ceiling - 5) / 90) * 100}%` }}
          />
          <div
            className="absolute inset-y-0 bg-blue-900/40"
            style={{
              left: `${((fast_ceiling - 5) / 90) * 100}%`,
              width: `${(standardRange / 90) * 100}%`,
            }}
          />
          <div
            className="absolute inset-y-0 right-0 bg-purple-900/40"
            style={{ width: `${((95 - pro_floor) / 90) * 100}%` }}
          />
          {/* Labels */}
          <div className="absolute inset-0 flex items-center justify-around text-[9px] font-bold">
            <span className="text-green-400">Fast &lt;{fast_ceiling}</span>
            <span className="text-blue-400">
              Std {fast_ceiling}-{pro_floor}
            </span>
            <span className="text-purple-400">Pro &ge;{pro_floor}</span>
          </div>
        </div>

        <div className="grid grid-cols-2 gap-3">
          <div>
            <label className="text-[10px] text-theme-text-dim block mb-1">
              Fast Ceiling (default: 35)
            </label>
            <input
              type="number"
              min={5}
              max={pro_floor - 1}
              value={fast_ceiling}
              onChange={(e) => {
                const v = parseInt(e.target.value) || 35;
                const clamped = Math.max(5, Math.min(v, pro_floor - 1));
                handleSetTierThresholds({
                  ...tierThresholds,
                  fast_ceiling: clamped,
                });
              }}
              className="w-full bg-theme-bg-inset border border-theme-border-dim rounded px-2 py-1 text-xs text-theme-primary focus:border-theme-primary outline-none"
            />
          </div>
          <div>
            <label className="text-[10px] text-theme-text-dim block mb-1">
              Pro Floor (default: 70)
            </label>
            <input
              type="number"
              min={fast_ceiling + 1}
              max={95}
              value={pro_floor}
              onChange={(e) => {
                const v = parseInt(e.target.value) || 70;
                const clamped = Math.max(fast_ceiling + 1, Math.min(v, 95));
                handleSetTierThresholds({
                  ...tierThresholds,
                  pro_floor: clamped,
                });
              }}
              className="w-full bg-theme-bg-inset border border-theme-border-dim rounded px-2 py-1 text-xs text-theme-primary focus:border-theme-primary outline-none"
            />
          </div>
        </div>
      </div>
    );
  };

  /** Primary intelligence section — provider, routing, model. */
  const renderPrimaryIntelligence = () => (
    <div className="space-y-6">
      {/* Active Provider */}
      <div className="space-y-3">
        <label className="text-[10px] text-theme-text-dim uppercase tracking-widest">
          Active Provider
        </label>
        <div className="grid grid-cols-2 gap-2">
          {storedProviders.map((p) => (
            <button
              key={p}
              onClick={() => handleProviderChange(p)}
              className={`px-3 py-2 border rounded text-xs text-left transition-all ${
                activeProvider === p
                  ? "border-theme-primary bg-theme-primary-glow text-theme-text-bright"
                  : "border-theme-border-dim text-theme-text-dim hover:border-theme-primary"
              }`}
            >
              <span className="font-bold uppercase">{p}</span>
              {activeProvider === p && (
                <span className="float-right text-theme-primary">
                  &#9679;
                </span>
              )}
            </button>
          ))}
        </div>
      </div>

      {/* Routing Strategy */}
      <div className="space-y-3">
        <label className="text-[10px] text-theme-text-dim uppercase tracking-widest">
          Routing Strategy
        </label>
        <div className="grid grid-cols-1 gap-2">
          {[
            {
              id: "ego_primary",
              label: "Ego Primary (Cloud)",
              desc: "Maximum power. Routes all queries to cloud.",
            },
            {
              id: "council",
              label: "Council (Collective)",
              desc: "Multi-agent consensus for complex tasks.",
            },
            {
              id: "tier_based",
              label: "Tier-Based (Hybrid)",
              desc: "Smart routing based on prompt complexity.",
            },
          ].map((m) => (
            <button
              key={m.id}
              onClick={() => setRoutingMode(m.id)}
              className={`px-3 py-2 border rounded text-xs text-left transition-all ${
                routingMode === m.id
                  ? "border-theme-primary bg-theme-primary-glow text-theme-text-bright"
                  : "border-theme-border-dim text-theme-text-dim hover:border-theme-primary"
              }`}
            >
              <div className="flex justify-between items-center mb-1">
                <span className="font-bold">{m.label}</span>
                {routingMode === m.id && (
                  <span className="text-theme-primary text-[10px]">ACTIVE</span>
                )}
              </div>
              <p className="text-[10px] text-theme-text-bright/80">{m.desc}</p>
            </button>
          ))}
        </div>
      </div>

      {/* Model selection — primary model for the active provider */}
      {activeProvider && (
        <div className="space-y-3">
          <label className="text-[10px] text-theme-text-dim uppercase tracking-widest">
            Model for {activeProvider.toUpperCase()}
          </label>
          <div className="flex flex-wrap gap-2">
            {getModelsForProvider(activeProvider).map((m) => (
              <button
                key={m}
                onClick={() => setCurrentModel(m)}
                className={`px-2 py-1 border rounded text-[10px] transition-all ${
                  currentModel === m && !customModel
                    ? "border-theme-primary bg-theme-primary-glow text-theme-text-bright"
                    : "border-theme-border-dim text-theme-text-dim hover:border-theme-primary"
                }`}
              >
                {m}
              </button>
            ))}
          </div>

          <div className="pt-2">
            <label className="text-[10px] text-theme-text-dim uppercase mb-1 block">
              Custom Model Override
            </label>
            <input
              type="text"
              placeholder={currentModel || "Enter model ID..."}
              value={customModel}
              onChange={(e) => setCustomModel(e.target.value)}
              className="w-full bg-theme-bg-inset border border-theme-border-dim rounded px-3 py-2 text-xs text-theme-primary focus:border-theme-primary outline-none"
            />
          </div>
        </div>
      )}

      {/* Tier Model Assignment (always visible when tier_based routing) */}
      {routingMode === "tier_based" && renderTierAssignment()}

      {/* Advanced Mode: Force Override + Tier Thresholds */}
      {advancedMode && routingMode === "tier_based" && (
        <>
          {renderForceOverride()}
          {renderTierThresholds()}
        </>
      )}

    </div>
  );

  // ---------------------------------------------------------------------------
  // Main render
  // ---------------------------------------------------------------------------

  if (loading)
    return <div className="p-6 animate-pulse">Forging data...</div>;

  return (
    <div className="p-6 space-y-8 font-mono">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h2 className="text-theme-primary-dim text-lg font-bold mb-2 uppercase tracking-widest border-b border-theme-border-dim pb-1">
            Forge Workspace
          </h2>
          <p className="text-theme-text-dim text-xs">
            Hybrid control and creation workspace with risk-aware operations.
          </p>
        </div>
        <div className="flex gap-2">
          <button
            onClick={async () => {
              const next = !advancedMode;
              setAdvancedMode(next);
              await invoke("set_forge_advanced_mode", { advancedMode: next });
            }}
            className="px-3 py-2 border border-theme-border-dim rounded text-xs text-theme-text-bright hover:border-theme-primary"
          >
            Complexity: {advancedMode ? "Advanced" : "Basic"}
          </button>
          <button
            onClick={() => setAuditOpen((v) => !v)}
            className="px-3 py-2 border border-theme-border-dim rounded text-xs text-theme-text-bright hover:border-theme-primary"
          >
            {auditOpen ? "Hide Audit" : "Show Audit"}
          </button>
        </div>
      </div>

      <div className="grid grid-cols-2 gap-2">
        <button
          onClick={() => setViewMode("task")}
          className={`px-3 py-2 border rounded text-xs text-theme-text-bright ${viewMode === "task" ? "border-theme-primary bg-theme-primary-glow" : "border-theme-border-dim"}`}
        >
          Task View
        </button>
        <button
          onClick={() => setViewMode("system")}
          className={`px-3 py-2 border rounded text-xs text-theme-text-bright ${viewMode === "system" ? "border-theme-primary bg-theme-primary-glow" : "border-theme-border-dim"}`}
        >
          System View
        </button>
      </div>

      {viewMode === "task" && (
        <div className="space-y-4">
          <div className="grid grid-cols-4 gap-2">
            {(["diagnose", "configure", "build", "audit"] as TaskTab[]).map(
              (t) => (
                <button
                  key={t}
                  onClick={() => setTaskTab(t)}
                  className={`px-3 py-2 border rounded text-xs capitalize text-theme-text-bright ${taskTab === t ? "border-theme-primary bg-theme-primary-glow" : "border-theme-border-dim"}`}
                >
                  {t}
                </button>
              )
            )}
          </div>
          {taskTab === "diagnose" && (
            <div className="text-xs text-theme-text-dim border border-theme-border-dim rounded p-3">
              Run diagnostics, inspect routing health, and identify drift before
              making changes.
            </div>
          )}
          {taskTab === "configure" && renderPrimaryIntelligence()}
          {taskTab === "build" && (
            <div className="space-y-4">
              <p className="text-xs text-theme-text-dim">
                Guardrailed persona controls (preset-safe ranges).
              </p>
              <div>
                <label className="text-[10px] uppercase text-theme-text-dim">
                  Directness {personaDirectness}
                </label>
                <input
                  type="range"
                  min={10}
                  max={90}
                  value={personaDirectness}
                  onChange={(e) =>
                    setPersonaDirectness(parseInt(e.target.value))
                  }
                  className="w-full"
                />
              </div>
              <div>
                <label className="text-[10px] uppercase text-theme-text-dim">
                  Autonomy {personaAutonomy}
                </label>
                <input
                  type="range"
                  min={10}
                  max={90}
                  value={personaAutonomy}
                  onChange={(e) => setPersonaAutonomy(parseInt(e.target.value))}
                  className="w-full"
                />
              </div>
              <div>
                <label className="text-[10px] uppercase text-theme-text-dim">
                  Creativity {personaCreativity}
                </label>
                <input
                  type="range"
                  min={10}
                  max={90}
                  value={personaCreativity}
                  onChange={(e) =>
                    setPersonaCreativity(parseInt(e.target.value))
                  }
                  className="w-full"
                />
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
            {(
              ["router", "skills", "memory", "identity", "ops"] as SystemTab[]
            ).map((t) => (
              <button
                key={t}
                onClick={() => setSystemTab(t)}
                className={`px-3 py-2 border rounded text-xs uppercase text-theme-text-bright ${systemTab === t ? "border-theme-primary bg-theme-primary-glow" : "border-theme-border-dim"}`}
              >
                {t}
              </button>
            ))}
          </div>

          {systemTab === "router" && renderPrimaryIntelligence()}
          {systemTab === "skills" && (
            <div className="space-y-2">
              <label className="text-[10px] uppercase text-theme-text-dim">
                Cross-Identity Sharing (Skills only)
              </label>
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
              Memory system controls are managed in dedicated memory panels.
              Forge keeps summary-level controls.
            </div>
          )}
          {systemTab === "identity" && (
            <div className="space-y-3">
              <p className="text-xs text-theme-text-dim">
                High-risk identity actions require explicit confirmation.
              </p>
              <button
                className="w-full px-3 py-2 border border-theme-border-dim rounded text-xs hover:border-theme-primary"
                onClick={async () => {
                  const ok = confirm(
                    "Archive current identity to backups?"
                  );
                  if (!ok) return;
                  await invoke("archive_identity");
                }}
              >
                Archive Current Identity (High Risk)
              </button>
              <button
                className="w-full px-3 py-2 border border-red-800 rounded text-xs text-red-400 hover:bg-red-950/20"
                onClick={async () => {
                  const ok = confirm(
                    "Factory reset current identity? This is destructive."
                  );
                  if (!ok) return;
                  const ok2 = confirm(
                    "Final confirmation: wipe all current identity data?"
                  );
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
              Office governance controls are intentionally deferred in this repo
              and handled via Orion Dock patterns.
            </div>
          )}
        </div>
      )}

      <div className="space-y-2 border-t border-theme-border-dim pt-4">
        {applyStatus && (
          <div
            className={`border rounded px-3 py-2 text-xs ${
              applyStatus.kind === "success"
                ? "border-green-700 bg-green-950/20 text-green-400"
                : "border-red-800 bg-red-950/20 text-red-400"
            }`}
          >
            {applyStatus.message}
          </div>
        )}
        <button
          onClick={handlePreview}
          disabled={!activeProvider}
          className="w-full py-2 border border-theme-border-dim text-theme-text-bright text-xs uppercase tracking-widest hover:border-theme-primary"
        >
          Preview Impact
        </button>
        {preview && (
          <div className="border border-theme-border-dim rounded p-3 text-xs">
            <div className="mb-2 text-theme-text-dim">
              Risk:{" "}
              <span
                className={
                  preview.risk_level === "high"
                    ? "text-theme-warning"
                    : "text-theme-primary"
                }
              >
                {preview.risk_level.toUpperCase()}
              </span>
            </div>
            {preview.changes.length === 0 ? (
              <div className="text-theme-text-dim">No effective changes.</div>
            ) : (
              <ul className="space-y-1">
                {preview.changes.map((c, i) => (
                  <li key={i} className="text-theme-text-dim">
                    - {c}
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}
        <button
          onClick={handleApply}
          disabled={saving || !activeProvider}
          className="w-full py-3 border border-theme-primary text-theme-text-bright font-bold uppercase tracking-widest hover:bg-theme-primary-glow disabled:opacity-50"
        >
          {saving ? "APPLYING..." : "Apply Changes"}
        </button>
        <button
          onClick={handleUndo}
          disabled={!undoStatus.available}
          className="w-full py-2 border border-theme-border-dim text-theme-text-dim text-xs uppercase tracking-widest hover:border-theme-primary disabled:opacity-50"
        >
          Undo Last Change ({undoStatus.steps} steps,{" "}
          {undoStatus.window_minutes}m window)
        </button>
      </div>
      {auditOpen && (
        <div className="border border-theme-border-dim rounded p-3 text-xs space-y-2">
          <div className="text-theme-text uppercase tracking-wider">
            Audit Trail
          </div>
          {auditEvents.length === 0 && (
            <div className="text-theme-text-dim">No events yet.</div>
          )}
          {auditEvents.map((e, idx) => (
            <div
              key={`${e.timestamp}-${idx}`}
              className="border border-theme-border-dim rounded p-2"
            >
              <div className="text-theme-text-dim">
                {e.timestamp} - {e.actor}
              </div>
              <div className="text-theme-text">
                Change: {e.what_changed}
              </div>
              <div className="text-theme-text-dim">
                Risk: {e.risk_level} | Outcome: {e.outcome}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
