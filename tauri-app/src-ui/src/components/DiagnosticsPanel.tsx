import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface DiagnosticsPanelProps {
  onNavigate?: (tab: string) => void;
}

export default function DiagnosticsPanel({ onNavigate }: DiagnosticsPanelProps) {
  const [report, setReport] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const runDiagnostics = async () => {
    setLoading(true);
    setError(null);
    try {
      const result = await invoke<string>("get_system_diagnostics");
      setReport(result);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="p-6 space-y-4">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-theme-primary-dim text-lg font-bold">Diagnostics</h2>
          <p className="text-theme-text-dim text-xs mt-1">
            Run a comprehensive check of router, email, skills, and integrations.
          </p>
        </div>
        <button
          className="border border-theme-primary px-4 py-2 rounded hover:bg-theme-primary-glow text-sm disabled:opacity-50"
          onClick={runDiagnostics}
          disabled={loading}
        >
          {loading ? "Running..." : "Run Diagnostics"}
        </button>
      </div>

      {error && (
        <div className="bg-red-950/30 border border-red-800 rounded p-3 text-red-400 text-sm">
          {error}
        </div>
      )}

      {report && (
        <div className="bg-theme-bg-inset border border-theme-border-dim rounded p-4 overflow-auto max-h-[60vh]">
          <pre className="text-sm text-theme-text-bright whitespace-pre-wrap font-mono">
            {report}
          </pre>
        </div>
      )}

      {onNavigate && (
        <div className="flex gap-2 pt-2">
          <button
            className="text-xs border border-theme-border-dim text-theme-text-dim hover:text-theme-text px-3 py-1.5 rounded"
            onClick={() => onNavigate("keys")}
          >
            Configure API Keys
          </button>
          <button
            className="text-xs border border-theme-border-dim text-theme-text-dim hover:text-theme-text px-3 py-1.5 rounded"
            onClick={() => onNavigate("llm")}
          >
            LLM Setup
          </button>
        </div>
      )}
    </div>
  );
}
