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
    const entityId = session.effective_entity_id ?? session.definition.entity_id;
    const assetId = session.definition.portrait_asset_id;
    const awakened = session.state === "ready" || Boolean((session as GenesisSession & { awakening?: string }).awakening);
    if (entityId && awakened && !seen.has(entityId)) {
      seen.add(entityId);
      if (assetId) portraits[entityId] = { sessionId: session.id, assetId };
    }
  }
  return portraits;
}

/** Entities Genesis allocated but has not awakened, mapped to their creation
 * session. Generic setup would bypass their approved story. */
export function awaitingFromSessions(sessions: GenesisSession[]): Record<string, string> {
  const awaiting: Record<string, string> = {};
  for (const session of sessions) {
    const entityId = session.effective_entity_id ?? session.definition.entity_id;
    if (entityId && !session.awakening) awaiting[entityId] = session.id;
  }
  return awaiting;
}

export interface GenesisEntities {
  portraits: EntityPortraits;
  awaiting: Record<string, string>;
  loading: boolean;
  error: string | null;
}

/** Call once in the collection parent; change refreshKey after leaving Genesis
 * or switching the Hive connection to refresh every card together. */
export function useGenesisEntities(refreshKey?: unknown): GenesisEntities {
  const [entities, setEntities] = useState<GenesisEntities>({
    portraits: {},
    awaiting: {},
    loading: true,
    error: null,
  });
  useEffect(() => {
    let current = true;
    setEntities((prior) => ({ ...prior, loading: true, error: null }));
    void listGenesisSessions().then((sessions) => {
      if (current) {
        setEntities({
          portraits: portraitsFromSessions(sessions),
          awaiting: awaitingFromSessions(sessions),
          loading: false,
          error: null,
        });
      }
    }).catch((error: unknown) => {
      // Existing Entities remain usable if Genesis or its capability is absent.
      if (current) {
        setEntities((prior) => ({
          ...prior,
          loading: false,
          error: error instanceof Error ? error.message : "Genesis records could not be refreshed.",
        }));
      }
    });
    return () => { current = false; };
  }, [refreshKey]);
  return entities;
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
