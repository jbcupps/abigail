import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";

interface StoreKeyResult {
  success: boolean;
  provider: string;
  validated: boolean;
  error: string | null;
}

interface ApiKeyModalProps {
  provider: string;
  onSaved: (result: StoreKeyResult) => void;
  onCancel: () => void;
}

const PROVIDER_INFO: Record<string, { label: string; placeholder: string; prefix: string }> = {
  openai: { label: "OpenAI", placeholder: "sk-...", prefix: "sk-" },
  anthropic: { label: "Anthropic", placeholder: "sk-ant-...", prefix: "sk-ant-" },
  perplexity: { label: "Perplexity", placeholder: "pplx-...", prefix: "pplx-" },
  xai: { label: "X.AI (Grok)", placeholder: "xai-...", prefix: "xai-" },
  google: { label: "Google (Gemini)", placeholder: "AIza...", prefix: "AIza" },
  tavily: { label: "Tavily (Web Search)", placeholder: "tvly-...", prefix: "tvly-" },
};

export default function ApiKeyModal({ provider, onSaved, onCancel }: ApiKeyModalProps) {
  const [value, setValue] = useState("");
  const [saving, setSaving] = useState(false);
  const [validating, setValidating] = useState(false);
  const [error, setError] = useState("");

  const info = PROVIDER_INFO[provider] || {
    label: provider,
    placeholder: "Enter API key...",
    prefix: "",
  };

  const handleSave = async () => {
    if (!value.trim()) {
      setError("API key is required");
      return;
    }
    setSaving(true);
    setValidating(true);
    setError("");
    try {
      const result = await invoke<StoreKeyResult>("store_provider_key", {
        provider,
        key: value.trim(),
        validate: true,
      });

      if (result.success) {
        setValue("");
        onSaved(result);
      } else {
        setError(result.error || "Validation failed");
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
      setValidating(false);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter") handleSave();
    else if (e.key === "Escape") onCancel();
  };

  return (
    <div className="fixed inset-0 bg-black/80 flex items-center justify-center z-50">
      <div className="bg-black border border-theme-primary rounded-lg p-6 max-w-md w-full mx-4">
        <h2 className="text-theme-primary-dim text-lg mb-2">{info.label} API Key</h2>
        <p className="text-theme-text-dim text-sm mb-4">
          Enter your {info.label} API key. It will be encrypted securely on your device.
        </p>
        <div className="mb-4">
          <input
            type="password"
            className="w-full bg-black border border-theme-primary text-theme-text px-3 py-2 rounded focus:border-theme-primary-dim focus:outline-none"
            placeholder={info.placeholder}
            value={value}
            onChange={(e) => setValue(e.target.value)}
            onKeyDown={handleKeyDown}
            autoFocus
          />
        </div>
        {error && <p className="text-red-400 text-sm mb-3">{error}</p>}
        <div className="flex gap-3 justify-end">
          <button
            className="border border-theme-primary-faint text-theme-text-dim px-4 py-2 rounded hover:bg-theme-surface text-sm"
            onClick={onCancel}
          >
            Cancel
          </button>
          <button
            className="border border-theme-primary text-theme-text px-4 py-2 rounded hover:bg-theme-primary-glow text-sm"
            onClick={handleSave}
            disabled={saving}
          >
            {validating ? "Validating..." : saving ? "Saving..." : "Save & Validate"}
          </button>
        </div>
      </div>
    </div>
  );
}
