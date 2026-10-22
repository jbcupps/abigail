import { useEffect, useState, type FormEvent } from "react";
import type { GenesisCreationSeed } from "../lib/genesisClient";
import { readGenesisLaunchDraft } from "../lib/genesisLaunchDraft";
import {
  CRESTS,
  getQuestCatalog,
  MOTIONS,
  PALETTES,
  TEMPERAMENTS,
  type PresenceChoice,
  type QuestCatalog,
} from "../lib/questClient";
import PresenceCrest from "./PresenceCrest";

const DEFAULT_PRESENCE: PresenceChoice = {
  temperament: "gentle",
  identity: "aether",
  crest: "orbit",
  motion: "calm",
};

function initialSignal(): { seed: GenesisCreationSeed; saved: boolean; error: string } {
  try {
    const draft = readGenesisLaunchDraft();
    return {
      seed: draft?.seed ?? { mode: "quick", name: "", purpose: "", presence: DEFAULT_PRESENCE },
      saved: draft !== null,
      error: "",
    };
  } catch (reason) {
    return {
      seed: { mode: "quick", name: "", purpose: "", presence: DEFAULT_PRESENCE },
      saved: false,
      error: reason instanceof Error ? reason.message : String(reason),
    };
  }
}

export default function GenesisFirstSignal({ onBegin }: {
  onBegin: (seed: GenesisCreationSeed) => Promise<void>;
}) {
  const [initial] = useState(initialSignal);
  const [name, setName] = useState(initial.seed.name);
  const [purpose, setPurpose] = useState(initial.seed.purpose);
  const [presence, setPresence] = useState(initial.seed.presence);
  const [catalog, setCatalog] = useState<QuestCatalog | null>(null);
  const [catalogError, setCatalogError] = useState("");
  const [submitError, setSubmitError] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let mounted = true;
    void getQuestCatalog()
      .then((next) => { if (mounted) setCatalog(next); })
      .catch((reason) => { if (mounted) setCatalogError(reason instanceof Error ? reason.message : String(reason)); });
    return () => { mounted = false; };
  }, []);

  const immutable = initial.saved || Boolean(initial.error);
  const ready = Boolean(name.trim() && purpose.trim() && catalog && !initial.error);
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!ready || busy) return;
    setBusy(true);
    setSubmitError("");
    try {
      await onBegin(initial.saved
        ? initial.seed
        : { mode: "quick", name: name.trim(), purpose: purpose.trim(), presence });
    } catch (reason) {
      setSubmitError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  };

  return <form className="first-signal" onSubmit={(event) => void submit(event)}>
    <div className="first-signal-copy">
      <div className="genesis-eyebrow">First Signal</div>
      <h3>Who is waiting to meet your family?</h3>
      <p>Choose a name, a purpose, and the Presence they will carry into your home. You can shape everything else together in the Genesis Quest.</p>
      {initial.saved && <p className="genesis-notice" role="status">A previous First Signal may already have reached Hive. Resume it exactly to recover safely.</p>}
      {initial.error && <p className="genesis-error" role="alert">{initial.error}</p>}
      {catalogError && <p className="genesis-error" role="alert">Presence choices could not be loaded: {catalogError}</p>}
      {submitError && <p className="genesis-error" role="alert">{submitError}</p>}
      <label>Name<input aria-label="Name" value={name} maxLength={80} disabled={immutable || busy} onChange={(event) => setName(event.target.value)} autoComplete="off" /></label>
      <label>Purpose<textarea aria-label="Purpose" rows={3} maxLength={2000} value={purpose} disabled={immutable || busy} onChange={(event) => setPurpose(event.target.value)} placeholder="Help our family notice small wonders" /></label>
    </div>

    <section className="first-signal-presence" aria-label="Presence">
      <div className="quest-presence-preview">
        <PresenceCrest palette={presence.identity} crest={presence.crest} motion={presence.motion} size={96} label={`${name.trim() || "Entity"} crest preview`} />
        <div><strong>{name.trim() || "A new presence"}</strong><span>{TEMPERAMENTS[presence.temperament]} · {PALETTES[presence.identity].label}</span></div>
      </div>
      <label>Temperament<select value={presence.temperament} disabled={immutable || busy} onChange={(event) => setPresence({ ...presence, temperament: event.target.value as PresenceChoice["temperament"] })}>{Object.entries(TEMPERAMENTS).map(([value, label]) => <option value={value} key={value}>{label}</option>)}</select></label>
      <fieldset><legend>Palette</legend><div className="quest-swatches">{catalog?.palettes.map(({ palette, unlocked_by }) => {
        const locked = unlocked_by !== null;
        return <button type="button" key={palette} aria-pressed={presence.identity === palette} disabled={immutable || busy || locked} onClick={() => setPresence({ ...presence, identity: palette })}><span style={{ background: `linear-gradient(135deg, ${PALETTES[palette].colors[0]}, ${PALETTES[palette].colors[1]})` }} aria-hidden="true" />{PALETTES[palette].label}{locked && <small>Locked</small>}</button>;
      })}</div></fieldset>
      <fieldset><legend>Crest</legend><div className="quest-swatches">{catalog?.crests.map(({ crest, unlocked_by }) => {
        const locked = unlocked_by !== null;
        return <button type="button" key={crest} aria-pressed={presence.crest === crest} disabled={immutable || busy || locked} onClick={() => setPresence({ ...presence, crest })}><PresenceCrest palette={presence.identity} crest={crest} size={28} />{CRESTS[crest]}{locked && <small>Locked</small>}</button>;
      })}</div></fieldset>
      <fieldset><legend>Motion</legend><div className="quest-swatches">{Object.entries(MOTIONS).map(([value, label]) => <button type="button" key={value} aria-pressed={presence.motion === value} disabled={immutable || busy} onClick={() => setPresence({ ...presence, motion: value as PresenceChoice["motion"] })}>{label}</button>)}</div></fieldset>
      <button type="submit" className="genesis-primary" disabled={!ready || busy}>{busy ? "Listening for Hive…" : initial.saved ? "Resume First Signal" : "Begin Genesis Quest"}</button>
      <p className="genesis-muted">Presence is visual and local. It never grants tools, permissions, or access to connected accounts.</p>
    </section>
  </form>;
}
