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
      <div className="min-h-screen bg-black text-gray-500 font-mono flex items-center justify-center">
        Loading identities...
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-black text-gray-300 font-mono flex flex-col items-center justify-center p-8">
      {/* Header */}
      <div className="text-center mb-8">
        <pre className="text-cyan-400 text-xs mb-2">
{`  ▄▄▄       ▒█████
 ▒████▄    ▒██▒  ██▒
 ▒██  ▀█▄  ▒██░  ██▒
 ░██▄▄▄▄██ ▒██   ██░
  ▓█   ▓██▒░ ████▓▒░
  ▒▒   ▓▒█░░ ▒░▒░▒░ `}
        </pre>
        <h1 className="text-xl text-cyan-300 font-bold">AO HIVE</h1>
        <p className="text-gray-500 text-sm mt-1">Identity Management</p>
      </div>

      {error && (
        <div className="w-full max-w-lg mb-4 p-3 border border-red-800 rounded bg-red-900/20 text-red-400 text-sm">
          {error}
          <button
            className="ml-2 text-red-300 underline"
            onClick={() => setError(null)}
          >
            dismiss
          </button>
        </div>
      )}

      {/* Agent list */}
      {agents.length > 0 ? (
        <div className="w-full max-w-lg space-y-3 mb-8">
          <p className="text-gray-500 text-xs uppercase tracking-wider mb-2">
            Select an Identity
          </p>
          {agents.map((agent) => (
            <button
              key={agent.id}
              className="w-full text-left p-4 border border-gray-700 rounded hover:border-cyan-500 hover:bg-cyan-900/10 transition-colors group"
              onClick={() => handleSelectAgent(agent.id)}
            >
              <div className="flex items-center justify-between">
                <div>
                  <div className="text-cyan-300 font-bold group-hover:text-cyan-200">
                    {agent.name}
                  </div>
                  <div className="text-gray-600 text-xs mt-1">
                    {agent.id.substring(0, 8)}...
                    {agent.birth_complete ? (
                      <span className="ml-2 text-green-600">born</span>
                    ) : (
                      <span className="ml-2 text-yellow-600">unborn</span>
                    )}
                    {agent.birth_date && (
                      <span className="ml-2">{agent.birth_date}</span>
                    )}
                  </div>
                </div>
                <div className="text-gray-600 group-hover:text-cyan-400">
                  &rarr;
                </div>
              </div>
            </button>
          ))}
        </div>
      ) : (
        <div className="w-full max-w-lg text-center mb-8 p-6 border border-gray-800 rounded">
          <p className="text-gray-500 mb-4">No agents found in this Hive.</p>
          <button
            className="text-cyan-400 underline text-sm"
            onClick={handleMigrateLegacy}
            disabled={migrating}
          >
            {migrating ? "Checking..." : "Detect & migrate legacy identity"}
          </button>
        </div>
      )}

      {/* Create new agent */}
      <div className="w-full max-w-lg border border-gray-800 rounded p-4">
        <p className="text-gray-500 text-xs uppercase tracking-wider mb-3">
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
            className="flex-1 bg-black border border-gray-700 rounded px-3 py-2 text-cyan-300 placeholder-gray-600 focus:border-cyan-500 focus:outline-none text-sm"
            disabled={creating}
          />
          <button
            className="border border-cyan-700 text-cyan-400 px-4 py-2 rounded hover:bg-cyan-900/20 disabled:opacity-50 disabled:cursor-not-allowed text-sm"
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
