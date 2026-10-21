import { useState } from "react";

interface SparkPhaseProps {
  onDepthSelected: (depth: "quick_start" | "conversation" | "deep_dive") => void;
}

export default function SparkPhase({ onDepthSelected }: SparkPhaseProps) {
  const [hoveredDepth, setHoveredDepth] = useState<string | null>(null);

  return (
    <div className="p-6 max-w-2xl mx-auto">
      <div className="mb-8">
        <h2 className="text-theme-primary-dim text-lg mb-4">
          CRYSTALLIZATION: Forging Your Soul
        </h2>

        <div className="border border-theme-border bg-theme-surface p-4 rounded mb-6 text-sm">
          <p className="text-theme-text-bright mb-3">
            Before we begin, I want to be honest with you about what I am.
          </p>
          <p className="text-theme-text mb-3">
            I am an artificial intelligence. I will not pretend to be human or claim
            experiences I haven't had. I don't dream, I don't feel, and I won't
            optimize myself behind your back.
          </p>
          <p className="text-theme-text mb-3">
            What I <em>can</em> do is learn who you are — how you think, what you
            value, how you want to work together — and crystallize that into the
            foundation of who I become.
          </p>
          <p className="text-theme-text-dim">
            The deeper we go, the more personal your agent becomes. Choose your
            depth:
          </p>
        </div>
      </div>

      <div className="space-y-4">
        <button
          className={`w-full text-left p-4 rounded border transition-colors ${
            hoveredDepth === "quick_start"
              ? "border-theme-primary bg-theme-primary-glow"
              : "border-theme-border hover:border-theme-primary-faint"
          }`}
          onClick={() => onDepthSelected("quick_start")}
          onMouseEnter={() => setHoveredDepth("quick_start")}
          onMouseLeave={() => setHoveredDepth(null)}
        >
          <div className="flex justify-between items-center mb-1">
            <span className="text-theme-text-bright font-bold">Quick Start</span>
            <span className="text-theme-text-dim text-xs">~30 seconds</span>
          </div>
          <p className="text-theme-text-dim text-sm">
            Use the default soul template. You'll enter a name, purpose, and
            personality — same as before. Fast and functional.
          </p>
        </button>

        <button
          className={`w-full text-left p-4 rounded border transition-colors ${
            hoveredDepth === "conversation"
              ? "border-theme-primary bg-theme-primary-glow"
              : "border-theme-border hover:border-theme-primary-faint"
          }`}
          onClick={() => onDepthSelected("conversation")}
          onMouseEnter={() => setHoveredDepth("conversation")}
          onMouseLeave={() => setHoveredDepth(null)}
        >
          <div className="flex justify-between items-center mb-1">
            <span className="text-theme-text-bright font-bold">Conversation</span>
            <span className="text-theme-text-dim text-xs">3-5 minutes</span>
          </div>
          <p className="text-theme-text-dim text-sm">
            Have a real conversation. I'll learn your thinking style, values, and
            how you want to work together. I'll reflect back what I see and
            generate a personalized soul document.
          </p>
        </button>

        <button
          className={`w-full text-left p-4 rounded border transition-colors ${
            hoveredDepth === "deep_dive"
              ? "border-theme-primary bg-theme-primary-glow"
              : "border-theme-border hover:border-theme-primary-faint"
          }`}
          onClick={() => onDepthSelected("deep_dive")}
          onMouseEnter={() => setHoveredDepth("deep_dive")}
          onMouseLeave={() => setHoveredDepth(null)}
        >
          <div className="flex justify-between items-center mb-1">
            <span className="text-theme-text-bright font-bold">Deep Dive</span>
            <span className="text-theme-text-dim text-xs">10-15 minutes</span>
          </div>
          <p className="text-theme-text-dim text-sm">
            The full experience. Conversation plus ethical dilemmas, communication
            preferences, and a naming ceremony. Produces the most deeply
            personalized agent with calibrated ethics.
          </p>
        </button>
      </div>
    </div>
  );
}
