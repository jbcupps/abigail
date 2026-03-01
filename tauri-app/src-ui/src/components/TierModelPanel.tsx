import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

interface TierModels {
  fast: Record<string, string>;
  standard: Record<string, string>;
  pro: Record<string, string>;
}

interface CatalogEntry {
  provider: string;
  model_id: string;
  display_name: string;
  lifecycle: string | null;
  last_fetched: string | null;
}

interface ValidationIssue {
  tier: string;
  result: string;
}

const PROVIDERS = ["openai", "anthropic", "google", "xai", "perplexity"];
const TIER_NAMES = ["fast", "standard", "pro"] as const;

export default function TierModelPanel() {
  const [tierModels, setTierModels] = useState<TierModels | null>(null);
  const [originalTierModels, setOriginalTierModels] = useState<TierModels | null>(null);
  const [catalog, setCatalog] = useState<CatalogEntry[]>([]);
  const [issues] = useState<Record<string, ValidationIssue[]>>({});
  const [refreshing, setRefreshing] = useState(false);
  const [saving, setSaving] = useState(false);
  const [activeProvider, setActiveProvider] = useState<string>("");

  const load = async () => {
    try {
      const [models, provider] = await Promise.all([
        invoke<TierModels>("get_tier_models"),
        invoke<string | null>("get_active_provider").catch(() => null),
      ]);
      setTierModels(models);
      setOriginalTierModels(models);
      if (provider) setActiveProvider(provider);
    } catch (e) {
      console.error("Failed to load tier models:", e);
    }
  };

  useEffect(() => {
    load();
  }, []);

  const handleRefreshCatalog = async () => {
    setRefreshing(true);
    try {
      setCatalog([]);
    } catch {
      // no-op
    } finally {
      setRefreshing(false);
    }
  };

  const handleModelChange = (tier: typeof TIER_NAMES[number], provider: string, modelId: string) => {
    if (!tierModels) return;
    setTierModels((prev) => {
      if (!prev) return prev;
      return {
        ...prev,
        [tier]: { ...prev[tier], [provider]: modelId },
      };
    });
  };

  const handleSave = async () => {
    if (!tierModels) return;
    setSaving(true);
    try {
      for (const tier of TIER_NAMES) {
        for (const provider of PROVIDERS) {
          const model = tierModels[tier][provider];
          const original = originalTierModels?.[tier][provider] ?? "";
          if (!model || model === original) continue;
          await invoke("set_tier_model", { tier, provider, model });
        }
      }
      setOriginalTierModels(tierModels);
    } catch (e) {
      console.error("Failed to save tier models:", e);
    } finally {
      setSaving(false);
    }
  };

  const handleReset = async () => {
    try {
      await invoke("reset_tier_models");
      await load();
    } catch (e) {
      console.error("Failed to reset:", e);
    }
  };

  const handleSetActiveProvider = async (provider: string) => {
    try {
      await invoke("set_active_provider", { provider });
      setActiveProvider(provider);
    } catch (e) {
      console.error("Failed to set active provider:", e);
    }
  };

  const getModelsForProvider = (provider: string) => {
    return catalog.filter((e) => e.provider === provider);
  };

  if (!tierModels) {
    return (
      <div className="flex items-center justify-center p-8">
        <p className="text-theme-text-dim animate-pulse">Loading tier models...</p>
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full p-4 gap-4 overflow-y-auto">
      <div className="flex items-center justify-between">
        <h2 className="text-lg font-semibold text-theme-text-bright">Model Tiers</h2>
        <div className="flex gap-2">
          <button
            className="text-xs text-theme-info hover:text-theme-primary-dim px-2 py-1 border border-theme-border-dim rounded"
            onClick={handleRefreshCatalog}
            disabled={refreshing}
          >
            {refreshing ? "Refreshing..." : "Refresh Catalog"}
          </button>
          <button
            className="text-xs text-theme-text-dim hover:text-theme-text px-2 py-1 border border-theme-border-dim rounded"
            onClick={handleReset}
          >
            Reset Defaults
          </button>
        </div>
      </div>

      {/* Active Provider Selector */}
      <div className="bg-theme-bg-elevated rounded-lg p-3">
        <label className="text-sm text-theme-text-dim block mb-2">Preferred Ego Provider</label>
        <div className="flex gap-2 flex-wrap">
          {PROVIDERS.map((p) => (
            <button
              key={p}
              className={`text-xs px-3 py-1.5 rounded capitalize ${
                activeProvider === p
                  ? "border border-theme-primary bg-theme-primary-glow text-theme-primary"
                  : "bg-theme-surface text-theme-text-dim hover:bg-theme-surface-bright"
              }`}
              onClick={() => handleSetActiveProvider(p)}
            >
              {p}
            </button>
          ))}
        </div>
      </div>

      {/* Tier Model Grid */}
      {TIER_NAMES.map((tier) => (
        <div key={tier} className="bg-theme-bg-elevated rounded-lg p-3">
          <h3 className="text-sm font-semibold text-theme-text-bright mb-2 capitalize">
            {tier} Tier
            <span className="text-theme-text-dim font-normal ml-2 text-xs">
              {tier === "fast" && "(Quick responses, lower cost)"}
              {tier === "standard" && "(Balanced quality and speed)"}
              {tier === "pro" && "(Maximum quality, council routing)"}
            </span>
          </h3>
          <div className="space-y-2">
            {PROVIDERS.map((provider) => {
              const currentModel = tierModels[tier][provider] || "";
              const providerModels = getModelsForProvider(provider);
              const hasIssue = issues[provider]?.some((i) => i.tier === tier);

              return (
                <div key={provider} className="flex items-center gap-2">
                  <span className="text-xs text-theme-text-dim w-20 capitalize">{provider}</span>
                  {providerModels.length > 0 ? (
                    <select
                      className={`flex-1 bg-theme-input-bg text-theme-text text-xs rounded px-2 py-1.5 ${
                        hasIssue ? "border border-yellow-600" : ""
                      }`}
                      value={currentModel}
                      onChange={(e) => handleModelChange(tier, provider, e.target.value)}
                    >
                      <option value="">-- none --</option>
                      {providerModels.map((m) => (
                        <option key={m.model_id} value={m.model_id}>
                          {m.display_name}
                          {m.lifecycle === "deprecated" ? " (deprecated)" : ""}
                        </option>
                      ))}
                    </select>
                  ) : (
                    <input
                      className={`flex-1 bg-theme-input-bg text-theme-text text-xs rounded px-2 py-1.5 ${
                        hasIssue ? "border border-yellow-600" : ""
                      }`}
                      value={currentModel}
                      onChange={(e) => handleModelChange(tier, provider, e.target.value)}
                      placeholder="model-id"
                    />
                  )}
                  {hasIssue && (
                    <span className="text-yellow-400 text-xs" title="Validation issue">!</span>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      ))}

      <button
        className="border border-theme-primary text-theme-primary hover:bg-theme-primary-glow transition-colors px-4 py-2 rounded-lg self-end disabled:opacity-50"
        onClick={handleSave}
        disabled={saving}
      >
        {saving ? "Saving..." : "Save Changes"}
      </button>
    </div>
  );
}
