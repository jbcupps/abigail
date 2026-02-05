import { invoke } from "@tauri-apps/api/core";
import { useState, useEffect } from "react";

interface SqliteStats {
  size_bytes: number;
  memory_count: number;
  has_birth: boolean;
}

interface BackupDialogProps {
  onBackup: (path: string) => void;
  onCancel: () => void;
}

function BackupDialog({ onBackup, onCancel }: BackupDialogProps) {
  const [path, setPath] = useState("");

  const handleSubmit = () => {
    if (path.trim()) {
      // Ensure .db extension
      const finalPath = path.trim().endsWith(".db") ? path.trim() : `${path.trim()}.db`;
      onBackup(finalPath);
    }
  };

  return (
    <div className="fixed inset-0 bg-black/80 flex items-center justify-center z-50">
      <div className="bg-black border border-theme-border rounded-lg p-6 max-w-md w-full mx-4">
        <h3 className="text-theme-text font-bold mb-4">Backup Database</h3>
        <p className="text-theme-text-dim text-sm mb-4">
          Enter the full path where you want to save the backup:
        </p>
        <input
          type="text"
          value={path}
          onChange={(e) => setPath(e.target.value)}
          placeholder="C:\Users\...\ao_backup.db"
          className="w-full bg-black border border-theme-primary text-theme-text px-3 py-2 rounded mb-4 text-sm"
          autoFocus
          onKeyDown={(e) => e.key === "Enter" && handleSubmit()}
        />
        <div className="flex gap-2">
          <button
            onClick={handleSubmit}
            disabled={!path.trim()}
            className="flex-1 py-2 border border-theme-primary rounded hover:bg-theme-primary-glow disabled:opacity-50"
          >
            Backup
          </button>
          <button
            onClick={onCancel}
            className="flex-1 py-2 border border-theme-border rounded hover:border-theme-primary"
          >
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}

export default function DataSourcesPanel() {
  const [stats, setStats] = useState<SqliteStats | null>(null);
  const [loading, setLoading] = useState(true);
  const [actionInProgress, setActionInProgress] = useState<string | null>(null);
  const [message, setMessage] = useState<{ type: "success" | "error"; text: string } | null>(null);
  const [confirmReset, setConfirmReset] = useState(false);
  const [showBackupDialog, setShowBackupDialog] = useState(false);

  useEffect(() => {
    refreshStats();
  }, []);

  const refreshStats = async () => {
    setLoading(true);
    try {
      const result = await invoke<SqliteStats>("get_sqlite_stats");
      setStats(result);
    } catch (e) {
      setMessage({ type: "error", text: `Failed to load stats: ${e}` });
    } finally {
      setLoading(false);
    }
  };

  const formatBytes = (bytes: number): string => {
    if (bytes === 0) return "0 B";
    const k = 1024;
    const sizes = ["B", "KB", "MB", "GB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
  };

  const handleOptimize = async () => {
    setActionInProgress("optimize");
    setMessage(null);
    try {
      const saved = await invoke<number>("optimize_sqlite");
      if (saved > 0) {
        setMessage({ type: "success", text: `Optimized! ${formatBytes(saved)} freed.` });
      } else {
        setMessage({ type: "success", text: "Database already optimal." });
      }
      await refreshStats();
    } catch (e) {
      setMessage({ type: "error", text: `Optimize failed: ${e}` });
    } finally {
      setActionInProgress(null);
    }
  };

  const handleBackup = () => {
    setShowBackupDialog(true);
  };

  const doBackup = async (destPath: string) => {
    setShowBackupDialog(false);
    setActionInProgress("backup");
    setMessage(null);
    try {
      await invoke("backup_sqlite", { destPath });
      setMessage({ type: "success", text: `Backup saved to ${destPath}` });
    } catch (e) {
      setMessage({ type: "error", text: `Backup failed: ${e}` });
    } finally {
      setActionInProgress(null);
    }
  };

  const handleReset = async () => {
    if (!confirmReset) {
      setConfirmReset(true);
      return;
    }

    setActionInProgress("reset");
    setMessage(null);
    try {
      const deleted = await invoke<number>("reset_memories");
      setMessage({ type: "success", text: `Cleared ${deleted} memories. Birth record preserved.` });
      setConfirmReset(false);
      await refreshStats();
    } catch (e) {
      setMessage({ type: "error", text: `Reset failed: ${e}` });
    } finally {
      setActionInProgress(null);
    }
  };

  return (
    <div className="p-6">
      <h2 className="text-theme-primary-dim text-lg mb-4">Data Archives</h2>
      <p className="text-theme-text-dim text-sm mb-6">
        Manage the internal SQLite memory database.
      </p>

      {/* SQLite Stats */}
      <div className="border border-theme-border rounded-lg p-4 mb-6">
        <h3 className="text-theme-text font-bold mb-3">Internal Memory (SQLite)</h3>

        {loading ? (
          <div className="text-theme-text-dim animate-pulse">Loading...</div>
        ) : stats ? (
          <div className="space-y-2 text-sm">
            <div className="flex justify-between">
              <span className="text-theme-text-dim">Database Size:</span>
              <span className="text-theme-text-bright">{formatBytes(stats.size_bytes)}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-theme-text-dim">Memory Count:</span>
              <span className="text-theme-text-bright">{stats.memory_count.toLocaleString()}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-theme-text-dim">Birth Record:</span>
              <span className={stats.has_birth ? "text-green-400" : "text-yellow-400"}>
                {stats.has_birth ? "Present" : "Missing"}
              </span>
            </div>
          </div>
        ) : (
          <div className="text-red-400">Failed to load stats</div>
        )}
      </div>

      {/* Actions */}
      <div className="space-y-3">
        {/* Optimize Button */}
        <button
          onClick={handleOptimize}
          disabled={!!actionInProgress}
          className="w-full p-3 border border-theme-border rounded-lg hover:border-theme-primary transition-colors disabled:opacity-50 text-left flex items-center justify-between"
        >
          <div>
            <div className="text-theme-text font-bold">OPTIMIZE</div>
            <div className="text-theme-text-dim text-sm">
              Run VACUUM to reclaim space and defragment
            </div>
          </div>
          {actionInProgress === "optimize" && (
            <span className="text-theme-text-dim animate-pulse">...</span>
          )}
        </button>

        {/* Backup Button */}
        <button
          onClick={handleBackup}
          disabled={!!actionInProgress}
          className="w-full p-3 border border-theme-border rounded-lg hover:border-theme-primary transition-colors disabled:opacity-50 text-left flex items-center justify-between"
        >
          <div>
            <div className="text-theme-text font-bold">BACKUP</div>
            <div className="text-theme-text-dim text-sm">
              Export database to a backup file
            </div>
          </div>
          {actionInProgress === "backup" && (
            <span className="text-theme-text-dim animate-pulse">...</span>
          )}
        </button>

        {/* Reset Button */}
        {!confirmReset ? (
          <button
            onClick={handleReset}
            disabled={!!actionInProgress}
            className="w-full p-3 border border-theme-border-dim rounded-lg hover:border-red-500 transition-colors disabled:opacity-50 text-left"
          >
            <div className="text-theme-text-dim font-bold">RESET MEMORIES</div>
            <div className="text-theme-text-dim text-sm">
              Clear all memories (birth record preserved)
            </div>
          </button>
        ) : (
          <div className="w-full p-3 border-2 border-red-500 bg-red-900/20 rounded-lg">
            <div className="text-red-500 font-bold mb-2">CONFIRM RESET</div>
            <div className="text-theme-text-dim text-sm mb-3">
              This will delete all {stats?.memory_count || 0} memories.
              The birth record will be preserved.
            </div>
            <div className="flex gap-2">
              <button
                onClick={handleReset}
                disabled={!!actionInProgress}
                className="flex-1 py-2 bg-red-600 hover:bg-red-700 text-white rounded text-sm disabled:opacity-50"
              >
                {actionInProgress === "reset" ? "Resetting..." : "Yes, Reset"}
              </button>
              <button
                onClick={() => setConfirmReset(false)}
                disabled={!!actionInProgress}
                className="flex-1 py-2 border border-theme-border hover:border-theme-primary rounded text-sm disabled:opacity-50"
              >
                Cancel
              </button>
            </div>
          </div>
        )}
      </div>

      {/* Status Message */}
      {message && (
        <div
          className={`mt-4 p-3 rounded text-sm ${
            message.type === "success"
              ? "bg-green-900/20 border border-green-700 text-green-400"
              : "bg-red-900/20 border border-red-700 text-red-400"
          }`}
        >
          {message.text}
        </div>
      )}

      {/* Future: External Connections */}
      <div className="mt-8 pt-6 border-t border-theme-border-dim">
        <h3 className="text-theme-text-dim font-bold mb-2">External Connections</h3>
        <p className="text-theme-text-dim text-sm">
          Postgres and Neo4j connections coming soon.
        </p>
      </div>

      {/* Backup Dialog */}
      {showBackupDialog && (
        <BackupDialog
          onBackup={doBackup}
          onCancel={() => setShowBackupDialog(false)}
        />
      )}
    </div>
  );
}
