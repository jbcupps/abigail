import { useTheme } from "../contexts/ThemeContext";

export default function PersonaToggle() {
  const { mode, setMode, agentName } = useTheme();

  const isEgo = mode === "ego";
  const label = isEgo ? (agentName || "Abigail") : "THE FORGE";
  const tooltip = isEgo ? "Access Core Identity (Id)" : "Return to Surface (Ego)";

  return (
    <button
      onClick={() => setMode(isEgo ? "id" : "ego")}
      title={tooltip}
      role="switch"
      aria-checked={isEgo}
      className={`fixed top-3 right-3 z-50 flex items-center gap-2 px-3 py-1.5 rounded-full border bg-theme-bg/80 backdrop-blur-sm transition-colors hover:bg-theme-bg/90 ${
        isEgo ? "border-green-500" : "border-amber-500"
      }`}
    >
      <span
        className={`w-2.5 h-2.5 rounded-full ${isEgo ? "bg-red-500 animate-glow-pulse" : "bg-theme-success"}`}
      />
      <span
        className={`text-xs font-mono font-bold tracking-wide ${
          isEgo ? "text-green-500" : "text-amber-500"
        }`}
      >
        {label}
      </span>
    </button>
  );
}
