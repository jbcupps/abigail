import { invoke } from "@tauri-apps/api/core";
import { useState, useEffect, useCallback } from "react";

type DataTab = "browse" | "search" | "sessions" | "stats";

interface SqliteStats {
  size_bytes: number;
  memory_count: number;
  has_birth: boolean;
}

interface MemoryItem {
  id?: number;
  content: string;
  weight?: string;
  created_at?: string;
}

interface SessionSummary {
  session_id: string;
  turn_count: number;
  first_at: string;
  last_at: string;
}

interface ConversationTurn {
  id?: number;
  session_id: string;
  turn_number: number;
  role: string;
  content: string;
  created_at?: string;
}

interface SearchResult {
  id?: number;
  content: string;
  weight?: string;
  created_at?: string;
}

const WEIGHT_BADGE: Record<string, string> = {
  Ephemeral: "bg-yellow-500/20 text-yellow-400 border-yellow-500/30",
  Distilled: "bg-blue-500/20 text-blue-400 border-blue-500/30",
  Crystallized: "bg-purple-500/20 text-purple-400 border-purple-500/30",
};

export default function DataSourcesPanel() {
  const [tab, setTab] = useState<DataTab>("browse");

  return (
    <div className="flex flex-col h-full">
      <div className="flex border-b border-theme-border-dim px-4 pt-3 gap-1">
        {(["browse", "search", "sessions", "stats"] as DataTab[]).map((t) => (
          <button
            key={t}
            className={`px-3 py-1.5 text-[10px] font-mono rounded-t border-b-2 transition-colors ${
              tab === t
                ? "border-theme-primary text-theme-primary"
                : "border-transparent text-theme-text-dim hover:text-theme-text"
            }`}
            onClick={() => setTab(t)}
          >
            {t}
          </button>
        ))}
      </div>
      <div className="flex-1 overflow-y-auto p-4">
        {tab === "browse" && <BrowseTab />}
        {tab === "search" && <SearchTab />}
        {tab === "sessions" && <SessionsTab />}
        {tab === "stats" && <StatsTab />}
      </div>
    </div>
  );
}

function BrowseTab() {
  const [memories, setMemories] = useState<MemoryItem[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    setLoading(true);
    invoke<MemoryItem[]>("recent_memories", { limit: 50 })
      .then(setMemories)
      .catch((e) => console.error("recent_memories:", e))
      .finally(() => setLoading(false));
  }, []);

  if (loading) return <p className="text-xs text-theme-text-dim animate-pulse">Loading...</p>;
  if (memories.length === 0) return <p className="text-xs text-theme-text-dim">No memories yet.</p>;

  return (
    <div className="flex flex-col gap-1.5">
      {memories.map((m, i) => (
        <div key={m.id ?? i} className="bg-theme-bg-elevated border border-theme-border-dim rounded px-3 py-2">
          <div className="flex items-center gap-2 mb-1">
            {m.weight && (
              <span className={`px-1.5 py-0.5 text-[8px] font-mono rounded border ${WEIGHT_BADGE[m.weight] || "text-theme-text-dim border-theme-border-dim"}`}>
                {m.weight}
              </span>
            )}
            {m.created_at && (
              <span className="text-[9px] text-theme-text-dim font-mono">{m.created_at}</span>
            )}
          </div>
          <p className="text-[10px] text-theme-text font-mono whitespace-pre-wrap break-words">
            {m.content.length > 300 ? m.content.slice(0, 300) + "..." : m.content}
          </p>
        </div>
      ))}
    </div>
  );
}

function SearchTab() {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResult[]>([]);
  const [searching, setSearching] = useState(false);

  const doSearch = useCallback(async () => {
    if (!query.trim()) return;
    setSearching(true);
    try {
      const r = await invoke<SearchResult[]>("search_memories", { query: query.trim(), limit: 30 });
      setResults(r);
    } catch (e) {
      console.error("search_memories:", e);
    } finally {
      setSearching(false);
    }
  }, [query]);

  return (
    <div className="space-y-3">
      <div className="flex gap-2">
        <input
          type="text"
          className="flex-1 bg-theme-input-bg border border-theme-border-dim text-theme-text px-3 py-1.5 rounded text-xs font-mono focus:border-theme-primary focus:outline-none"
          placeholder="Search memories..."
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && doSearch()}
        />
        <button
          className="px-3 py-1.5 text-xs rounded border border-theme-primary text-theme-primary hover:bg-theme-primary-glow font-mono"
          onClick={doSearch}
          disabled={searching}
        >
          {searching ? "..." : "search"}
        </button>
      </div>
      {results.length > 0 && (
        <div className="flex flex-col gap-1.5">
          {results.map((r, i) => (
            <div key={r.id ?? i} className="bg-theme-bg-elevated border border-theme-border-dim rounded px-3 py-2">
              <div className="flex items-center gap-2 mb-1">
                {r.weight && (
                  <span className={`px-1.5 py-0.5 text-[8px] font-mono rounded border ${WEIGHT_BADGE[r.weight] || "text-theme-text-dim border-theme-border-dim"}`}>
                    {r.weight}
                  </span>
                )}
                {r.created_at && (
                  <span className="text-[9px] text-theme-text-dim font-mono">{r.created_at}</span>
                )}
              </div>
              <p className="text-[10px] text-theme-text font-mono whitespace-pre-wrap break-words">
                {r.content.length > 300 ? r.content.slice(0, 300) + "..." : r.content}
              </p>
            </div>
          ))}
        </div>
      )}
      {results.length === 0 && query.trim() && !searching && (
        <p className="text-xs text-theme-text-dim">No results.</p>
      )}
    </div>
  );
}

function SessionsTab() {
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [turns, setTurns] = useState<ConversationTurn[]>([]);
  const [turnsLoading, setTurnsLoading] = useState(false);

  useEffect(() => {
    invoke<SessionSummary[]>("list_sessions", { limit: 30 })
      .then(setSessions)
      .catch((e) => console.error("list_sessions:", e))
      .finally(() => setLoading(false));
  }, []);

  const toggleSession = async (sessionId: string) => {
    if (expanded === sessionId) {
      setExpanded(null);
      setTurns([]);
      return;
    }
    setExpanded(sessionId);
    setTurnsLoading(true);
    try {
      const t = await invoke<ConversationTurn[]>("get_session_turns", { sessionId, limit: 30 });
      setTurns(t);
    } catch (e) {
      console.error("get_session_turns:", e);
    } finally {
      setTurnsLoading(false);
    }
  };

  if (loading) return <p className="text-xs text-theme-text-dim animate-pulse">Loading...</p>;
  if (sessions.length === 0) return <p className="text-xs text-theme-text-dim">No sessions yet.</p>;

  return (
    <div className="flex flex-col gap-1.5">
      {sessions.map((s) => (
        <div key={s.session_id}>
          <button
            className="w-full flex items-center justify-between px-3 py-2 bg-theme-bg-elevated border border-theme-border-dim rounded hover:border-theme-text-dim transition-colors text-left"
            onClick={() => toggleSession(s.session_id)}
          >
            <span className="text-[10px] font-mono text-theme-text truncate">
              {s.session_id.slice(0, 8)}...
            </span>
            <span className="text-[9px] text-theme-text-dim font-mono">
              {s.turn_count} turns
            </span>
            <span className="text-[9px] text-theme-text-dim font-mono">
              {s.last_at}
            </span>
          </button>
          {expanded === s.session_id && (
            <div className="ml-3 mt-1 border-l border-theme-border-dim pl-3 space-y-1">
              {turnsLoading ? (
                <p className="text-[10px] text-theme-text-dim animate-pulse">Loading turns...</p>
              ) : turns.length === 0 ? (
                <p className="text-[10px] text-theme-text-dim">No turns.</p>
              ) : (
                turns.map((t, i) => (
                  <div
                    key={t.id ?? i}
                    className={`px-2 py-1 rounded text-[10px] font-mono ${
                      t.role === "user"
                        ? "bg-theme-primary/5 text-theme-text"
                        : "bg-theme-bg-inset text-theme-text-dim"
                    }`}
                  >
                    <span className="font-bold text-[9px]">{t.role}</span>{" "}
                    {t.content.length > 200 ? t.content.slice(0, 200) + "..." : t.content}
                  </div>
                ))
              )}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}

function StatsTab() {
  const [stats, setStats] = useState<SqliteStats | null>(null);
  const [loading, setLoading] = useState(true);
  const [message, setMessage] = useState<{ type: "success" | "error"; text: string } | null>(null);
  const [actionInProgress, setActionInProgress] = useState<string | null>(null);
  const [confirmReset, setConfirmReset] = useState(false);

  const refreshStats = async () => {
    setLoading(true);
    try {
      const result = await invoke<SqliteStats>("get_sqlite_stats");
      setStats(result);
    } catch (e) {
      setMessage({ type: "error", text: `Failed to load stats: ${e}` });
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => { void refreshStats(); }, []);

  const formatBytes = (b: number) => {
    if (b < 1024) return `${b} B`;
    if (b < 1024 * 1024) return `${(b / 1024).toFixed(1)} KB`;
    return `${(b / (1024 * 1024)).toFixed(1)} MB`;
  };

  const handleOptimize = async () => {
    setActionInProgress("optimize");
    try {
      const freed = await invoke<number>("optimize_sqlite");
      setMessage({ type: "success", text: `Optimized. ${formatBytes(freed)} freed.` });
      await refreshStats();
    } catch (e) {
      setMessage({ type: "error", text: `Optimize failed: ${e}` });
    } finally {
      setActionInProgress(null);
    }
  };

  const handleReset = async () => {
    setActionInProgress("reset");
    try {
      const deleted = await invoke<number>("reset_memories");
      setMessage({ type: "success", text: `Reset complete. ${deleted} memories cleared.` });
      setConfirmReset(false);
      await refreshStats();
    } catch (e) {
      setMessage({ type: "error", text: `Reset failed: ${e}` });
    } finally {
      setActionInProgress(null);
    }
  };

  return (
    <div className="space-y-4 max-w-md">
      {loading ? (
        <p className="text-xs text-theme-text-dim animate-pulse">Loading stats...</p>
      ) : stats ? (
        <div className="grid grid-cols-3 gap-3">
          <div className="bg-theme-bg-elevated border border-theme-border-dim rounded p-3 text-center">
            <p className="text-lg font-bold text-theme-text">{formatBytes(stats.size_bytes)}</p>
            <p className="text-[9px] text-theme-text-dim uppercase">Database</p>
          </div>
          <div className="bg-theme-bg-elevated border border-theme-border-dim rounded p-3 text-center">
            <p className="text-lg font-bold text-theme-text">{stats.memory_count}</p>
            <p className="text-[9px] text-theme-text-dim uppercase">Memories</p>
          </div>
          <div className="bg-theme-bg-elevated border border-theme-border-dim rounded p-3 text-center">
            <p className="text-lg font-bold text-theme-text">{stats.has_birth ? "Yes" : "No"}</p>
            <p className="text-[9px] text-theme-text-dim uppercase">Birth Record</p>
          </div>
        </div>
      ) : null}

      {message && (
        <p className={`text-xs ${message.type === "error" ? "text-theme-danger" : "text-green-400"}`}>
          {message.text}
        </p>
      )}

      <div className="flex gap-2">
        <button
          className="px-3 py-1.5 text-xs rounded border border-theme-primary text-theme-primary hover:bg-theme-primary-glow font-mono"
          onClick={handleOptimize}
          disabled={actionInProgress !== null}
        >
          {actionInProgress === "optimize" ? "Optimizing..." : "Optimize"}
        </button>
        {!confirmReset ? (
          <button
            className="px-3 py-1.5 text-xs rounded border border-theme-danger text-theme-danger hover:bg-theme-danger/20 font-mono"
            onClick={() => setConfirmReset(true)}
            disabled={actionInProgress !== null}
          >
            Reset Memories
          </button>
        ) : (
          <div className="flex gap-1">
            <button
              className="px-3 py-1.5 text-xs rounded bg-theme-danger text-white font-mono"
              onClick={handleReset}
              disabled={actionInProgress !== null}
            >
              {actionInProgress === "reset" ? "Resetting..." : "Confirm Reset"}
            </button>
            <button
              className="px-3 py-1.5 text-xs rounded border border-theme-border-dim text-theme-text-dim font-mono"
              onClick={() => setConfirmReset(false)}
            >
              Cancel
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
