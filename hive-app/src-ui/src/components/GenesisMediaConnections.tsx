import { useState } from "react";
import { genesisRequest } from "../lib/genesisClient";

export default function GenesisMediaConnections() {
  const [open, setOpen] = useState(false);
  const [provider, setProvider] = useState("openai");
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const [error, setError] = useState("");
  return <section className="mb-4 rounded-theme-lg border border-theme-border bg-theme-surface p-4">
    <button type="button" aria-expanded={open} onClick={() => { setOpen(!open); setKey(""); }} className="text-sm font-medium text-theme-text-bright">Optional portrait and film connections</button>
    {open && <form className="mt-3 grid gap-3" onSubmit={async (event) => {
      event.preventDefault();
      if (busy || !key.trim()) return;
      setBusy(true); setNotice(""); setError("");
      try {
        await genesisRequest("/media/credentials", { provider, key: key.trim() }); setKey("");
        setNotice("Credential saved in Hive. Reopen Genesis to refresh availability. Provider access is checked when you confirm a generation request.");
      } catch { setError("The credential could not be saved. Check that Hive is running and try again."); }
      finally { setBusy(false); }
    }}>
      <p className="text-sm text-theme-text-dim">Visual generation sends your approved appearance brief and, for films, your portrait to the chosen provider. It uses separate API billing. Each generation asks you to approve its budget.</p>
      <label className="text-sm">Visual provider<select className="ml-3 rounded-theme-md border border-theme-border bg-theme-bg p-2" value={provider} disabled={busy} onChange={(event) => { setProvider(event.target.value); setKey(""); setNotice(""); }}><option value="openai">OpenAI portraits</option><option value="runway">Runway manifestation films</option></select></label>
      <label className="text-sm">API key<input className="ml-3 rounded-theme-md border border-theme-border bg-theme-bg p-2" type="password" autoComplete="off" value={key} disabled={busy} onChange={(event) => setKey(event.target.value)} placeholder="Paste your provider API key" /></label>
      <p className="text-xs text-theme-text-dim">Saving an OpenAI key updates the shared OpenAI credential in Hive. Your selected chat model stays the same.</p>
      <button className="justify-self-start rounded-theme-md bg-theme-primary px-3 py-2 text-sm text-white disabled:opacity-40" type="submit" disabled={busy || !key.trim()}>{busy ? "Saving…" : "Save media credential"}</button>
      {notice && <p role="status" className="text-sm">{notice}</p>}
      {error && <p role="alert" className="text-sm text-theme-danger">{error}</p>}
    </form>}
  </section>;
}
