import { useState, useEffect } from "react";
import { check, Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export default function UpdateNotification() {
  const [update, setUpdate] = useState<Update | null>(null);
  const [installing, setInstalling] = useState(false);
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    check()
      .then((u) => {
        if (!cancelled && u?.available) {
          setUpdate(u);
        }
      })
      .catch(() => {
        // Silently ignore: no internet, Linux DEB, dev mode, etc.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (!update || dismissed) return null;

  const handleInstall = async () => {
    setInstalling(true);
    try {
      await update.downloadAndInstall();
      await relaunch();
    } catch {
      // If install fails, dismiss the banner
      setDismissed(true);
    }
  };

  return (
    <div className="flex items-center justify-between gap-3 px-4 py-2 bg-theme-primary/10 border-b border-theme-primary/30 text-sm font-mono">
      <span className="text-theme-text">
        Update available: <strong>{update.version}</strong>
      </span>
      <div className="flex gap-2">
        <button
          onClick={handleInstall}
          disabled={installing}
          className="px-3 py-1 rounded border border-theme-primary text-theme-primary hover:bg-theme-primary-glow disabled:opacity-50"
        >
          {installing ? "Installing..." : "Install & Restart"}
        </button>
        <button
          onClick={() => setDismissed(true)}
          className="px-3 py-1 rounded border border-theme-border-dim text-theme-text-dim hover:bg-theme-bg-elevated"
        >
          Later
        </button>
      </div>
    </div>
  );
}
