import { useEffect, useState } from "react";

export default function GenesisRetention({ days, busy, onSave }: {
  days: number | null; busy: boolean; onSave: (days: number | null) => Promise<unknown>;
}) {
  const [selection, setSelection] = useState(days === null ? "keep" : String(days));
  useEffect(() => { setSelection(days === null ? "keep" : String(days)); }, [days]);
  const proposedDays = selection === "keep" ? null : Number(selection);
  return <details className="genesis-retention"><summary>Creation record retention</summary>
    <p>Choose whether Hive should delete this Genesis record, uploaded sources, transcript, and generated visuals after inactivity. Your awakened Entity and its real chats are kept.</p>
    <label>Keep this creation record<select value={selection} disabled={busy} onChange={(event) => setSelection(event.target.value)}>
      <option value="keep">Until I delete it</option>
      {[7, 30, 90, 365].map((duration) => <option key={duration} value={duration}>{duration} days after the last change</option>)}
      {days !== null && ![7, 30, 90, 365].includes(days) && <option value={days}>{days} days after the last change</option>}
    </select></label>
    <p className="genesis-muted">Changes to this creation restart its inactivity period. Scheduled deletion is permanent.</p>
    <button type="button" disabled={busy || proposedDays === days} onClick={() => void onSave(proposedDays)}>Save retention</button>
  </details>;
}
