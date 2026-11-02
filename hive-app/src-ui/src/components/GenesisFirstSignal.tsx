import { useCallback, useEffect, useState, type FormEvent } from "react";
import type { GenesisCreationSeed } from "../lib/genesisClient";
import { readGenesisLaunchDraft } from "../lib/genesisLaunchDraft";
import {
  canonicalLegacyPresenceText,
  clearLegacyFirstSignalJournal,
  readLegacyFirstSignalJournal,
  resolveLegacyCandidates,
  type LegacyCandidate,
  type LegacyFirstSignalJournal,
  type LegacyMigrationRequest,
  type LegacyMigrationResult,
} from "../lib/legacyFirstSignal";
import {
  CRESTS,
  getQuestCatalog,
  migrateLegacyFirstSignal,
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

function legacySeed(journal: LegacyFirstSignalJournal): GenesisCreationSeed {
  return {
    mode: "quick",
    name: journal.profile.name,
    purpose: journal.profile.purpose,
    presence: {
      temperament: journal.profile.temperament,
      identity: journal.profile.identity,
      crest: "orbit",
      motion: "calm",
    },
  };
}

function initialSignal(): {
  seed: GenesisCreationSeed;
  recovering: boolean;
  legacy: LegacyFirstSignalJournal | null;
  error: string;
} {
  let legacy: LegacyFirstSignalJournal | null = null;
  let error = "";
  try {
    legacy = readLegacyFirstSignalJournal();
  } catch (reason) {
    error = reason instanceof Error ? reason.message : String(reason);
  }
  try {
    const draft = readGenesisLaunchDraft();
    return {
      seed: draft?.seed
        ?? (legacy
          ? legacySeed(legacy)
          : { mode: "quick", name: "", purpose: "", presence: DEFAULT_PRESENCE }),
      recovering: draft !== null,
      legacy,
      error,
    };
  } catch (reason) {
    return {
      seed: legacy
        ? legacySeed(legacy)
        : { mode: "quick", name: "", purpose: "", presence: DEFAULT_PRESENCE },
      recovering: false,
      legacy,
      error: reason instanceof Error ? reason.message : String(reason),
    };
  }
}

function legacyRequest(journal: LegacyFirstSignalJournal): LegacyMigrationRequest {
  return {
    attempt_key: journal.attempt.attemptId,
    original_name: journal.attempt.name,
    purpose: journal.profile.purpose,
    boundaries: journal.profile.boundaries,
    presence: {
      temperament: journal.profile.temperament,
      identity: journal.profile.identity,
      crest: "orbit",
      motion: "calm",
    },
  };
}

function isMatchingMigration(
  result: LegacyMigrationResult,
  entityId: string,
  request: LegacyMigrationRequest,
): boolean {
  const expectedBadges: LegacyMigrationResult["badges"] = [
    "first_spark",
    "named",
    ...(canonicalLegacyPresenceText(request.purpose)
      ? ["purposeful" as const]
      : []),
    "presence_chosen",
  ];
  return result.entity_id === entityId
    && result.presence.name === request.original_name
    && result.presence.purpose === canonicalLegacyPresenceText(request.purpose)
    && result.presence.boundaries === canonicalLegacyPresenceText(request.boundaries)
    && result.presence.temperament === request.presence.temperament
    && result.presence.identity === request.presence.identity
    && result.presence.crest === "orbit"
    && result.presence.motion === "calm"
    && Array.isArray(result.badges)
    && result.badges.length === expectedBadges.length
    && result.badges.every((badge, index) => badge === expectedBadges[index])
    && typeof result.migrated_at === "string"
    && result.migrated_at.length > 0;
}

export default function GenesisFirstSignal({ onBegin }: {
  onBegin: (seed: GenesisCreationSeed) => Promise<void>;
}) {
  const [initial] = useState(initialSignal);
  const [name, setName] = useState(initial.seed.name);
  const [purpose, setPurpose] = useState(initial.seed.purpose);
  const [presence, setPresence] = useState(initial.seed.presence);
  const [recoverySeed, setRecoverySeed] = useState<GenesisCreationSeed | null>(initial.recovering ? initial.seed : null);
  const [legacy, setLegacy] = useState(initial.legacy);
  const [legacyCandidates, setLegacyCandidates] = useState<LegacyCandidate[] | null>(null);
  const [legacySelection, setLegacySelection] = useState<LegacyCandidate | null>(null);
  const [legacyError, setLegacyError] = useState("");
  const [legacySuccess, setLegacySuccess] = useState("");
  const [catalog, setCatalog] = useState<QuestCatalog | null>(null);
  const [catalogError, setCatalogError] = useState("");
  const [submitError, setSubmitError] = useState("");
  const [busy, setBusy] = useState(false);

  const loadCatalog = useCallback(() => {
    let mounted = true;
    setCatalogError("");
    void getQuestCatalog()
      .then((next) => { if (mounted) setCatalog(next); })
      .catch((reason) => { if (mounted) setCatalogError(reason instanceof Error ? reason.message : String(reason)); });
    return () => { mounted = false; };
  }, []);
  useEffect(() => {
    if (recoverySeed || legacy || initial.error) return;
    return loadCatalog();
  }, [initial.error, legacy, loadCatalog, recoverySeed]);

  const immutable = Boolean(recoverySeed || legacy || initial.error);
  const ready = !legacy && !initial.error && (recoverySeed
    ? true
    : Boolean(name.trim() && purpose.trim() && catalog));
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!ready || busy) return;
    setBusy(true);
    setSubmitError("");
    const submittedSeed: GenesisCreationSeed = recoverySeed ?? {
      mode: "quick",
      name: name.trim(),
      purpose: purpose.trim(),
      presence,
    };
    try {
      await onBegin(submittedSeed);
    } catch (reason) {
      setSubmitError(reason instanceof Error ? reason.message : String(reason));
      try {
        const retained = readGenesisLaunchDraft();
        if (retained) {
          setRecoverySeed(retained.seed);
          setName(retained.seed.name);
          setPurpose(retained.seed.purpose);
          setPresence(retained.seed.presence);
        }
      } catch {
        // The retained journal's validation error is already represented by
        // the launch failure. Never clear or replace it here.
      }
    } finally {
      setBusy(false);
    }
  };

  const checkLegacy = async () => {
    if (!legacy || busy) return;
    setBusy(true);
    setLegacyError("");
    setLegacyCandidates(null);
    setLegacySelection(null);
    try {
      setLegacyCandidates(await resolveLegacyCandidates(legacy));
    } catch (reason) {
      setLegacyError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  };

  const migrateLegacy = async () => {
    if (!legacy || !legacySelection || busy) return;
    setBusy(true);
    setLegacyError("");
    try {
      const request = legacyRequest(legacy);
      const migrated = await migrateLegacyFirstSignal(legacySelection.id, request);
      if (!isMatchingMigration(migrated, legacySelection.id, request)) {
        throw new Error(
          "Hive did not return the selected Entity's canonical legacy migration result. The retained brief was not cleared.",
        );
      }
      await clearLegacyFirstSignalJournal(legacy);
      setLegacy(null);
      setLegacyCandidates(null);
      setLegacySelection(null);
      setName("");
      setPurpose("");
      setPresence(DEFAULT_PRESENCE);
      setLegacySuccess(
        `${migrated.presence.name}'s old First Signal was migrated into canonical Presence and journey progress.`,
      );
    } catch (reason) {
      setLegacyError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  };

  const paletteChoices = catalog?.palettes ?? (immutable ? [{ palette: presence.identity, unlocked_by: null }] : []);
  const crestChoices = catalog?.crests ?? (immutable ? [{ crest: presence.crest, unlocked_by: null }] : []);

  return <form className="first-signal" onSubmit={(event) => void submit(event)}>
    <div className="first-signal-copy">
      <div className="genesis-eyebrow">First Signal</div>
      <h3>Who is waiting to meet your family?</h3>
      <p>Choose a name, a purpose, and the Presence they will carry into your home. You can shape everything else together in the Genesis Quest.</p>
      {legacy && <section className="genesis-notice" aria-label="Legacy First Signal recovery">
        <h4>Recover a retained First Signal</h4>
        <p>This older brief is kept unchanged until you choose an existing Entity and Hive confirms a canonical migration. Abigail will not create or retry an Entity from it.</p>
        <dl>
          <div><dt>Name</dt><dd>{legacy.profile.name}</dd></div>
          <div><dt>Original registered name</dt><dd>{legacy.attempt.name}</dd></div>
          <div><dt>Purpose</dt><dd>{legacy.profile.purpose || "None"}</dd></div>
          <div><dt>Boundaries</dt><dd>{legacy.profile.boundaries || "None"}</dd></div>
          <div><dt>Presence</dt><dd>{TEMPERAMENTS[legacy.profile.temperament]} · {PALETTES[legacy.profile.identity].label} · Orbit · Calm</dd></div>
        </dl>
        {legacy.attempt.creationKey
          ? <p>Check the exact old creation key. Only its completed receipt can be offered; reserved, absent, and unavailable outcomes are never retried.</p>
          : <p>This request had no replay-safe key. A matching name is only a candidate, never proof that this request created the Entity.</p>}
        <button type="button" disabled={busy} onClick={() => void checkLegacy()}>
          {busy && legacyCandidates === null ? "Checking Hive…" : "Check retained First Signal"}
        </button>
        {legacyCandidates && legacyCandidates.length === 0 && <p role="status">No eligible existing Entity was found. The old brief remains retained.</p>}
        {legacyCandidates?.map((candidate) => <div key={candidate.id}>
          <p>
            {candidate.name} <small>{candidate.id}</small>{" "}
            {candidate.resolution === "completed_receipt"
              ? "Exact completed receipt"
              : "Exact-name post-baseline candidate"}
          </p>
          <button
            type="button"
            disabled={busy}
            aria-label={legacyCandidates.length === 1 ? `Review ${candidate.name}` : `Review ${candidate.name} (${candidate.id})`}
            onClick={() => {
              setLegacySelection(candidate);
              setLegacyError("");
            }}
          >
            Review {candidate.name}
          </button>
        </div>)}
        {legacySelection && <div>
          <p>{`Selected existing Entity: ${legacySelection.name} (${legacySelection.id}).`}</p>
          <p>Review the retained brief above. Migration writes its cosmetic Presence and applicable presentation badges only; it does not change tools, models, permissions, or memories.</p>
          <button type="button" disabled={busy} onClick={() => void migrateLegacy()}>
            {busy ? "Migrating retained brief…" : "Migrate this brief"}
          </button>
        </div>}
        {legacyError && <p className="genesis-error" role="alert">{legacyError}</p>}
      </section>}
      {legacySuccess && <p className="genesis-notice" role="status">{legacySuccess}</p>}
      {recoverySeed && !initial.error && <p className="genesis-notice" role="status">A previous First Signal may already have reached Hive. Resume it exactly to recover safely.</p>}
      {initial.error && <section className="genesis-notice" aria-label="Legacy First Signal recovery error">
        <h4>Retained First Signal needs recovery</h4>
        <p className="genesis-error" role="alert">{initial.error} Launch and resume actions are paused until these retained bytes are recovered.</p>
      </section>}
      {catalogError && !immutable && <div className="genesis-error" role="alert">Presence choices could not be loaded: {catalogError}<button type="button" onClick={loadCatalog}>Retry Presence choices</button></div>}
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
      <fieldset><legend>Palette</legend><div className="quest-swatches">{paletteChoices.map(({ palette, unlocked_by }) => {
        const locked = unlocked_by !== null;
        return <button type="button" key={palette} aria-pressed={presence.identity === palette} disabled={immutable || busy || locked} onClick={() => setPresence({ ...presence, identity: palette })}><span style={{ background: `linear-gradient(135deg, ${PALETTES[palette].colors[0]}, ${PALETTES[palette].colors[1]})` }} aria-hidden="true" />{PALETTES[palette].label}{locked && <small>Locked</small>}</button>;
      })}</div></fieldset>
      <fieldset><legend>Crest</legend><div className="quest-swatches">{crestChoices.map(({ crest, unlocked_by }) => {
        const locked = unlocked_by !== null;
        return <button type="button" key={crest} aria-pressed={presence.crest === crest} disabled={immutable || busy || locked} onClick={() => setPresence({ ...presence, crest })}><PresenceCrest palette={presence.identity} crest={crest} size={28} />{CRESTS[crest]}{locked && <small>Locked</small>}</button>;
      })}</div></fieldset>
      <fieldset><legend>Motion</legend><div className="quest-swatches">{Object.entries(MOTIONS).map(([value, label]) => <button type="button" key={value} aria-pressed={presence.motion === value} disabled={immutable || busy} onClick={() => setPresence({ ...presence, motion: value as PresenceChoice["motion"] })}>{label}</button>)}</div></fieldset>
      <button type="submit" className="genesis-primary" disabled={!ready || busy}>{busy ? "Listening for Hive…" : recoverySeed ? "Resume First Signal" : "Begin Genesis Quest"}</button>
      <p className="genesis-muted">Presence is visual and local. It never grants tools, permissions, or access to connected accounts.</p>
    </section>
  </form>;
}
