import { useMemo, useState } from "react";
import type { CrystallizationIdentityDraft } from "./crystallizationPaths";

interface Props {
  onComplete: (draft: CrystallizationIdentityDraft) => void;
}

type Archetype = {
  id: string;
  label: string;
  trait: string;
  color: string;
  assetHint: string;
};

const ROUNDS: Archetype[][] = [
  [
    { id: "aegis", label: "Aegis", trait: "protective", color: "#2563eb", assetHint: "bundled://archetypes/aegis.png" },
    { id: "lumen", label: "Lumen", trait: "clarifying", color: "#eab308", assetHint: "bundled://archetypes/lumen.png" },
    { id: "stride", label: "Stride", trait: "decisive", color: "#22c55e", assetHint: "bundled://archetypes/stride.png" },
  ],
  [
    { id: "anchor", label: "Anchor", trait: "steady", color: "#0ea5e9", assetHint: "bundled://archetypes/anchor.png" },
    { id: "spark", label: "Spark", trait: "inventive", color: "#f97316", assetHint: "bundled://archetypes/spark.png" },
  ],
  [
    { id: "sage", label: "Sage", trait: "reflective", color: "#8b5cf6", assetHint: "bundled://archetypes/sage.png" },
    { id: "forge", label: "Forge", trait: "builder", color: "#ef4444", assetHint: "bundled://archetypes/forge.png" },
    { id: "bridge", label: "Bridge", trait: "collaborative", color: "#06b6d4", assetHint: "bundled://archetypes/bridge.png" },
  ],
];

export default function CrystallizationPathImage({ onComplete }: Props) {
  const [round, setRound] = useState(0);
  const [name, setName] = useState("");
  const [picks, setPicks] = useState<Archetype[]>([]);
  const current = ROUNDS[round];
  const maxRounds = Math.min(ROUNDS.length, 10);

  const finish = (extraPick?: Archetype) => {
    const selected = extraPick ? [...picks, extraPick] : picks;
    const traits = Array.from(new Set(selected.map((p) => p.trait))).join(", ");
    const color = selected[selected.length - 1]?.color || "#00ffcc";
    onComplete({
      name: name.trim() || "Abigail",
      purpose: "Serve with calibrated judgment and clear mentorship alignment.",
      personality: `Visual archetype blend: ${traits || "balanced, clear, and calm"}.`,
      primaryColor: color,
      avatarUrl: selected[selected.length - 1]?.assetHint || "",
    });
  };

  const progress = useMemo(() => `${Math.min(round + 1, maxRounds)} / ${maxRounds}`, [round, maxRounds]);

  return (
    <div className="p-6 max-w-3xl">
      <h3 className="text-theme-primary-dim text-lg mb-2">Image Archetypes</h3>
      <p className="text-theme-text-dim text-sm mb-2">
        Bundled archetype cards are used first. Pick the visual resonance that best fits this entity.
      </p>
      <p className="text-theme-text-dim text-xs mb-4">Round {progress}</p>

      <input
        className="w-full bg-theme-input-bg border border-theme-border-dim rounded px-3 py-2 mb-4"
        placeholder="Agent name"
        value={name}
        onChange={(e) => setName(e.target.value)}
      />

      <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
        {current.map((item) => (
          <button
            key={item.id}
            className="rounded border border-theme-border-dim p-4 text-left hover:border-theme-primary"
            onClick={() => {
              const nextPicks = [...picks, item];
              setPicks(nextPicks);
              if (round + 1 >= maxRounds) {
                finish(item);
              } else {
                setRound((r) => r + 1);
              }
            }}
          >
            <div className="h-14 rounded mb-3" style={{ backgroundColor: item.color }} />
            <div className="font-semibold text-theme-text">{item.label}</div>
            <div className="text-xs text-theme-text-dim mt-1">Trait: {item.trait}</div>
          </button>
        ))}
      </div>

      <button
        className="mt-5 border border-theme-primary px-5 py-2 rounded hover:bg-theme-primary-glow"
        onClick={() => finish()}
      >
        Continue with Current Picks
      </button>
    </div>
  );
}
