import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";
import SparkPhase from "./crystallization/SparkPhase";
import BirthChat from "./BirthChat";

type CrystallizationDepth = "quick_start" | "conversation" | "deep_dive";
type Phase = "spark" | "conversation" | "soul_preview" | "complete";

interface SoulCrystallizationProps {
  /** Called when Quick Start finishes and legacy SoulPreview should be shown. */
  onQuickStart: () => void;
  /** Called when conversation-based crystallization finishes and SoulPreview should show. */
  onCrystallizationComplete: (identity: {
    name: string;
    purpose: string;
    personality: string;
  }) => void;
  onError: (error: string) => void;
}

export default function SoulCrystallization({
  onQuickStart,
  onCrystallizationComplete,
  onError,
}: SoulCrystallizationProps) {
  const [phase, setPhase] = useState<Phase>("spark");
  const [depth, setDepth] = useState<CrystallizationDepth | null>(null);
  const [_message, setMessage] = useState("");

  const handleDepthSelected = async (selectedDepth: CrystallizationDepth) => {
    setDepth(selectedDepth);

    try {
      await invoke<string>("start_crystallization", { depth: selectedDepth });

      if (selectedDepth === "quick_start") {
        // Quick Start: skip conversation, go directly to legacy SoulPreview
        onQuickStart();
      } else {
        // Conversation or Deep Dive: enter conversation phase
        setPhase("conversation");
      }
    } catch (e) {
      onError(String(e));
    }
  };

  const handleConversationDone = async () => {
    // Extract identity from conversation
    setMessage("Extracting identity from conversation...");
    try {
      const identity = await invoke<{
        name: string | null;
        purpose: string | null;
        personality: string | null;
      }>("extract_crystallization_identity");

      onCrystallizationComplete({
        name: identity.name || "",
        purpose: identity.purpose || "",
        personality: identity.personality || "",
      });
    } catch (e) {
      console.warn("Could not extract identity:", e);
      // Fall back to empty form
      onCrystallizationComplete({
        name: "",
        purpose: "",
        personality: "",
      });
    }
  };

  return (
    <div className="flex flex-col h-full">
      {phase === "spark" && (
        <SparkPhase onDepthSelected={handleDepthSelected} />
      )}

      {phase === "conversation" && (
        <div className="flex flex-col h-full" style={{ minHeight: "60vh" }}>
          <BirthChat
            stage="Crystallization"
            onStageAdvance={handleConversationDone}
            onAction={(action) => {
              if (action.type === "SoulReady" && action.preview) {
                try {
                  const data = JSON.parse(action.preview);
                  onCrystallizationComplete({
                    name: data.name || "",
                    purpose: data.purpose || "",
                    personality: data.personality || "",
                  });
                } catch {
                  // If preview isn't valid JSON, let manual flow continue
                }
              }
            }}
          />
          {depth && (
            <div className="px-4 py-1 border-t border-theme-border-dim">
              <span className="text-theme-text-dim text-xs">
                Depth: {depth === "conversation" ? "Conversation" : "Deep Dive"}
              </span>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
