import { useTheme } from "../contexts/ThemeContext";

export default function PersonaToggle() {
  const { mode, setMode, agentName } = useTheme();

  const isEgo = mode === "ego";
  const label = isEgo ? (agentName || "AO") : "THE FORGE";
  const tooltip = isEgo ? "Access Core Identity (Id)" : "Return to Surface (Ego)";

  return (
    <button
      onClick={() => setMode(isEgo ? "id" : "ego")}
      title={tooltip}
      className="fixed top-3 right-3 z-50 flex items-center gap-2 px-3 py-1.5 rounded-full border bg-black/80 backdrop-blur-sm transition-colors hover:bg-black/90"
      style={{ borderColor: isEgo ? "#22c55e" : "#f59e0b" }}
    >
      <span
        className={`w-2.5 h-2.5 rounded-full ${isEgo ? "bg-red-500 animate-pulse" : "bg-green-500"}`}
      />
      <span
        className="text-xs font-mono font-bold tracking-wide"
        style={{ color: isEgo ? "#22c55e" : "#f59e0b" }}
      >
        {label}
      </span>
    </button>
  );
}
