import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

const COMMON_MODELS: Record<string, string[]> = {
  openai: ["gpt-4o", "gpt-4o-mini", "o1", "o1-mini", "o3-mini"],
  anthropic: ["claude-3-5-sonnet-latest", "claude-3-5-haiku-latest", "claude-3-opus-latest"],
  google: ["gemini-2.0-flash", "gemini-1.5-pro", "gemini-1.5-flash"],
  xai: ["grok-2-latest", "grok-beta"],
  perplexity: ["sonar", "sonar-pro", "sonar-reasoning"],
};

export default function ForgePanel() {
  const [storedProviders, setStoredProviders] = useState<string[]>([]);
  const [activeProvider, setActiveProvider] = useState<string | null>(null);
  const [currentModel, setCurrentModel] = useState<string>("");
  const [customModel, setCustomModel] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  const fetchData = async () => {
    try {
      setLoading(true);
      const [providers, active, router] = await Promise.all([
        invoke<string[]>("get_stored_providers"),
        invoke<string | null>("get_active_provider"),
        invoke<any>("get_router_status")
      ]);
      
      const coreProviders = providers.filter(p => !p.endsWith("-cli") && p !== "tavily");
      setStoredProviders(coreProviders);
      
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
    const model = await invoke<string | null>("get_ego_model", { provider });
    setCurrentModel(model || "");
  };

  const handleSave = async () => {
    if (!activeProvider) return;
    setSaving(true);
    try {
      const modelToSave = customModel || currentModel;
      await Promise.all([
        invoke("set_active_provider", { provider: activeProvider }),
        invoke("set_ego_model", { provider: activeProvider, model: modelToSave })
      ]);
      setCustomModel("");
      await fetchData();
    } catch (e) {
      alert(String(e));
    } finally {
      setSaving(false);
    }
  };

  if (loading) return <div className="p-6 animate-pulse">Forging data...</div>;

  return (
    <div className="p-6 space-y-8 font-mono">
      <div>
        <h2 className="text-theme-primary-dim text-lg font-bold mb-2 uppercase tracking-widest border-b border-theme-border-dim pb-1">Primary Intelligence</h2>
        <p className="text-theme-text-dim text-xs">Choose which cloud provider and model acts as Abigail's Ego.</p>
      </div>

      <div className="space-y-6">
        {/* Provider Selection */}
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
          {storedProviders.length === 0 && (
            <p className="text-theme-warning text-[10px] bg-theme-warning-dim p-2 rounded">No cloud providers authenticated. Go to "Secrets" first.</p>
          )}
        </div>

        {/* Model Selection */}
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

        <button
          onClick={handleSave}
          disabled={saving || !activeProvider}
          className="w-full py-3 border border-theme-primary text-theme-text font-bold uppercase tracking-widest hover:bg-theme-primary-glow disabled:opacity-50"
        >
          {saving ? "REFORGING..." : "SAVE CONFIGURATION"}
        </button>
      </div>
    </div>
  );
}
