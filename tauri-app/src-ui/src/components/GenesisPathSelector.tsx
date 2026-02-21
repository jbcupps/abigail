import { useState } from "react";
import { CRYSTALLIZATION_PATHS } from "./crystallizationPaths";

interface Props {
  onSelect: (pathId: string) => void;
}

export default function GenesisPathSelector({ onSelect }: Props) {
  const [selected, setSelected] = useState<string | null>(null);

  const handleConfirm = () => {
    if (selected) onSelect(selected);
  };

  return (
    <div className="flex flex-col items-center gap-6 p-8 max-w-2xl mx-auto">
      <h2 className="text-2xl font-bold text-theme-text-bright">Choose Your Path</h2>
      <p className="text-theme-text-dim text-center">
        How would you like to calibrate your agent's soul?
      </p>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-4 w-full">
        {CRYSTALLIZATION_PATHS.map((path) => (
          <button
            key={path.id}
            className={`text-left p-4 rounded-lg border-2 transition-all ${
              selected === path.id
                ? "border-theme-primary bg-theme-primary-glow"
                : "border-theme-border-dim bg-theme-bg-elevated hover:border-theme-border"
            }`}
            onClick={() => setSelected(path.id)}
          >
            <h3 className="text-theme-text-bright font-semibold mb-1">{path.name}</h3>
            <p className="text-theme-text-dim text-sm mb-2">{path.description}</p>
            <span className="text-xs text-theme-text-dim">{path.estimatedTime}</span>
          </button>
        ))}
      </div>

      <button
        className="border border-theme-primary text-theme-primary hover:bg-theme-primary-glow transition-colors px-8 py-3 rounded-lg disabled:opacity-50"
        onClick={handleConfirm}
        disabled={!selected}
      >
        Begin
      </button>
    </div>
  );
}
