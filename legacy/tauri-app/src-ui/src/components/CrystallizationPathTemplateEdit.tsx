import { useState } from "react";
import type { CrystallizationIdentityDraft } from "./crystallizationPaths";

interface Props {
  onComplete: (draft: CrystallizationIdentityDraft) => void;
}

const BASE_TEMPLATE = {
  name: "Abigail",
  purpose: "Assist with trustworthy execution and clear decisions.",
  personality: "Calm, direct, and transparent under pressure.",
  primaryColor: "#00ffcc",
  avatarUrl: "",
};

export default function CrystallizationPathTemplateEdit({ onComplete }: Props) {
  const [draft, setDraft] = useState<CrystallizationIdentityDraft>(BASE_TEMPLATE);

  return (
    <div className="p-6 max-w-3xl">
      <h3 className="text-theme-primary-dim text-lg mb-2">Editable Template</h3>
      <p className="text-theme-text-dim text-sm mb-4">
        Start from a stable baseline and adjust directly before final soul crystallization.
      </p>

      <div className="space-y-3">
        <input
          className="w-full bg-theme-input-bg border border-theme-border-dim rounded px-3 py-2"
          value={draft.name}
          onChange={(e) => setDraft((p) => ({ ...p, name: e.target.value }))}
          placeholder="Agent name"
        />
        <textarea
          className="w-full bg-theme-input-bg border border-theme-border-dim rounded px-3 py-2"
          rows={2}
          value={draft.purpose}
          onChange={(e) => setDraft((p) => ({ ...p, purpose: e.target.value }))}
          placeholder="Purpose"
        />
        <textarea
          className="w-full bg-theme-input-bg border border-theme-border-dim rounded px-3 py-2"
          rows={2}
          value={draft.personality}
          onChange={(e) => setDraft((p) => ({ ...p, personality: e.target.value }))}
          placeholder="Personality"
        />
        <div className="flex gap-2">
          <input
            className="bg-theme-input-bg border border-theme-border-dim rounded px-3 py-2 w-36"
            value={draft.primaryColor}
            onChange={(e) => setDraft((p) => ({ ...p, primaryColor: e.target.value }))}
            placeholder="#00ffcc"
          />
          <input
            className="flex-1 bg-theme-input-bg border border-theme-border-dim rounded px-3 py-2"
            value={draft.avatarUrl}
            onChange={(e) => setDraft((p) => ({ ...p, avatarUrl: e.target.value }))}
            placeholder="Avatar URL (optional)"
          />
        </div>
      </div>

      <button
        className="mt-5 border border-theme-primary px-5 py-2 rounded hover:bg-theme-primary-glow"
        onClick={() => onComplete(draft)}
      >
        Continue to Soul Preview
      </button>
    </div>
  );
}
