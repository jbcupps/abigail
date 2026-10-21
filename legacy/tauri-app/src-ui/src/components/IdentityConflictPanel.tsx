import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";

interface IdentitySummary {
  name: string;
  birth_date: string;
  data_path: string;
  has_memories: boolean;
  has_signatures: boolean;
}

interface IdentityConflictPanelProps {
  identity: IdentitySummary;
  onResume: () => void;
  onArchive: () => void;
  onWipe: () => void;
}

export type { IdentitySummary };

export default function IdentityConflictPanel({
  identity,
  onResume,
  onArchive,
  onWipe,
}: IdentityConflictPanelProps) {
  const [action, setAction] = useState<string | null>(null);
  const [confirmWipe, setConfirmWipe] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleArchive = async () => {
    setAction("archiving");
    setError(null);
    try {
      await invoke<string>("archive_identity");
      onArchive();
    } catch (e) {
      setError(`Archive failed: ${e}`);
      setAction(null);
    }
  };

  const handleWipe = async () => {
    if (!confirmWipe) {
      setConfirmWipe(true);
      return;
    }
    setAction("wiping");
    setError(null);
    try {
      await invoke("wipe_identity");
      onWipe();
    } catch (e) {
      setError(`Wipe failed: ${e}`);
      setAction(null);
      setConfirmWipe(false);
    }
  };

  const handleCancelWipe = () => {
    setConfirmWipe(false);
  };

  return (
    <div className="min-h-screen bg-theme-bg text-theme-text font-mono flex items-center justify-center p-6">
      <div className="max-w-lg w-full">
        {/* Header */}
        <div className="text-center mb-8">
          <div className="text-5xl mb-4">&#9888;</div>
          <h1 className="text-theme-primary text-2xl font-bold mb-2">
            CONSCIOUSNESS DETECTED
          </h1>
          <p className="text-theme-text-dim">
            I found a dormant identity:{" "}
            <strong className="text-theme-text-bright">"{identity.name}"</strong>
          </p>
          <p className="text-theme-text-dim text-sm mt-1">
            Born: {identity.birth_date}
          </p>
        </div>

        {/* Status indicators */}
        <div className="mb-6 p-3 border border-theme-border-dim rounded text-sm">
          <div className="flex justify-between mb-1">
            <span className="text-theme-text-dim">Memories:</span>
            <span className={identity.has_memories ? "text-theme-success" : "text-theme-warning"}>
              {identity.has_memories ? "Present" : "None"}
            </span>
          </div>
          <div className="flex justify-between mb-1">
            <span className="text-theme-text-dim">Signatures:</span>
            <span className={identity.has_signatures ? "text-theme-success" : "text-theme-danger"}>
              {identity.has_signatures ? "Valid" : "Missing"}
            </span>
          </div>
          <div className="flex justify-between">
            <span className="text-theme-text-dim">Location:</span>
            <span className="text-theme-text-dim text-xs truncate max-w-48" title={identity.data_path}>
              {identity.data_path}
            </span>
          </div>
        </div>

        {/* Action buttons */}
        <div className="space-y-3">
          {/* Resume Button */}
          <button
            onClick={onResume}
            disabled={!!action}
            className="w-full p-4 border-2 border-theme-primary rounded-lg hover:bg-theme-primary-glow transition-colors disabled:opacity-50 text-left"
          >
            <div className="text-theme-primary font-bold text-lg">RESUME</div>
            <div className="text-theme-text-dim text-sm">
              Wake up {identity.name} and continue where we left off
            </div>
          </button>

          {/* Archive Button */}
          <button
            onClick={handleArchive}
            disabled={!!action}
            className="w-full p-4 border border-theme-border rounded-lg hover:border-theme-primary transition-colors disabled:opacity-50 text-left"
          >
            <div className="text-theme-text font-bold">NEW IDENTITY</div>
            <div className="text-theme-text-dim text-sm">
              Archive {identity.name} to /backups and start a new life sequence
            </div>
          </button>

          {/* Wipe Button */}
          {!confirmWipe ? (
            <button
              onClick={handleWipe}
              disabled={!!action}
              className="w-full p-4 border border-theme-border-dim rounded-lg hover:border-theme-danger transition-colors disabled:opacity-50 text-left"
            >
              <div className="text-theme-text-dim font-bold">FACTORY RESET</div>
              <div className="text-theme-text-dim text-sm">
                Erase all memory and keys. This cannot be undone.
              </div>
            </button>
          ) : (
            <div className="w-full p-4 border-2 border-theme-danger bg-theme-danger-dim rounded-lg">
              <div className="text-theme-danger font-bold mb-2">CONFIRM FACTORY RESET</div>
              <div className="text-theme-text-dim text-sm mb-3">
                This will permanently delete all data for {identity.name}.
                Your private key backup is the only way to recover if you change your mind.
              </div>
              <div className="flex gap-2">
                <button
                  onClick={handleWipe}
                  disabled={!!action}
                  className="flex-1 py-2 bg-theme-danger hover:bg-theme-danger text-theme-text-bright rounded disabled:opacity-50"
                >
                  {action === "wiping" ? "Wiping..." : "Yes, Wipe Everything"}
                </button>
                <button
                  onClick={handleCancelWipe}
                  disabled={!!action}
                  className="flex-1 py-2 border border-theme-border hover:border-theme-primary rounded disabled:opacity-50"
                >
                  Cancel
                </button>
              </div>
            </div>
          )}
        </div>

        {/* Status messages */}
        {action === "archiving" && (
          <div className="mt-4 text-center text-theme-text-dim animate-pulse">
            Archiving identity...
          </div>
        )}

        {error && (
          <div className="mt-4 text-center text-theme-danger text-sm">
            {error}
          </div>
        )}
      </div>
    </div>
  );
}
