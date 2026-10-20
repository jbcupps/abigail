import { isEntityProfile, type EntityProfile } from "../../../../entity-runtime-app/src-ui/src/presence/profile";
import type { GenesisConnection, GenesisSeed } from "./genesisClient";
import { isCanonicalUuid } from "./presenceCreation";

export const GENESIS_DRAFT_KEY = "abigail.first-signal.genesis.v1";
export interface GenesisDraft {
  version: 1; key: string; connection: GenesisConnection; seed: GenesisSeed; profile: EntityProfile;
}
export function readGenesisDraft(): GenesisDraft | null {
  const raw = localStorage.getItem(GENESIS_DRAFT_KEY);
  if (!raw) return null;
  const value = JSON.parse(raw) as GenesisDraft;
  if (value?.version !== 1 || !isCanonicalUuid(value.key) || !isEntityProfile(value.profile)
    || value.seed?.mode !== "quick" || !value.seed.name?.trim() || !value.seed.purpose?.trim()
    || value.seed.name !== value.profile.name || value.seed.purpose !== value.profile.purpose
    || !value.connection || !(value.connection.profileId === null || isCanonicalUuid(value.connection.profileId))) throw new Error("The saved creation record cannot be read. It has been kept; check saved creations in its original Hive.");
  const url = new URL(value.connection.hiveUrl);
  if (url.origin !== value.connection.hiveUrl || url.protocol !== "http:" || !["127.0.0.1", "localhost", "[::1]"].includes(url.hostname)) throw new Error("The saved creation has an invalid Hive connection.");
  return value;
}
/** Caller holds the shared creation lock. The immutable journal precedes every POST. */
export function saveGenesisDraft(value: GenesisDraft): void {
  if (localStorage.getItem(GENESIS_DRAFT_KEY) !== null) throw new Error("An unfinished creation already exists. Resume it before starting another.");
  localStorage.setItem(GENESIS_DRAFT_KEY, JSON.stringify(value));
  assertGenesisDraft(value);
}
export function assertGenesisDraft(value: GenesisDraft): void {
  if (JSON.stringify(readGenesisDraft()) !== JSON.stringify(value)) throw new Error("This creation changed in another tab. Reload its saved record before continuing.");
}
export function clearGenesisDraft(value: GenesisDraft): void {
  assertGenesisDraft(value);
  localStorage.removeItem(GENESIS_DRAFT_KEY);
}
