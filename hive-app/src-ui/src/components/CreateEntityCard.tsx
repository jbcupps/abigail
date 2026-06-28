import { useRef, useState } from "react";
import { createEntity } from "../lib/daemonClient";

interface CreateEntityCardProps {
  onCreated: () => void;
}

// Inline "new Entity" card for the Hive home. A family head names a new Entity
// and it appears in the grid; everything else (model, persona) is handled for
// them — there is nothing else to configure.
export default function CreateEntityCard({ onCreated }: CreateEntityCardProps) {
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inFlight = useRef(false);

  const submit = async () => {
    const trimmed = name.trim();
    // Synchronous guard: `busy` state lags, so mashing Enter could double-submit.
    if (!trimmed || inFlight.current) return;
    inFlight.current = true;
    setBusy(true);
    setError(null);
    try {
      await createEntity(trimmed);
      setName("");
      onCreated();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
      inFlight.current = false;
    }
  };

  return (
    <div className="rounded-theme-lg border border-dashed border-theme-border bg-theme-surface-dim p-4">
      <div className="mb-2 text-xs uppercase tracking-wide text-theme-text-dim">New Entity</div>
      <input
        value={name}
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") void submit();
        }}
        placeholder="Name (e.g. Ada)"
        disabled={busy}
        className="mb-2 w-full rounded-theme-md border border-theme-border bg-theme-input-bg px-3 py-2 text-sm text-theme-text outline-none focus:border-theme-primary"
      />
      <button
        type="button"
        onClick={() => void submit()}
        disabled={!name.trim() || busy}
        className="w-full rounded-theme-md bg-theme-primary px-3 py-2 text-sm font-medium text-white disabled:opacity-40"
      >
        {busy ? "Creating…" : "Create"}
      </button>
      {error && <p className="mt-2 text-xs text-theme-danger">{error}</p>}
    </div>
  );
}
