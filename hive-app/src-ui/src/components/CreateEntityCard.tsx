import { useRef, useState } from "react";
import { completeEntitySetup, createEntity } from "../lib/daemonClient";

interface CreateEntityCardProps {
  onCreated: () => void;
}

// Creation is resumable: if setup fails after the identity was saved, retry
// that identity instead of creating another Entity with the same name.
export default function CreateEntityCard({ onCreated }: CreateEntityCardProps) {
  const [name, setName] = useState("");
  const [purpose, setPurpose] = useState("");
  const [pendingId, setPendingId] = useState<string | null>(null);
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
      const id = pendingId ?? (await createEntity(trimmed)).id;
      setPendingId(id);
      await completeEntitySetup(id, purpose);
      setName("");
      setPurpose("");
      setPendingId(null);
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
      <label htmlFor="entity-name" className="mb-1 block text-sm text-theme-text">Name</label>
      <input
        id="entity-name"
        value={name}
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") void submit();
        }}
        placeholder="Name (e.g. Ada)"
        maxLength={80}
        disabled={busy || pendingId !== null}
        className="mb-2 w-full rounded-theme-md border border-theme-border bg-theme-input-bg px-3 py-2 text-sm text-theme-text outline-none focus:border-theme-primary"
      />
      <label htmlFor="entity-purpose" className="mb-1 block text-sm text-theme-text">Purpose (optional)</label>
      <input
        id="entity-purpose"
        value={purpose}
        onChange={(e) => setPurpose(e.target.value)}
        placeholder="A study buddy, a family organizer…"
        maxLength={400}
        disabled={busy}
        className="mb-2 w-full rounded-theme-md border border-theme-border bg-theme-input-bg px-3 py-2 text-sm text-theme-text outline-none focus:border-theme-primary"
      />
      <button
        type="button"
        onClick={() => void submit()}
        disabled={!name.trim() || busy}
        className="w-full rounded-theme-md bg-theme-primary px-3 py-2 text-sm font-medium text-white disabled:opacity-40"
      >
        {busy ? "Creating…" : pendingId ? "Finish creating" : "Create Entity"}
      </button>
      {error && <p role="alert" className="mt-2 text-xs text-theme-danger">{error}</p>}
    </div>
  );
}
