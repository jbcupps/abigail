interface Job {
  job_id: string;
  name: string;
  cron_expression: string;
  mode: string;
  enabled: boolean;
  created_at: string;
  updated_at: string;
}

interface Props {
  jobs: Job[];
  onToggle: (jobId: string, enabled: boolean) => void;
  onDelete: (jobId: string) => void;
  onRunNow: (jobId: string) => void;
}

export default function JobsTable({ jobs, onToggle, onDelete, onRunNow }: Props) {
  if (jobs.length === 0) {
    return (
      <div className="flex-1 flex items-center justify-center text-theme-text-dim">
        No orchestration jobs configured. Create one to get started.
      </div>
    );
  }

  return (
    <div className="flex-1 overflow-y-auto">
      <table className="w-full text-sm text-left">
        <thead className="text-theme-text-dim border-b border-theme-border-dim">
          <tr>
            <th className="py-2 px-3">Name</th>
            <th className="py-2 px-3">Schedule</th>
            <th className="py-2 px-3">Mode</th>
            <th className="py-2 px-3">Status</th>
            <th className="py-2 px-3 text-right">Actions</th>
          </tr>
        </thead>
        <tbody>
          {jobs.map((job) => (
            <tr key={job.job_id} className="border-b border-theme-border-dim hover:bg-theme-hover">
              <td className="py-2 px-3 text-theme-text-bright">{job.name}</td>
              <td className="py-2 px-3 text-theme-text-dim font-mono text-xs">
                {job.cron_expression}
              </td>
              <td className="py-2 px-3">
                <span className={`text-xs px-2 py-0.5 rounded ${
                  job.mode === "agentic_run" ? "bg-purple-900/50 text-purple-300" : "bg-blue-900/50 text-blue-300"
                }`}>
                  {job.mode === "agentic_run" ? "Agentic" : "Id Check"}
                </span>
              </td>
              <td className="py-2 px-3">
                <span className={`text-xs ${job.enabled ? "text-theme-success" : "text-theme-text-dim"}`}>
                  {job.enabled ? "Enabled" : "Disabled"}
                </span>
              </td>
              <td className="py-2 px-3 text-right space-x-2">
                <button
                  className="text-xs text-theme-info hover:text-theme-primary-dim"
                  onClick={() => onRunNow(job.job_id)}
                >
                  Run Now
                </button>
                <button
                  className="text-xs text-theme-warning hover:text-theme-text-bright"
                  onClick={() => onToggle(job.job_id, !job.enabled)}
                >
                  {job.enabled ? "Disable" : "Enable"}
                </button>
                <button
                  className="text-xs text-theme-danger hover:text-theme-text-bright"
                  onClick={() => onDelete(job.job_id)}
                >
                  Delete
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
