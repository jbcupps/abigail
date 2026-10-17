import { useEffect, useState } from "react";
import { listGenesisSessions, type GenesisSession } from "../lib/genesisClient";
import { genesisAssetBlob } from "../lib/genesisMedia";

export interface EntityPortraitSource { sessionId: string; assetId: string }
export type EntityPortraits = Record<string, EntityPortraitSource>;

/** One household list request supplies every Entity card. Unapproved candidates
 * never appear here; only the server-managed canonical portrait pointer does. */
export function portraitsFromSessions(sessions: GenesisSession[]): EntityPortraits {
  const portraits: EntityPortraits = {};
  const seen = new Set<string>();
  const newest = [...sessions].sort((a, b) => b.updated_at.localeCompare(a.updated_at));
  for (const session of newest) {
    const entityId = session.definition.entity_id;
    const assetId = session.definition.portrait_asset_id;
    const awakened = session.state === "ready" || Boolean((session as GenesisSession & { awakening?: string }).awakening);
    if (entityId && awakened && !seen.has(entityId)) {
      seen.add(entityId);
      if (assetId) portraits[entityId] = { sessionId: session.id, assetId };
    }
  }
  return portraits;
}

/** Call once in the collection parent; change refreshKey after leaving Genesis
 * or switching the Hive connection to refresh every card together. */
export function useGenesisEntityPortraits(refreshKey?: unknown): EntityPortraits {
  const [portraits, setPortraits] = useState<EntityPortraits>({});
  useEffect(() => {
    let current = true;
    void listGenesisSessions().then((sessions) => {
      if (current) setPortraits(portraitsFromSessions(sessions));
    }).catch(() => {
      // Existing Entities remain usable if Genesis or its capability is absent.
      if (current) setPortraits({});
    });
    return () => { current = false; };
  }, [refreshKey]);
  return portraits;
}

export default function GenesisEntityPortrait({ entityId, name, portrait }: {
  entityId: string; name: string; portrait?: EntityPortraitSource;
}) {
  const [image, setImage] = useState<{ assetId: string; url: string } | null>(null);
  const sessionId = portrait?.sessionId;
  const assetId = portrait?.assetId;
  useEffect(() => {
    if (!sessionId || !assetId) return;
    let current = true;
    let objectUrl: string | null = null;
    void genesisAssetBlob(sessionId, assetId).then((blob) => {
      if (!current || blob.type !== "image/png") return;
      objectUrl = URL.createObjectURL(blob);
      setImage({ assetId, url: objectUrl });
    }).catch(() => { if (current) setImage(null); });
    return () => {
      current = false;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [sessionId, assetId]);
  const style = { width: 48, height: 48, borderRadius: 14, objectFit: "cover" as const, flexShrink: 0 };
  if (image && image.assetId === assetId) {
    return <img src={image.url} alt={`${name}'s approved portrait`} style={style} />;
  }
  return <span data-entity-id={entityId} aria-hidden="true" style={{ ...style, display: "grid", placeItems: "center", background: "var(--bg-tertiary, #222536)", color: "var(--text-primary, #ebe8ff)", fontSize: 20 }}>{Array.from(name.trim())[0]?.toLocaleUpperCase() || "✦"}</span>;
}
