import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface AgenticEvent {
  type: string;
  task_id: string;
  [key: string]: unknown;
}

interface AgenticRun {
  task_id: string;
  status: string;
  current_turn: number;
  config: { goal: string; max_turns: number };
  events: AgenticEvent[];
}

export default function AgenticPanel() {
  const [goal, setGoal] = useState("");
  const [maxTurns, setMaxTurns] = useState(10);
  const [requireConfirmation, setRequireConfirmation] = useState(false);
  const [activeRun, setActiveRun] = useState<AgenticRun | null>(null);
  const [events, setEvents] = useState<AgenticEvent[]>([]);
  const [mentorInput, setMentorInput] = useState("");
  const eventsEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const unlisten = listen<AgenticEvent>("agentic-event", (event) => {
      setEvents((prev) => [...prev, event.payload]);

      // Update status from events
      const ev = event.payload;
      if (ev.type === "run_completed" || ev.type === "run_failed" || ev.type === "run_cancelled") {
        setActiveRun((prev) => prev ? { ...prev, status: ev.type.replace("run_", "") } : null);
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    eventsEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [events]);

  const startRun = async () => {
    if (!goal.trim()) return;
    try {
      const taskId = await invoke<string>("start_agentic_run", {
        goal: goal.trim(),
        maxTurns,
        requireConfirmation,
      });
      setActiveRun({
        task_id: taskId,
        status: "running",
        current_turn: 0,
        config: { goal: goal.trim(), max_turns: maxTurns },
        events: [],
      });
      setEvents([]);
      setGoal("");
    } catch (e) {
      console.error("Failed to start run:", e);
    }
  };

  const cancelRun = async () => {
    if (!activeRun) return;
    try {
      await invoke("cancel_agentic_run", { taskId: activeRun.task_id });
    } catch (e) {
      console.error("Failed to cancel run:", e);
    }
  };

  const respondToMentor = async () => {
    if (!activeRun || !mentorInput.trim()) return;
    try {
      await invoke("respond_to_mentor_ask", {
        taskId: activeRun.task_id,
        response: mentorInput.trim(),
      });
      setMentorInput("");
    } catch (e) {
      console.error("Failed to respond:", e);
    }
  };

  const confirmTool = async (approved: boolean) => {
    if (!activeRun) return;
    try {
      await invoke("confirm_tool_execution", {
        taskId: activeRun.task_id,
        approved,
      });
    } catch (e) {
      console.error("Failed to confirm:", e);
    }
  };

  const isWaiting = activeRun?.status === "waiting_for_input" || activeRun?.status === "waiting_for_confirmation";
  const isRunning = activeRun?.status === "running";

  return (
    <div className="flex flex-col h-full p-4 gap-4">
      <h2 className="text-lg font-semibold text-white">Agentic Runs</h2>

      {/* Start new run */}
      {!isRunning && !isWaiting && (
        <div className="flex flex-col gap-2 bg-gray-800 rounded-lg p-4">
          <textarea
            className="bg-gray-700 text-white rounded p-2 resize-none"
            rows={3}
            placeholder="Describe the goal for the agent..."
            value={goal}
            onChange={(e) => setGoal(e.target.value)}
          />
          <div className="flex items-center gap-4">
            <label className="text-sm text-gray-400">
              Max turns:
              <input
                type="number"
                className="ml-2 bg-gray-700 text-white rounded px-2 py-1 w-16"
                value={maxTurns}
                onChange={(e) => setMaxTurns(Number(e.target.value))}
                min={1}
                max={100}
              />
            </label>
            <label className="text-sm text-gray-400 flex items-center gap-1">
              <input
                type="checkbox"
                checked={requireConfirmation}
                onChange={(e) => setRequireConfirmation(e.target.checked)}
              />
              Require tool confirmation
            </label>
            <button
              className="ml-auto bg-blue-600 hover:bg-blue-700 text-white px-4 py-2 rounded disabled:opacity-50"
              onClick={startRun}
              disabled={!goal.trim()}
            >
              Start Run
            </button>
          </div>
        </div>
      )}

      {/* Active run controls */}
      {(isRunning || isWaiting) && (
        <div className="flex items-center gap-2 bg-gray-800 rounded-lg p-3">
          <div className="flex-1">
            <span className="text-sm text-gray-400">
              Running: {activeRun?.config.goal}
            </span>
            <span className="ml-2 text-xs text-yellow-400">
              Turn {activeRun?.current_turn}/{activeRun?.config.max_turns}
            </span>
          </div>
          <button
            className="bg-red-600 hover:bg-red-700 text-white px-3 py-1 rounded text-sm"
            onClick={cancelRun}
          >
            Cancel
          </button>
        </div>
      )}

      {/* Mentor ask / Tool confirmation */}
      {activeRun?.status === "waiting_for_input" && (
        <div className="bg-yellow-900/30 border border-yellow-600 rounded-lg p-3 flex gap-2">
          <input
            className="flex-1 bg-gray-700 text-white rounded px-3 py-2"
            placeholder="Respond to agent question..."
            value={mentorInput}
            onChange={(e) => setMentorInput(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && respondToMentor()}
          />
          <button
            className="bg-yellow-600 hover:bg-yellow-700 text-white px-4 py-2 rounded"
            onClick={respondToMentor}
          >
            Send
          </button>
        </div>
      )}

      {activeRun?.status === "waiting_for_confirmation" && (
        <div className="bg-orange-900/30 border border-orange-600 rounded-lg p-3 flex items-center gap-3">
          <span className="text-sm text-orange-300 flex-1">
            Agent wants to execute a tool. Approve?
          </span>
          <button
            className="bg-green-600 hover:bg-green-700 text-white px-4 py-2 rounded"
            onClick={() => confirmTool(true)}
          >
            Approve
          </button>
          <button
            className="bg-red-600 hover:bg-red-700 text-white px-3 py-2 rounded"
            onClick={() => confirmTool(false)}
          >
            Deny
          </button>
        </div>
      )}

      {/* Event timeline */}
      <div className="flex-1 overflow-y-auto bg-gray-900 rounded-lg p-3 space-y-2">
        {events.length === 0 && (
          <p className="text-gray-500 text-sm text-center py-8">
            No events yet. Start an agentic run above.
          </p>
        )}
        {events.map((ev, i) => (
          <div key={i} className="text-sm">
            {ev.type === "run_started" && (
              <div className="text-green-400">Started: {(ev as any).goal}</div>
            )}
            {ev.type === "turn_started" && (
              <div className="text-blue-400">Turn {(ev as any).turn_number}</div>
            )}
            {ev.type === "llm_response" && (
              <div className="text-gray-300 pl-4 border-l border-gray-700">
                {(ev as any).content?.substring(0, 200)}
                {((ev as any).content?.length || 0) > 200 && "..."}
              </div>
            )}
            {ev.type === "tool_executed" && (
              <div className="text-purple-400 pl-4">
                Tool: {(ev as any).tool_name} → {(ev as any).result?.substring(0, 100)}
              </div>
            )}
            {ev.type === "mentor_ask" && (
              <div className="text-yellow-400">Question: {(ev as any).question}</div>
            )}
            {ev.type === "tool_confirmation" && (
              <div className="text-orange-400">
                Confirm tool: {(ev as any).tool_name}
              </div>
            )}
            {ev.type === "run_completed" && (
              <div className="text-green-400 font-semibold">
                Completed: {(ev as any).summary?.substring(0, 200)}
              </div>
            )}
            {ev.type === "run_failed" && (
              <div className="text-red-400 font-semibold">
                Failed: {(ev as any).error}
              </div>
            )}
            {ev.type === "run_cancelled" && (
              <div className="text-gray-400">Run cancelled</div>
            )}
          </div>
        ))}
        <div ref={eventsEndRef} />
      </div>
    </div>
  );
}
