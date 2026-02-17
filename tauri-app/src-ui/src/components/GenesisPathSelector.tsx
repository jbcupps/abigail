import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

interface GenesisPathInfo {
  id: string;
  name: string;
  description: string;
  estimated_time: string;
}

interface Props {
  onSelect: (pathId: string) => void;
}

export default function GenesisPathSelector({ onSelect }: Props) {
  const [paths, setPaths] = useState<GenesisPathInfo[]>([]);
  const [selected, setSelected] = useState<string | null>(null);

  useEffect(() => {
    invoke<GenesisPathInfo[]>("get_genesis_paths")
      .then(setPaths)
      .catch(console.error);
  }, []);

  const handleConfirm = () => {
    if (selected) onSelect(selected);
  };

  return (
    <div className="flex flex-col items-center gap-6 p-8 max-w-2xl mx-auto">
      <h2 className="text-2xl font-bold text-white">Choose Your Path</h2>
      <p className="text-gray-400 text-center">
        How would you like to calibrate your agent's soul?
      </p>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-4 w-full">
        {paths.map((path) => (
          <button
            key={path.id}
            className={`text-left p-4 rounded-lg border-2 transition-all ${
              selected === path.id
                ? "border-blue-500 bg-blue-900/20"
                : "border-gray-700 bg-gray-800 hover:border-gray-600"
            }`}
            onClick={() => setSelected(path.id)}
          >
            <h3 className="text-white font-semibold mb-1">{path.name}</h3>
            <p className="text-gray-400 text-sm mb-2">{path.description}</p>
            <span className="text-xs text-gray-500">{path.estimated_time}</span>
          </button>
        ))}
      </div>

      <button
        className="bg-blue-600 hover:bg-blue-700 text-white px-8 py-3 rounded-lg disabled:opacity-50 transition-opacity"
        onClick={handleConfirm}
        disabled={!selected}
      >
        Begin
      </button>
    </div>
  );
}
