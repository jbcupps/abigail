import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

interface JobRecord {
  id: string;
  topic: string;
  goal: string;
  status: string;
  priority: string;
  capability: string;
  is_recurring: boolean;
  cron_expression: string | null;
  result: string | null;
  error: string | null;
  created_at: string;
  started_at: string | null;
  completed_at: string | null;
}

const STATUS_BADGE: Record<string, string> = {
  queued: "bg-yellow-500/20 text-yellow-400 border-yellow-500/30",
  running: "bg-blue-500/20 text-blue-400 border-blue-500/30",
  completed: "bg-green-500/20 text-green-400 border-green-500/30",
  failed: "bg-red-500/20 text-red-400 border-red-500/30",
  cancelled: "bg-theme-text-dim/20 text-theme-text-dim border-theme-border-dim",
  expired: "bg-theme-text-dim/20 text-theme-text-dim border-theme-border-dim",
};

export default function OrchestrationPanel() {
  const [jobs, setJobs] = useState<JobRecord[]>([]);
  const [recurring, setRecurring] = useState<JobRecord[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [jobList, recurringList] = await Promise.all([
        invoke<JobRecord[]>("list_jobs", { status: filter, limit: 50 }),
        invoke<JobRecord[]>("list_recurring_templates"),
      ]);
      setJobs(jobList);
      setRecurring(recurringList);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [filter]);

  useEffect(() => {
    void refresh();
    const interval = setInterval(refresh, 5000);
    return () => clearInterval(interval);
  }, [refresh]);

  const cancelJob = async (jobId: string) => {
    try {
      await invoke("cancel_job", { jobId });
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  };

  const formatTime = (iso: string) => {
    try {
      return new Date(iso).toLocaleTimeString();
    } catch {
      return iso;
    }
  };

  return (
    <div className="flex flex-col h-full p-4 gap-4 overflow-y-auto">
      {/* Header */}
      <div className="flex items-center justify-between">
        <h2 className="text-sm font-semibold text-theme-text tracking-wider uppercase">
          Job Queue
        </h2>
        <div className="flex items-center gap-2">
          {["all", "queued", "running", "completed", "failed"].map((f) => (
            <button
              key={f}
              className={`px-2 py-0.5 text-[10px] rounded border font-mono transition-colors ${
                (f === "all" ? filter === null : filter === f)
                  ? "border-theme-primary text-theme-primary bg-theme-primary/10"
                  : "border-theme-border-dim text-theme-text-dim hover:border-theme-text-dim"
              }`}
              onClick={() => setFilter(f === "all" ? null : f)}
            >
              {f}
            </button>
          ))}
          <button
            onClick={refresh}
            className="px-2 py-0.5 text-[10px] rounded border border-theme-border-dim text-theme-text-dim hover:text-theme-text font-mono"
          >
            refresh
          </button>
        </div>
      </div>

      {error && (
        <p className="text-xs text-theme-danger bg-theme-danger/10 border border-theme-danger/30 rounded px-2 py-1">
          {error}
        </p>
      )}

      {/* Jobs list */}
      {loading && jobs.length === 0 ? (
        <p className="text-xs text-theme-text-dim animate-pulse">Loading jobs...</p>
      ) : jobs.length === 0 ? (
        <p className="text-xs text-theme-text-dim">No jobs found.</p>
      ) : (
        <div className="flex flex-col gap-1">
          {jobs.map((job) => (
            <div
              key={job.id}
              className="bg-theme-bg-elevated border border-theme-border-dim rounded px-3 py-2 cursor-pointer hover:border-theme-text-dim transition-colors"
              onClick={() => setExpanded(expanded === job.id ? null : job.id)}
            >
              <div className="flex items-center gap-2">
                <span
                  className={`px-1.5 py-0.5 text-[9px] font-mono rounded border ${
                    STATUS_BADGE[job.status] || STATUS_BADGE.queued
                  }`}
                >
                  {job.status}
                </span>
                <span className="text-xs text-theme-text font-mono truncate flex-1">
                  {job.goal.length > 80 ? job.goal.slice(0, 80) + "..." : job.goal}
                </span>
                <span className="text-[10px] text-theme-text-dim font-mono">
                  {formatTime(job.created_at)}
                </span>
                {(job.status === "queued" || job.status === "running") && (
                  <button
                    className="text-[10px] text-theme-danger hover:text-red-400 font-mono"
                    onClick={(e) => {
                      e.stopPropagation();
                      cancelJob(job.id);
                    }}
                  >
                    cancel
                  </button>
                )}
              </div>

              {expanded === job.id && (
                <div className="mt-2 pt-2 border-t border-theme-border-dim text-[10px] font-mono text-theme-text-dim space-y-1">
                  <div>ID: {job.id}</div>
                  <div>Topic: {job.topic}</div>
                  <div>Priority: {job.priority}</div>
                  <div>Capability: {job.capability}</div>
                  {job.started_at && <div>Started: {job.started_at}</div>}
                  {job.completed_at && <div>Completed: {job.completed_at}</div>}
                  {job.result && (
                    <div className="mt-1">
                      <span className="text-green-400">Result:</span>{" "}
                      {job.result.length > 200
                        ? job.result.slice(0, 200) + "..."
                        : job.result}
                    </div>
                  )}
                  {job.error && (
                    <div className="mt-1 text-theme-danger">Error: {job.error}</div>
                  )}
                </div>
              )}
            </div>
          ))}
        </div>
      )}

      {/* Recurring templates */}
      {recurring.length > 0 && (
        <div className="mt-2">
          <h3 className="text-[10px] font-semibold text-theme-text-dim tracking-wider uppercase mb-1">
            Recurring Schedules
          </h3>
          <div className="flex flex-col gap-1">
            {recurring.map((t) => (
              <div
                key={t.id}
                className="bg-theme-bg-elevated border border-theme-border-dim rounded px-3 py-1.5 flex items-center gap-2"
              >
                <span className="text-[10px] text-theme-primary font-mono">
                  {t.cron_expression || "—"}
                </span>
                <span className="text-xs text-theme-text-dim font-mono truncate flex-1">
                  {t.goal}
                </span>
                <span className="text-[10px] text-theme-text-dim font-mono">
                  {t.topic}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
