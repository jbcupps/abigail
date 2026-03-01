import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import JobsTable from "./JobsTable";

interface Job {
  job_id: string;
  name: string;
  cron_expression: string;
  mode: string;
  enabled: boolean;
  created_at: string;
  updated_at: string;
}

export default function OrchestrationPanel() {
  const [jobs, setJobs] = useState<Job[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busyJobId, setBusyJobId] = useState<string | null>(null);

  const refreshJobs = async () => {
    setLoading(true);
    setError(null);
    try {
      const nextJobs = await invoke<Job[]>("list_orchestration_jobs");
      setJobs(nextJobs);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void refreshJobs();
  }, []);

  const onToggle = async (jobId: string, enabled: boolean) => {
    setBusyJobId(jobId);
    setError(null);
    try {
      await invoke("set_orchestration_job_enabled", { jobId, enabled });
      await refreshJobs();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyJobId(null);
    }
  };

  const onDelete = async (jobId: string) => {
    setBusyJobId(jobId);
    setError(null);
    try {
      await invoke("delete_orchestration_job", { jobId });
      await refreshJobs();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyJobId(null);
    }
  };

  const onRunNow = async (jobId: string) => {
    setBusyJobId(jobId);
    setError(null);
    try {
      await invoke("run_orchestration_job_now", { jobId });
      await refreshJobs();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyJobId(null);
    }
  };

  return (
    <div className="flex flex-col h-full p-4 gap-4">
      <div className="bg-theme-bg-elevated border border-theme-border-dim rounded-lg p-4">
        <h2 className="text-lg font-semibold text-theme-text-bright mb-2">Orchestration Jobs</h2>
        <p className="text-sm text-theme-text-dim">
          Jobs are backed by persisted runtime orchestration state.
        </p>
        {loading && <p className="text-xs text-theme-text-dim mt-2">Loading jobs...</p>}
        {error && <p className="text-xs text-theme-danger mt-2">{error}</p>}
        {busyJobId && (
          <p className="text-xs text-theme-text-dim mt-2">Working on job "{busyJobId}"...</p>
        )}
      </div>
      <JobsTable
        jobs={jobs}
        onToggle={onToggle}
        onDelete={onDelete}
        onRunNow={onRunNow}
      />
    </div>
  );
}
