import { useCallback, useEffect, useRef, useState } from "react";
import { type FieldUpdate, type GenesisSession } from "../lib/genesisClient";
import { actOnGenesisJob, approveGenesisAsset, deleteGenesisAsset, genesisAssetBlob, getGenesisMedia, getGenesisMediaCapabilities, requestGenesisMedia, type GenesisAsset, type GenesisMedia, type MediaCapabilities } from "../lib/genesisMedia";

const STYLES = ["High fantasy", "Cyberpunk", "Anime", "Cinematic realism", "Watercolor", "Oil painting", "Graphic novel", "Gothic", "Pixel art", "Abstract", "Celestial"];
const MANIFESTATIONS = ["Celestial emergence", "Shadow awakening", "Elemental formation", "Digital assembly", "Ancient summoning", "Custom direction"];
const PORTRAIT_CANDIDATES = 2;

function PrivateAsset({ sessionId, asset, playback, onFinished }: { sessionId: string; asset: GenesisAsset; playback?: boolean; onFinished?: () => void }) {
  const [url, setUrl] = useState<string | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    setUrl(null);
    setError("");
    let disposed = false;
    let objectUrl: string | null = null;
    void genesisAssetBlob(sessionId, asset.asset_id).then((blob) => {
      if (disposed) return;
      objectUrl = URL.createObjectURL(blob);
      setUrl(objectUrl);
    }).catch((reason) => { if (!disposed) { setError(String(reason)); onFinished?.(); } });
    return () => { disposed = true; if (objectUrl) URL.revokeObjectURL(objectUrl); };
  }, [sessionId, asset.asset_id, onFinished]);
  if (error) return <p role="status">{error}</p>;
  if (!url) return <p role="status">Loading private visual…</p>;
  return asset.kind === "portrait" ? <img src={url} alt={`Entity portrait, version ${asset.version}${asset.approved ? ", approved" : ", candidate"}`} /> : <video controls preload="metadata" autoPlay={playback} muted={playback} playsInline onEnded={onFinished} onError={onFinished} src={url} style={playback ? { width: "100%", height: "100%", objectFit: "contain" } : undefined} aria-label={`Manifestation film, version ${asset.version}`} />;
}

function ManifestationPreview({ sessionId, portrait, film, motion }: { sessionId: string; portrait: GenesisAsset; film?: GenesisAsset; motion: boolean }) {
  const [settled, setSettled] = useState(false);
  const finish = useCallback(() => setSettled(true), []);
  return <div style={{ position: "relative", width: "100%", height: "100%" }}>
    <PrivateAsset sessionId={sessionId} asset={portrait} />
    {film && motion && <div aria-hidden={settled} style={{ position: "absolute", inset: 0, opacity: settled ? 0 : 1, transition: "opacity 700ms ease, visibility 0s linear 700ms", pointerEvents: settled ? "none" : "auto", visibility: settled ? "hidden" : "visible" }}>
      <PrivateAsset sessionId={sessionId} asset={film} playback onFinished={finish} />
    </div>}
  </div>;
}

export default function GenesisStudio({ session, busy, onUpdate, onRefresh }: {
  session: GenesisSession; busy: boolean; onUpdate: (updates: FieldUpdate[]) => Promise<void>; onRefresh: () => Promise<void>;
}) {
  const [media, setMedia] = useState<GenesisMedia>({ jobs: [], assets: [] });
  const [capabilities, setCapabilities] = useState<MediaCapabilities | null>(null);
  const [error, setError] = useState("");
  const [working, setWorking] = useState(false);
  const [costConsent, setCostConsent] = useState(false);
  const [preset, setPreset] = useState(MANIFESTATIONS[0]);
  const [direction, setDirection] = useState("");
  const [seconds, setSeconds] = useState(5);
  const [replay, setReplay] = useState(0);
  const [motion, setMotion] = useState(() => !window.matchMedia?.("(prefers-reduced-motion: reduce)").matches);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const operation = useRef(false);
  const mounted = useRef(true);
  const refresh = useCallback(async () => {
    const result = await getGenesisMedia(session.id);
    if (mounted.current) setMedia(result);
  }, [session.id]);
  useEffect(() => {
    mounted.current = true;
    let cancelled = false;
    setCostConsent(false);
    setCapabilities(null);
    setMedia({ jobs: [], assets: [] });
    void Promise.all([getGenesisMediaCapabilities(), getGenesisMedia(session.id)]).then(([caps, next]) => {
      if (!cancelled) {
        setCapabilities(caps); setMedia(next);
        setSeconds((selected) => caps.video_seconds.includes(selected) ? selected : (caps.video_seconds[0] ?? 5));
      }
    }).catch((reason) => { if (!cancelled) setError(reason instanceof Error ? reason.message : String(reason)); });
    return () => { cancelled = true; mounted.current = false; };
  }, [session.id]);
  const running = media.jobs.some((job) => ["queued", "submitting", "running"].includes(job.status));
  useEffect(() => {
    if (!running) return;
    let pending = false;
    const timer = setInterval(() => {
      if (pending || operation.current) return;
      pending = true;
      void refresh().catch((reason) => setError(String(reason))).finally(() => { pending = false; });
    }, 4000);
    return () => clearInterval(timer);
  }, [running, refresh]);

  const run = async (action: () => Promise<unknown>) => {
    if (operation.current) return;
    operation.current = true; setWorking(true); setError("");
    try { await action(); await refresh(); await onRefresh(); }
    catch (reason) { if (mounted.current) setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { operation.current = false; if (mounted.current) setWorking(false); }
  };
  const portrait = media.assets.find((asset) => asset.asset_id === session.definition.portrait_asset_id && asset.kind === "portrait" && asset.approved);
  const film = media.assets.find((asset) => asset.asset_id === session.definition.manifestation_asset_id && asset.kind === "manifestation" && asset.approved && asset.reference_asset_id === portrait?.asset_id);
  const request = (kind: "portrait" | "manifestation") => run(async () => {
    await requestGenesisMedia(session.id, { kind, idempotency_key: crypto.randomUUID(), cost_confirmed: costConsent, candidates: PORTRAIT_CANDIDATES, seconds, ...(kind === "manifestation" ? { reference_asset_id: portrait?.asset_id } : {}), preset, direction });
    setCostConsent(false);
  });

  return <aside className="genesis-panel genesis-studio" aria-label="Visual identity studio">
    <div className="genesis-eyebrow">A presence, taking shape</div>
    <h3>Visual identity</h3>
    <div key={replay} className={`genesis-manifestation ${motion ? "is-animated" : "is-still"}`} data-preset={preset}>
      {portrait ? <ManifestationPreview key={`${portrait.asset_id}:${film?.asset_id ?? "ambient"}:${motion}`} sessionId={session.id} portrait={portrait} film={film} motion={motion} /> : <><div className="genesis-orbit orbit-one" /><div className="genesis-orbit orbit-two" /><div className="genesis-core" /><span>{session.definition.name || "Your Entity"}</span></>}
    </div>
    <p className="genesis-muted">{portrait ? "Your approved visual reference." : "An ambient preview. Your portrait can be added whenever you are ready."}</p>
    <div className="genesis-actions"><button type="button" onClick={() => { setMotion(true); setReplay((previous) => previous + 1); }}>Replay emergence</button><button type="button" onClick={() => setMotion(false)}>Skip animation</button></div>
    <label className="genesis-check"><input type="checkbox" checked={motion} onChange={(event) => setMotion(event.target.checked)} />Ambient motion</label>
    <label>Artistic style<select value={STYLES.includes(session.definition.visual_style) ? session.definition.visual_style : "Custom"} disabled={busy || working} onChange={(event) => { if (event.target.value !== "Custom") void onUpdate([{ field: "visual_style", value: event.target.value }]).catch(() => {}); }}>{STYLES.map((style) => <option key={style}>{style}</option>)}<option>Custom</option></select></label>
    <p className="genesis-muted">Custom style, clothing, expression, environment, and palette can be edited in Definition.</p>
    <label>Manifestation style<select value={preset} onChange={(event) => setPreset(event.target.value)}>{MANIFESTATIONS.map((style) => <option key={style}>{style}</option>)}</select></label>
    <label>Film direction<textarea rows={2} value={direction} maxLength={600} onChange={(event) => setDirection(event.target.value)} placeholder="Warm light and floating petals…" /></label>
    <p className="genesis-muted">The approved portrait is the reference for your film. You review the result for consistency before approving it.</p>
    {capabilities && <>
      <p className="genesis-muted">{capabilities.notice}</p>
      {capabilities.video_available && <label>Film duration<select value={seconds} onChange={(event) => setSeconds(Number(event.target.value))}>{capabilities.video_seconds.map((duration) => <option key={duration} value={duration}>{duration} seconds</option>)}</select></label>}
      <p className="genesis-muted">Budget reservation: {PORTRAIT_CANDIDATES} portrait candidates ${(capabilities.portrait_reservation_usd * PORTRAIT_CANDIDATES).toFixed(2)} total, film ${capabilities.video_reservation_usd.toFixed(2)}. Daily budget ${capabilities.daily_budget_usd.toFixed(2)}, up to {capabilities.daily_job_limit} jobs. Reservations are estimates, not a provider invoice.</p>
      <label className="genesis-check"><input type="checkbox" checked={costConsent} onChange={(event) => setCostConsent(event.target.checked)} />I approve the displayed generation budget for this request.</label>
    </>}
    <div className="genesis-actions">
      <button type="button" disabled={!capabilities?.portrait_available || !costConsent || busy || working} onClick={() => void request("portrait")}>Generate portrait candidates</button>
      <button type="button" disabled={!capabilities?.video_available || !capabilities.video_seconds.includes(seconds) || !portrait || !costConsent || busy || working} onClick={() => void request("manifestation")}>Generate manifestation</button>
    </div>
    {!capabilities?.portrait_available && <p className="genesis-muted">Portrait generation is not configured in Hive. You can awaken your Entity with the ambient preview.</p>}
    {!capabilities?.video_available && <p className="genesis-muted">Film generation is unavailable. The animated emergence works without a video provider.</p>}
    {error && <p role="alert" className="genesis-error">{error}</p>}
    <div aria-live="polite">{media.jobs.map((job) => <div className="genesis-job" key={job.job_id}>
      <strong>{job.kind === "portrait" ? "Portrait" : "Manifestation"}</strong> · {job.status}
      {["queued", "submitting", "running"].includes(job.status) && <><progress max={100} value={job.progress} aria-label={`${job.kind} generation progress`} /><button type="button" disabled={working} onClick={() => void run(() => actOnGenesisJob(session.id, job.job_id, "cancel"))}>Cancel generation</button></>}
      {job.error && <p>{job.error}</p>}
      {["failed", "cancelled", "interrupted"].includes(job.status) && <button type="button" disabled={working || !costConsent} onClick={() => void run(async () => { await actOnGenesisJob(session.id, job.job_id, "retry"); setCostConsent(false); })}>Retry generation</button>}
    </div>)}</div>
    {running && <p role="status" className="genesis-muted">Rendering continues in Hive. Save and exit safely; return here to check progress.</p>}
    {media.assets.length > 0 && <details open><summary>Visual versions ({media.assets.length})</summary><div className="genesis-asset-list">{media.assets.map((asset) => <article key={asset.asset_id}>
      <PrivateAsset sessionId={session.id} asset={asset} /><p>{asset.kind === "portrait" ? "Portrait" : "Film"} · version {asset.version}{asset.approved ? " · Approved" : " · Candidate"}</p>
      {asset.identity_review_required && <p>Confirm that this film depicts the same Entity as your approved portrait.</p>}
      <div className="genesis-actions"><button type="button" disabled={working || asset.asset_id === session.definition.portrait_asset_id || asset.asset_id === session.definition.manifestation_asset_id} onClick={() => void run(() => approveGenesisAsset(session.id, asset.asset_id))}>{asset.kind === "manifestation" ? "Approve matching film" : "Use this portrait"}</button><button type="button" disabled={working} onClick={() => setConfirmDelete(asset.asset_id)}>Delete visual</button></div>
      {confirmDelete === asset.asset_id && <div className="genesis-confirm"><p>Delete this saved visual? This cannot be undone.</p><button type="button" disabled={working} onClick={() => void run(async () => { await deleteGenesisAsset(session.id, asset.asset_id); setConfirmDelete(null); })}>Confirm deletion</button><button type="button" onClick={() => setConfirmDelete(null)}>Keep visual</button></div>}
    </article>)}</div></details>}
  </aside>;
}
