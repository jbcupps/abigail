import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import JobsTable from "./JobsTable";

interface OrchestrationJob {
  job_id: string;
  name: string;
  cron_expression: string;
  mode: string;
  goal_template: string | null;
  enabled: boolean;
  created_at: string;
  updated_at: string;
}

export default function OrchestrationPanel() {
  const [jobs, setJobs] = useState<OrchestrationJob[]>([]);
  const [showCreate, setShowCreate] = useState(false);
  const [newJob, setNewJob] = useState({
    name: "",
    cron: "0 */6 * * *",
    mode: "id_check" as string,
    goal: "",
  });

  const loadJobs = async () => {
    try {
      const result = await invoke<OrchestrationJob[]>("list_orchestration_jobs");
      setJobs(result);
    } catch (e) {
      console.error("Failed to load jobs:", e);
    }
  };

  useEffect(() => {
    loadJobs();
  }, []);

  const createJob = async () => {
    if (!newJob.name.trim()) return;
    try {
      await invoke("create_orchestration_job", {
        name: newJob.name,
        cronExpression: newJob.cron,
        mode: newJob.mode,
        goalTemplate: newJob.goal || null,
      });
      setShowCreate(false);
      setNewJob({ name: "", cron: "0 */6 * * *", mode: "id_check", goal: "" });
      loadJobs();
    } catch (e) {
      console.error("Failed to create job:", e);
    }
  };

  const toggleJob = async (jobId: string, enabled: boolean) => {
    try {
      await invoke("enable_orchestration_job", { jobId, enabled });
      loadJobs();
    } catch (e) {
      console.error("Failed to toggle job:", e);
    }
  };

  const deleteJob = async (jobId: string) => {
    try {
      await invoke("delete_orchestration_job", { jobId });
      loadJobs();
    } catch (e) {
      console.error("Failed to delete job:", e);
    }
  };

  const runNow = async (jobId: string) => {
    try {
      await invoke("run_orchestration_job_now", { jobId });
    } catch (e) {
      console.error("Failed to run job:", e);
    }
  };

  return (
    <div className="flex flex-col h-full p-4 gap-4">
      <div className="flex items-center justify-between">
        <h2 className="text-lg font-semibold text-white">Orchestration Jobs</h2>
        <button
          className="bg-blue-600 hover:bg-blue-700 text-white px-3 py-1 rounded text-sm"
          onClick={() => setShowCreate(!showCreate)}
        >
          {showCreate ? "Cancel" : "New Job"}
        </button>
      </div>

      {/* Create job form */}
      {showCreate && (
        <div className="bg-gray-800 rounded-lg p-4 flex flex-col gap-3">
          <input
            className="bg-gray-700 text-white rounded px-3 py-2"
            placeholder="Job name"
            value={newJob.name}
            onChange={(e) => setNewJob({ ...newJob, name: e.target.value })}
          />
          <div className="flex gap-3">
            <input
              className="flex-1 bg-gray-700 text-white rounded px-3 py-2"
              placeholder="Cron expression (e.g. 0 */6 * * *)"
              value={newJob.cron}
              onChange={(e) => setNewJob({ ...newJob, cron: e.target.value })}
            />
            <select
              className="bg-gray-700 text-white rounded px-3 py-2"
              value={newJob.mode}
              onChange={(e) => setNewJob({ ...newJob, mode: e.target.value })}
            >
              <option value="id_check">Id Check</option>
              <option value="agentic_run">Agentic Run</option>
            </select>
          </div>
          {newJob.mode === "agentic_run" && (
            <textarea
              className="bg-gray-700 text-white rounded px-3 py-2 resize-none"
              rows={2}
              placeholder="Goal template for agentic run..."
              value={newJob.goal}
              onChange={(e) => setNewJob({ ...newJob, goal: e.target.value })}
            />
          )}
          <button
            className="bg-green-600 hover:bg-green-700 text-white px-4 py-2 rounded self-end"
            onClick={createJob}
          >
            Create
          </button>
        </div>
      )}

      {/* Jobs table */}
      <JobsTable
        jobs={jobs}
        onToggle={toggleJob}
        onDelete={deleteJob}
        onRunNow={runNow}
      />
    </div>
  );
}
