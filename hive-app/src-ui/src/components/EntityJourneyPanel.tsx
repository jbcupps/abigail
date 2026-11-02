import { useEffect, useRef, useState } from "react";
import {
  getEntityJourney,
  getQuestCatalog,
  type EntityJourney,
  type QuestCatalog,
} from "../lib/questClient";
import PresenceCrest from "./PresenceCrest";
import { BadgeShelf, JourneyProgressBody, QuestLevel } from "./JourneyViews";

/** Post-creation mentoring arc for a born Entity. Copy-only; never sends or executes. */
export default function EntityJourneyPanel({
  entityId,
  entityName,
  onClose,
}: {
  entityId: string;
  entityName: string;
  onClose: () => void;
}) {
  const [catalog, setCatalog] = useState<QuestCatalog | null>(null);
  const [journey, setJourney] = useState<EntityJourney | null>(null);
  const [error, setError] = useState("");
  const closeButton = useRef<HTMLButtonElement>(null);
  const close = useRef(onClose);
  close.current = onClose;

  useEffect(() => {
    closeButton.current?.focus();
    let current = true;
    setError("");
    void Promise.all([getQuestCatalog(), getEntityJourney(entityId)])
      .then(([nextCatalog, nextJourney]) => {
        if (!current) return;
        setCatalog(nextCatalog);
        setJourney(nextJourney);
      })
      .catch((reason) => {
        if (!current) return;
        setError(reason instanceof Error ? reason.message : String(reason));
      });
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") close.current();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      current = false;
      window.removeEventListener("keydown", onKey);
    };
  }, [entityId]);

  const title = `${journey?.name ?? entityName}'s Journey`;
  return <div className="entity-journey-panel" role="dialog" aria-modal="true" aria-labelledby="entity-journey-title">
    <div className="entity-journey-card">
      <header className="entity-journey-head">
        <div className="entity-journey-identity">
          <PresenceCrest
            palette={journey?.presence?.identity ?? "aether"}
            crest={journey?.presence?.crest ?? "orbit"}
            motion={journey?.presence?.motion ?? "calm"}
            size={48}
            label={`${journey?.name ?? entityName} crest`}
          />
          <div>
            <div className="genesis-eyebrow">Mentoring journey</div>
            <h2 id="entity-journey-title">{title}</h2>
          </div>
        </div>
        <button ref={closeButton} type="button" onClick={onClose}>Close</button>
      </header>
      {error ? <p role="alert" className="genesis-muted">{error}</p>
        : !catalog || !journey ? <p className="genesis-muted" role="status">Reading their journey…</p>
          : <>
            <QuestLevel level={journey.level} />
            <JourneyProgressBody journey={journey} catalog={catalog} />
            <p className="genesis-muted">Nothing is sent for you. Copy a prompt, or say whatever feels right.</p>
            <BadgeShelf badges={journey.badges} />
          </>}
    </div>
  </div>;
}
