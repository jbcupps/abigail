import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

type FaultMode = "none" | "chat_timeout" | "chat_error" | "provider_validation_error";

interface Snapshot {
  runtime: "browser-harness";
  config: { strict: boolean; trace: boolean; seed: number };
  faultMode: FaultMode;
  state: {
    activeAgentId: string | null;
    birthComplete: boolean;
    birthStage: string;
    providers: string[];
    activeProviderPreference: string | null;
    localLlmUrl: string | null;
    memoryDisclosureEnabled: boolean;
    listenerCount: number;
  };
}

export default function HarnessDebugPanel() {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [open, setOpen] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = async () => {
    try {
      const next = await invoke<Snapshot>("harness_debug_snapshot");
      setSnapshot(next);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  };

  useEffect(() => {
    refresh();
    const timer = setInterval(refresh, 1200);
    return () => clearInterval(timer);
  }, []);

  const setFault = async (mode: FaultMode) => {
    setLoading(true);
    try {
      await invoke("harness_debug_set_fault", { mode });
      await refresh();
    } finally {
      setLoading(false);
    }
  };

  const resetHarness = async () => {
    setLoading(true);
    try {
      await invoke("harness_debug_reset");
      await refresh();
    } finally {
      setLoading(false);
    }
  };

  const toggleTrace = async (enabled: boolean) => {
    setLoading(true);
    try {
      await invoke("harness_debug_config", { trace: enabled });
      await refresh();
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="fixed top-10 right-2 z-[9999] w-80 text-xs font-mono">
      <button
        className="w-full px-2 py-1 rounded border border-theme-border-dim bg-theme-bg-elevated text-theme-text-dim hover:text-theme-text"
        onClick={() => setOpen((v) => !v)}
      >
        Harness Debug {open ? "[-]" : "[+]"}
      </button>
      {open && (
        <div className="mt-1 p-2 rounded border border-theme-border-dim bg-theme-bg-elevated text-theme-text-dim space-y-2">
          {error && <p className="text-red-400">{error}</p>}
          <div>
            <p>stage: {snapshot?.state.birthStage ?? "?"}</p>
            <p>agent: {snapshot?.state.activeAgentId ?? "none"}</p>
            <p>providers: {(snapshot?.state.providers ?? []).join(", ") || "none"}</p>
            <p>listeners: {snapshot?.state.listenerCount ?? 0}</p>
            <p>fault: {snapshot?.faultMode ?? "none"}</p>
          </div>
          <div className="grid grid-cols-2 gap-1">
            <button
              className="px-2 py-1 border border-theme-border-dim rounded hover:border-theme-primary"
              disabled={loading}
              onClick={() => setFault("none")}
            >
              fault:none
            </button>
            <button
              className="px-2 py-1 border border-theme-border-dim rounded hover:border-theme-primary"
              disabled={loading}
              onClick={() => setFault("chat_error")}
            >
              chat:error
            </button>
            <button
              className="px-2 py-1 border border-theme-border-dim rounded hover:border-theme-primary"
              disabled={loading}
              onClick={() => setFault("chat_timeout")}
            >
              chat:timeout
            </button>
            <button
              className="px-2 py-1 border border-theme-border-dim rounded hover:border-theme-primary"
              disabled={loading}
              onClick={() => setFault("provider_validation_error")}
            >
              key:error
            </button>
          </div>
          <div className="grid grid-cols-2 gap-1">
            <button
              className="px-2 py-1 border border-theme-border-dim rounded hover:border-theme-primary"
              disabled={loading}
              onClick={() => toggleTrace(true)}
            >
              trace:on
            </button>
            <button
              className="px-2 py-1 border border-theme-border-dim rounded hover:border-theme-primary"
              disabled={loading}
              onClick={() => toggleTrace(false)}
            >
              trace:off
            </button>
          </div>
          <button
            className="w-full px-2 py-1 border border-theme-border-dim rounded hover:border-theme-primary"
            disabled={loading}
            onClick={resetHarness}
          >
            reset harness state
          </button>
        </div>
      )}
    </div>
  );
}

