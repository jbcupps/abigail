import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";

interface MissingSkillSecret {
  skill_id: string;
  skill_name: string;
  secret_name: string;
  secret_description: string;
  required: boolean;
}

interface VaultModalProps {
  secret: MissingSkillSecret;
  onSaved: () => void;
  onCancel: () => void;
}

export type { MissingSkillSecret };

export default function VaultModal({ secret, onSaved, onCancel }: VaultModalProps) {
  const [value, setValue] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  const handleSave = async () => {
    if (!value.trim()) {
      setError("Value is required");
      return;
    }
    setSaving(true);
    setError("");
    try {
      await invoke("store_secret", { key: secret.secret_name, value: value.trim() });
      setValue("");
      onSaved();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter") {
      handleSave();
    } else if (e.key === "Escape") {
      onCancel();
    }
  };

  return (
    <div className="fixed inset-0 bg-black/80 flex items-center justify-center z-50" role="dialog" aria-modal="true" aria-label="Setup Required">
      <div className="bg-theme-bg-elevated border border-theme-primary rounded-lg p-6 max-w-md w-full mx-4">
        <h2 className="text-theme-primary-dim text-lg mb-2">Setup Required</h2>
        <p className="text-theme-text-dim text-sm mb-4">
          Abigail needs access to <span className="text-theme-text-bright">{secret.secret_name}</span> to
          enable the <span className="text-theme-text-bright">{secret.skill_name}</span> skill.
          This will be encrypted securely on your device.
        </p>
        {secret.secret_description && (
          <p className="text-theme-primary-faint text-xs mb-4">{secret.secret_description}</p>
        )}
        <div className="mb-4">
          <label className="block text-theme-text text-xs mb-1">{secret.secret_name}</label>
          <input
            type="password"
            aria-label={secret.secret_name}
            className="w-full bg-theme-input-bg border border-theme-border-dim text-theme-text px-3 py-2 rounded focus:border-theme-primary focus:ring-1 focus:ring-theme-focus-ring focus:outline-none"
            placeholder="Enter value..."
            value={value}
            onChange={(e) => setValue(e.target.value)}
            onKeyDown={handleKeyDown}
            autoFocus
          />
        </div>
        {error && <p className="text-red-400 text-sm mb-3">{error}</p>}
        <div className="flex gap-3 justify-end">
          {!secret.required && (
            <button
              className="border border-theme-primary-faint text-theme-text-dim px-4 py-2 rounded hover:bg-theme-surface text-sm"
              onClick={onCancel}
            >
              Skip
            </button>
          )}
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
            {saving ? "Encrypting..." : "Save"}
          </button>
        </div>
      </div>
    </div>
  );
}
