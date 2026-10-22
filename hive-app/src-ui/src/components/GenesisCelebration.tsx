import { useEffect, useRef, useState } from "react";
import { getEntityJourney, type BadgeView, type EntityJourney, type PresenceCard } from "../lib/questClient";
import PresenceCrest from "./PresenceCrest";

/** Shown once, right after a first awakening. Purely celebratory. */
export default function GenesisCelebration({ entityId, name, presence, newBadges, onMeet, onClose }: {
  entityId: string; name: string; presence: PresenceCard | null; newBadges: BadgeView[]; onMeet: () => void; onClose: () => void;
}) {
  const [journey, setJourney] = useState<EntityJourney | null>(null);
  const meet = useRef<HTMLButtonElement>(null);
  const close = useRef(onClose);
  close.current = onClose;
  useEffect(() => {
    meet.current?.focus();
    let current = true;
    void getEntityJourney(entityId).then((next) => { if (current) setJourney(next); }).catch(() => undefined);
    const escape = (event: KeyboardEvent) => { if (event.key === "Escape") close.current(); };
    window.addEventListener("keydown", escape);
    return () => { current = false; window.removeEventListener("keydown", escape); };
  }, [entityId]);
  const card = journey?.presence ?? presence;
  const certificate = journey?.certificate;
  return <div className="genesis-celebration" role="dialog" aria-modal="true" aria-labelledby="genesis-celebration-title">
    <div className="genesis-celebration-card">
      <div className="genesis-celebration-glow" aria-hidden="true">{Array.from({ length: 12 }, (_, index) => <span key={index} style={{ "--spark": index } as React.CSSProperties} />)}</div>
      <PresenceCrest palette={card?.identity ?? "aether"} crest={card?.crest ?? "orbit"} motion={card?.motion ?? "calm"} size={112} label={`${name}'s crest`} />
      <div className="genesis-eyebrow">First Light</div>
      <h2 id="genesis-celebration-title">{name} has awakened</h2>
      {certificate ? <p className="genesis-celebration-soul"><strong>{certificate.archetype}</strong> · {certificate.epithet}<span>{certificate.birth_path === "forge" ? "Forged from your trial answers" : "A balanced soul"} · {certificate.scoring_version || "Legacy scoring"} · {certificate.signature_verified ? "Signed by your Hive" : "Signature could not be verified"}</span></p> : <p className="genesis-muted">Reading the birth certificate…</p>}
      {newBadges.length > 0 && <ul className="genesis-celebration-badges" aria-label="Badges earned">{newBadges.map((badge) => <li key={badge.badge}>✦ {badge.title} <small>+{badge.xp} XP</small></li>)}</ul>}
      {journey && <p className="genesis-muted">Level {journey.level.level} · {journey.level.title} · {journey.level.xp} XP</p>}
      <div className="genesis-actions"><button ref={meet} type="button" className="genesis-primary" onClick={onMeet}>Meet {name}</button><button type="button" onClick={onClose}>Stay in Genesis</button></div>
    </div>
  </div>;
}
