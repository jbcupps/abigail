import JobsTable from "./JobsTable";

export default function OrchestrationPanel() {
  return (
    <div className="flex flex-col h-full p-4 gap-4">
      <div className="bg-theme-bg-elevated border border-theme-border-dim rounded-lg p-4">
        <h2 className="text-lg font-semibold text-theme-text-bright mb-2">Orchestration Jobs</h2>
        <p className="text-sm text-theme-text-dim">
          Orchestration controls are currently disabled in this runtime. Enable experimental UI and
          backend wiring before using scheduled jobs.
        </p>
      </div>
      <JobsTable
        jobs={[]}
        onToggle={() => {}}
        onDelete={() => {}}
        onRunNow={() => {}}
      />
    </div>
  );
}
