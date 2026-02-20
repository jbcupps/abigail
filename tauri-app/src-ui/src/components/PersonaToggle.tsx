import { useTheme } from "../contexts/ThemeContext";

interface PersonaToggleProps {
  onToggle: () => void;
  forgeOpen?: boolean;
}

export default function PersonaToggle({ onToggle, forgeOpen }: PersonaToggleProps) {
  const { agentName } = useTheme();

  const label = agentName || "Abigail";

  return (
    <button
      onClick={onToggle}
      title={forgeOpen ? "Close The Forge" : "Open The Forge"}
      className={`fixed top-3 right-3 z-50 flex items-center gap-2 px-3 py-1.5 rounded-full border bg-theme-bg/80 backdrop-blur-sm transition-colors hover:bg-theme-bg/90 ${
        forgeOpen ? "border-amber-500" : "border-green-500"
      }`}
    >
      {/* Gear icon */}
      <svg
        className={`w-3.5 h-3.5 ${forgeOpen ? "text-amber-500" : "text-green-500"}`}
        fill="none"
        viewBox="0 0 24 24"
        stroke="currentColor"
        strokeWidth={2}
      >
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.066 2.573c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.573 1.066c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.066-2.573c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z"
        />
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"
        />
      </svg>
      <span
        className={`text-xs font-mono font-bold tracking-wide ${
          forgeOpen ? "text-amber-500" : "text-green-500"
        }`}
      >
        {label}
      </span>
    </button>
  );
}
