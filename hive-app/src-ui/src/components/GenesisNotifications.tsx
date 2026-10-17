import { useEffect, useRef, useState } from "react";
import { genesisRequest } from "../lib/genesisClient";

interface Activity { job_id: string; name: string; kind: string; status: string }
export default function GenesisNotifications({ onOpen }: { onOpen: () => void }) {
  const seen = useRef(new Map<string, string>());
  const [notices, setNotices] = useState<Activity[]>([]);
  useEffect(() => {
    let stopped = false;
    let pending = false;
    let initialized = false;
    const poll = async () => {
      if (pending) return;
      pending = true;
      try {
        const activity = await genesisRequest<Activity[]>("/media/activity");
        if (stopped) return;
        const completed = activity.filter((job) => (seen.current.has(job.job_id) || initialized)
          && seen.current.get(job.job_id) !== job.status
          && ["completed", "failed", "interrupted"].includes(job.status));
        activity.forEach((job) => seen.current.set(job.job_id, job.status));
        initialized = true;
        if (completed.length) setNotices((previous) => [...previous, ...completed].slice(-5));
      } catch { /* Background status must never interrupt the Hive shell. */ }
      finally { pending = false; }
    };
    void poll();
    const timer = setInterval(() => void poll(), 10_000);
    return () => { stopped = true; clearInterval(timer); };
  }, []);
  if (!notices.length) return null;
  return <div role="status" className="mb-4 rounded-theme-lg border border-theme-border p-4">
    {notices.map((job) => <p key={`${job.job_id}-${job.status}`}>
      {job.name || "Your Entity"}’s {job.kind === "portrait" ? "portrait" : "manifestation"} {job.status === "completed" ? "is ready to review." : "needs attention. Your Entity is still available."}
    </p>)}
    <button type="button" className="mr-4 text-theme-primary" onClick={() => { onOpen(); setNotices([]); }}>Open Genesis</button>
    <button type="button" onClick={() => setNotices([])}>Dismiss</button>
  </div>;
}
