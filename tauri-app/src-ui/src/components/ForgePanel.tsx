import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState, useCallback } from "react";

type TopicInfo = {
  stream: string;
  topic: string;
  description: string;
};

type JobRecord = {
  id: string;
  topic: string;
  goal: string;
  status: string;
  priority: string;
  capability: string;
  result: string | null;
  error: string | null;
  created_at: string;
  started_at: string | null;
  completed_at: string | null;
};

const TOPIC_COLORS: Record<string, string> = {
  "conversation-turns": "text-cyan-400",
  "job-events": "text-yellow-400",
  "skill-events": "text-green-400",
  "conscience-check": "text-purple-400",
  "ethical-signals": "text-pink-400",
};

const STATUS_DOT: Record<string, string> = {
  queued: "bg-yellow-400",
  running: "bg-blue-400 animate-pulse",
  completed: "bg-green-400",
  failed: "bg-red-400",
  cancelled: "bg-theme-text-dim",
  expired: "bg-theme-text-dim",
};

export default function ForgePanel() {
  const [topics, setTopics] = useState<TopicInfo[]>([]);
  const [recentJobs, setRecentJobs] = useState<JobRecord[]>([]);
  const [loading, setLoading] = useState(true);
  const [visibleTopics, setVisibleTopics] = useState<Set<string>>(new Set());

  const refresh = useCallback(async () => {
    try {
      const [topicData, jobData] = await Promise.all([
        invoke<{ topics: TopicInfo[] }>("get_topic_stats"),
        invoke<JobRecord[]>("list_jobs", { limit: 20 }),
      ]);
      setTopics(topicData.topics);
      setRecentJobs(jobData);
      if (visibleTopics.size === 0) {
        setVisibleTopics(new Set(topicData.topics.map((t) => t.topic)));
      }
    } catch (e) {
      console.error("Nerve center refresh failed:", e);
    } finally {
      setLoading(false);
    }
  }, [visibleTopics.size]);

  useEffect(() => {
    void refresh();
    const interval = setInterval(refresh, 3000);
    return () => clearInterval(interval);
  }, [refresh]);

  const toggleTopic = (topic: string) => {
    setVisibleTopics((prev) => {
      const next = new Set(prev);
      if (next.has(topic)) next.delete(topic);
      else next.add(topic);
      return next;
    });
  };

  const formatTime = (iso: string) => {
    try {
      return new Date(iso).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
    } catch {
      return iso;
    }
  };

  const activeJobs = recentJobs.filter((j) => j.status === "running" || j.status === "queued");
  const terminalJobs = recentJobs.filter((j) => j.status !== "running" && j.status !== "queued");

  return (
    <div className="flex flex-col h-full p-4 gap-4 overflow-y-auto">
      {/* Header */}
      <div>
        <h2 className="text-sm font-semibold text-theme-text tracking-wider uppercase">
          Nerve Center
        </h2>
        <p className="text-[10px] text-theme-text-dim mt-0.5">
          Internal event streams and consumer health.
        </p>
      </div>

      {/* Topic filters */}
      <div className="flex flex-wrap gap-1.5">
        {topics.map((t) => (
          <button
            key={t.topic}
            className={`px-2 py-0.5 text-[9px] rounded border font-mono transition-colors ${
              visibleTopics.has(t.topic)
                ? `border-current ${TOPIC_COLORS[t.topic] || "text-theme-primary"} bg-current/10`
                : "border-theme-border-dim text-theme-text-dim opacity-50"
            }`}
            onClick={() => toggleTopic(t.topic)}
            title={`${t.stream}/${t.topic}: ${t.description}`}
          >
            {t.topic}
          </button>
        ))}
      </div>

      {/* Topic overview */}
      <div className="grid grid-cols-1 gap-1.5">
        {topics
          .filter((t) => visibleTopics.has(t.topic))
          .map((t) => (
            <div
              key={t.topic}
              className="flex items-center gap-2 px-3 py-1.5 bg-theme-bg-elevated border border-theme-border-dim rounded"
            >
              <span className={`w-1.5 h-1.5 rounded-full ${TOPIC_COLORS[t.topic] ? "bg-current" : "bg-theme-primary"} ${TOPIC_COLORS[t.topic] || ""}`} />
              <span className="text-[10px] font-mono text-theme-text flex-1">
                {t.stream}/{t.topic}
              </span>
              <span className="text-[9px] text-theme-text-dim font-mono">
                {t.description}
              </span>
            </div>
          ))}
      </div>

      {/* Active work */}
      {activeJobs.length > 0 && (
        <div>
          <h3 className="text-[10px] font-semibold text-theme-text-dim tracking-wider uppercase mb-1">
            Active
          </h3>
          <div className="flex flex-col gap-1">
            {activeJobs.map((j) => (
              <div
                key={j.id}
                className="flex items-center gap-2 px-3 py-1.5 bg-theme-bg-elevated border border-theme-border-dim rounded"
              >
                <span className={`w-2 h-2 rounded-full ${STATUS_DOT[j.status] || STATUS_DOT.queued}`} />
                <span className="text-[10px] font-mono text-theme-text truncate flex-1">
                  {j.goal.length > 60 ? j.goal.slice(0, 60) + "..." : j.goal}
                </span>
                <span className="text-[9px] text-theme-text-dim font-mono">
                  {j.status}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Recent activity timeline */}
      <div>
        <h3 className="text-[10px] font-semibold text-theme-text-dim tracking-wider uppercase mb-1">
          Recent Activity
        </h3>
        {loading ? (
          <p className="text-xs text-theme-text-dim animate-pulse">Loading...</p>
        ) : terminalJobs.length === 0 && activeJobs.length === 0 ? (
          <p className="text-[10px] text-theme-text-dim">No activity yet.</p>
        ) : (
          <div className="flex flex-col gap-0.5">
            {terminalJobs.slice(0, 15).map((j) => (
              <div
                key={j.id}
                className="flex items-center gap-2 px-2 py-1 text-[9px] font-mono"
              >
                <span className={`w-1.5 h-1.5 rounded-full ${STATUS_DOT[j.status] || STATUS_DOT.queued}`} />
                <span className="text-theme-text-dim w-16 shrink-0">
                  {formatTime(j.completed_at || j.created_at)}
                </span>
                <span className="text-theme-text-dim w-14 shrink-0">{j.status}</span>
                <span className="text-theme-text truncate">
                  {j.goal.length > 50 ? j.goal.slice(0, 50) + "..." : j.goal}
                </span>
                {j.error && (
                  <span className="text-theme-danger truncate ml-auto">
                    {j.error.length > 30 ? j.error.slice(0, 30) + "..." : j.error}
                  </span>
                )}
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Consumers */}
      <div>
        <h3 className="text-[10px] font-semibold text-theme-text-dim tracking-wider uppercase mb-1">
          Consumers
        </h3>
        <div className="grid grid-cols-3 gap-1.5">
          {[
            { name: "MemoryConsumer", topic: "conversation-turns", desc: "Persists chat turns" },
            { name: "ConscienceConsumer", topic: "conscience-check", desc: "Ethical evaluation" },
            { name: "JobScheduler", topic: "job-events", desc: "Task dispatch" },
          ].map((c) => (
            <div
              key={c.name}
              className="px-2 py-1.5 bg-theme-bg-elevated border border-theme-border-dim rounded"
            >
              <div className="flex items-center gap-1">
                <span className="w-1.5 h-1.5 rounded-full bg-green-400" />
                <span className="text-[9px] font-mono text-theme-text">{c.name}</span>
              </div>
              <p className="text-[8px] text-theme-text-dim mt-0.5">{c.desc}</p>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
