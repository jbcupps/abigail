import { useState } from "react";
import type { CrystallizationIdentityDraft } from "./crystallizationPaths";

interface Props {
  onComplete: (draft: CrystallizationIdentityDraft) => void;
}

const FAST_TEMPLATES: CrystallizationIdentityDraft[] = [
  {
    name: "Abigail",
    purpose: "Assist, retrieve, connect, and surface information.",
    personality: "Clear, calm, accountable, and practical.",
    primaryColor: "#00c2a8",
    avatarUrl: "",
  },
  {
    name: "Sentinel",
    purpose: "Protect operational focus and surface risk early.",
    personality: "Structured, vigilant, and concise.",
    primaryColor: "#3b82f6",
    avatarUrl: "",
  },
  {
    name: "Atlas",
    purpose: "Map knowledge and coordinate multi-step execution.",
    personality: "Methodical, patient, and collaborative.",
    primaryColor: "#8b5cf6",
    avatarUrl: "",
  },
];

export default function CrystallizationPathFast({ onComplete }: Props) {
  const [selected, setSelected] = useState(0);

  return (
    <div className="p-6 max-w-3xl">
      <h3 className="text-theme-primary-dim text-lg mb-2">Fast Template</h3>
      <p className="text-theme-text-dim text-sm mb-4">
        Choose a starter profile, then continue to finalize in Soul Preview.
      </p>

      <div className="grid gap-3">
        {FAST_TEMPLATES.map((template, idx) => (
          <button
            key={`${template.name}-${idx}`}
            className={`text-left rounded border px-4 py-3 ${
              idx === selected
                ? "border-theme-primary bg-theme-primary-glow"
                : "border-theme-border-dim bg-theme-bg-elevated"
            }`}
            onClick={() => setSelected(idx)}
          >
            <div className="font-bold text-theme-text">{template.name}</div>
            <div className="text-xs text-theme-text-dim mt-1">{template.personality}</div>
          </button>
        ))}
      </div>

      <button
        className="mt-5 border border-theme-primary px-5 py-2 rounded hover:bg-theme-primary-glow"
        onClick={() => onComplete(FAST_TEMPLATES[selected])}
      >
        Use Template
      </button>
    </div>
  );
}
