import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

interface AgentIdentityInfo {
  id: string;
  name: string;
  directory: string;
  birth_complete: boolean;
  birth_date: string | null;
}

interface ManagementScreenProps {
  onAgentSelected: (agentId: string) => void;
  onCreateAgent: () => void;
}

export default function ManagementScreen({
  onAgentSelected,
  onCreateAgent,
}: ManagementScreenProps) {
  const [agents, setAgents] = useState<AgentIdentityInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [newAgentName, setNewAgentName] = useState("");
  const [migrating, setMigrating] = useState(false);

  const fetchAgents = async () => {
    try {
      setLoading(true);
      const identities = await invoke<AgentIdentityInfo[]>("get_identities");
      setAgents(identities);
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchAgents();
  }, []);

  const handleCreateAgent = async () => {
    if (!newAgentName.trim()) return;

    try {
      setCreating(true);
      const uuid = await invoke<string>("create_agent", {
        name: newAgentName.trim(),
      });
      setNewAgentName("");
      setCreating(false);
      // Load the new agent and go to birth
      await invoke("load_agent", { agentId: uuid });
      onCreateAgent();
    } catch (e) {
      setError(String(e));
      setCreating(false);
    }
  };

  const handleSelectAgent = async (agentId: string) => {
    try {
      await invoke("load_agent", { agentId });
      onAgentSelected(agentId);
    } catch (e) {
      setError(String(e));
    }
  };

  const handleMigrateLegacy = async () => {
    try {
      setMigrating(true);
      const uuid = await invoke<string | null>("migrate_legacy_identity");
      if (uuid) {
        await fetchAgents();
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setMigrating(false);
    }
  };

  if (loading) {
    return (
      <div className="min-h-screen bg-theme-bg text-theme-text-dim font-mono flex items-center justify-center">
        <div className="animate-pulse">Loading identities...</div>
      </div>
    );
  }

  // Error banner (shared between both states)
  const errorBanner = error && (
    <div className="w-full max-w-lg mb-4 p-3 border border-red-800 rounded bg-red-900/20 text-red-400 text-sm">
      {error}
      <button
        className="ml-2 text-red-300 underline"
        onClick={() => setError(null)}
      >
        dismiss
      </button>
    </div>
  );

  // Empty state: Welcome landing page
  if (agents.length === 0) {
    return (
      <div className="min-h-screen bg-theme-bg text-theme-text font-mono flex flex-col items-center justify-center p-8">
        {/* Welcome header */}
        <div className="text-center mb-10">
          <h1 className="text-theme-primary text-4xl font-bold tracking-widest mb-2">
            ABIGAIL
          </h1>
          <p className="text-theme-text-dim text-sm">
            Your personal desktop AI agent
          </p>
        </div>

        {errorBanner}

        {/* Create agent card */}
        <div className="w-full max-w-md border-2 border-theme-primary rounded-lg p-6 mb-6">
          <p className="text-theme-text-bright text-sm mb-4">
            Create your first agent to begin
          </p>
          <div className="flex gap-2">
            <input
              type="text"
              value={newAgentName}
              onChange={(e) => setNewAgentName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") handleCreateAgent();
              }}
              placeholder="Agent name..."
              className="flex-1 bg-theme-bg border border-theme-border rounded px-3 py-2 text-theme-primary-dim placeholder:text-theme-text-dim focus:border-theme-primary focus:outline-none text-sm"
              disabled={creating}
              autoFocus
            />
            <button
              className="border border-theme-primary text-theme-primary px-5 py-2 rounded hover:bg-theme-primary-glow disabled:opacity-50 disabled:cursor-not-allowed text-sm font-bold"
              onClick={handleCreateAgent}
              disabled={creating || !newAgentName.trim()}
            >
              {creating ? "Creating..." : "Create"}
            </button>
          </div>
        </div>

        {/* Migration link */}
        <div className="text-center">
          <div className="text-theme-border text-xs mb-3">or</div>
          <button
            className="text-theme-text-dim hover:text-theme-text text-xs underline"
            onClick={handleMigrateLegacy}
            disabled={migrating}
          >
            {migrating ? "Checking..." : "Detect & migrate legacy identity"}
          </button>
        </div>
      </div>
    );
  }

  // Populated state: Agent selector
  return (
    <div className="min-h-screen bg-theme-bg text-theme-text font-mono flex flex-col items-center justify-center p-8">
      {/* Header */}
      <div className="text-center mb-8">
        <h1 className="text-xl text-theme-primary-dim font-bold">ABIGAIL HIVE</h1>
        <p className="text-theme-text-dim text-sm mt-1">Identity Management</p>
      </div>

      {errorBanner}

      {/* Agent table */}
      <div className="w-full max-w-2xl mb-8">
        <p className="text-theme-text-dim text-xs uppercase tracking-wider mb-3">
          Select an Identity
        </p>
        <table className="w-full border-collapse">
          <thead>
            <tr className="border-b border-theme-border text-left text-xs text-theme-text-dim uppercase tracking-wider">
              <th className="py-2 px-3">Name</th>
              <th className="py-2 px-3">UUID</th>
              <th className="py-2 px-3">Status</th>
              <th className="py-2 px-3">Birth Date</th>
            </tr>
          </thead>
          <tbody>
            {agents.map((agent) => (
              <tr
                key={agent.id}
                className="border-b border-theme-border-dim hover:bg-theme-primary-glow cursor-pointer transition-colors group"
                onClick={() => handleSelectAgent(agent.id)}
              >
                <td className="py-3 px-3 text-theme-primary-dim font-bold group-hover:text-theme-text-bright">
                  {agent.name}
                </td>
                <td className="py-3 px-3 text-theme-text-dim text-xs font-mono">
                  {agent.id.substring(0, 8)}...
                </td>
                <td className="py-3 px-3">
                  {agent.birth_complete ? (
                    <span className="text-green-600 text-xs">born</span>
                  ) : (
                    <span className="text-yellow-600 text-xs">unborn</span>
                  )}
                </td>
                <td className="py-3 px-3 text-theme-text-dim text-xs">
                  {agent.birth_date || "\u2014"}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {/* Create new agent */}
      <div className="w-full max-w-lg border border-theme-border-dim rounded p-4">
        <p className="text-theme-text-dim text-xs uppercase tracking-wider mb-3">
          Initialize New Agent
        </p>
        <div className="flex gap-2">
          <input
            type="text"
            value={newAgentName}
            onChange={(e) => setNewAgentName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") handleCreateAgent();
            }}
            placeholder="Agent name..."
            className="flex-1 bg-theme-bg border border-theme-border rounded px-3 py-2 text-theme-primary-dim placeholder:text-theme-text-dim focus:border-theme-primary focus:outline-none text-sm"
            disabled={creating}
          />
          <button
            className="border border-theme-primary-faint text-theme-primary px-4 py-2 rounded hover:bg-theme-primary-glow disabled:opacity-50 disabled:cursor-not-allowed text-sm"
            onClick={handleCreateAgent}
            disabled={creating || !newAgentName.trim()}
          >
            {creating ? "Creating..." : "Create"}
          </button>
        </div>
      </div>
    </div>
  );
}
