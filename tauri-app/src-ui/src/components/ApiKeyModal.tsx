import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";

interface ApiKeyModalProps {
  provider: string;
  onSaved: () => void;
  onCancel: () => void;
}

const PROVIDER_INFO: Record<string, { label: string; placeholder: string; prefix: string }> = {
  openai: { label: "OpenAI", placeholder: "sk-...", prefix: "sk-" },
  anthropic: { label: "Anthropic", placeholder: "sk-ant-...", prefix: "sk-ant-" },
  xai: { label: "X.AI (Grok)", placeholder: "xai-...", prefix: "xai-" },
  google: { label: "Google (Gemini)", placeholder: "AIza...", prefix: "AIza" },
};

export default function ApiKeyModal({ provider, onSaved, onCancel }: ApiKeyModalProps) {
  const [value, setValue] = useState("");
  const [saving, setSaving] = useState(false);
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
    setError("");
    try {
      await invoke("store_provider_key", { provider, key: value.trim() });
      setValue("");
      onSaved();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter") handleSave();
    else if (e.key === "Escape") onCancel();
  };

  return (
    <div className="fixed inset-0 bg-black/80 flex items-center justify-center z-50">
      <div className="bg-black border border-green-500 rounded-lg p-6 max-w-md w-full mx-4">
        <h2 className="text-green-400 text-lg mb-2">{info.label} API Key</h2>
        <p className="text-green-600 text-sm mb-4">
          Enter your {info.label} API key. It will be encrypted securely on your device.
        </p>
        <div className="mb-4">
          <input
            type="password"
            className="w-full bg-black border border-green-500 text-green-500 px-3 py-2 rounded focus:border-green-400 focus:outline-none"
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
            className="border border-green-700 text-green-600 px-4 py-2 rounded hover:bg-green-900/50 text-sm"
            onClick={onCancel}
          >
            Cancel
          </button>
          <button
            className="border border-green-500 text-green-500 px-4 py-2 rounded hover:bg-green-500/20 text-sm"
            onClick={handleSave}
            disabled={saving}
          >
            {saving ? "Encrypting..." : "Save"}
          </button>
        </div>
      </div>
    </div>
  );
}
