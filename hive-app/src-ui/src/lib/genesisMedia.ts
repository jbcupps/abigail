import { hiveRequestHeaders } from "../../../../entity-runtime-app/src-ui/src/lib/nativeConnection";
import { genesisRequest, resolveGenesisConnection } from "./genesisClient";

export interface MediaCapabilities {
  portrait_available: boolean; video_available: boolean; portrait_model: string; video_model: string;
  portrait_reservation_usd: number; video_reservation_usd: number; daily_budget_usd: number;
  daily_job_limit: number; video_seconds: number[]; notice: string;
}
export interface GenesisJob {
  job_id: string; session_id: string; kind: "portrait" | "manifestation";
  status: "queued" | "submitting" | "running" | "completed" | "failed" | "cancelled" | "interrupted";
  progress: number; error?: string | null; asset_ids: string[]; reference_asset_id?: string | null;
  reserved_cost_usd: number; candidates: number; seconds: number; created_at: string; updated_at: string;
}
export interface GenesisAsset {
  asset_id: string; kind: "portrait" | "manifestation"; mime_type: string; version: number;
  approved: boolean; identity_review_required: boolean; reference_asset_id?: string | null;
  job_id: string; created_at: string;
}
export interface GenesisMedia { jobs: GenesisJob[]; assets: GenesisAsset[] }
const pathFor = (sessionId: string) => `/sessions/${encodeURIComponent(sessionId)}/media`;
export const getGenesisMediaCapabilities = () => genesisRequest<MediaCapabilities>("/media/capabilities");
export const getGenesisMedia = (sessionId: string) => genesisRequest<GenesisMedia>(pathFor(sessionId));
export const requestGenesisMedia = (sessionId: string, args: Record<string, unknown>) => genesisRequest<unknown>(`${pathFor(sessionId)}/jobs`, args);
export const actOnGenesisJob = (sessionId: string, jobId: string, action: "retry" | "cancel") => genesisRequest<unknown>(`${pathFor(sessionId)}/jobs/${encodeURIComponent(jobId)}/${action}`, { cost_confirmed: action === "retry" });
export const approveGenesisAsset = (sessionId: string, assetId: string) => genesisRequest<unknown>(`${pathFor(sessionId)}/assets/${encodeURIComponent(assetId)}/approve`, { identity_confirmed: true });
export const deleteGenesisAsset = (sessionId: string, assetId: string) => genesisRequest<unknown>(`${pathFor(sessionId)}/assets/${encodeURIComponent(assetId)}`, undefined, "DELETE");
export async function genesisAssetBlob(sessionId: string, assetId: string): Promise<Blob> {
  const { base, token } = await resolveGenesisConnection();

  const signal = AbortSignal.timeout(30_000);
  const headers = await hiveRequestHeaders(base, { Authorization: `Bearer ${token}` }, signal);
  const response = await fetch(`${base}/v1/genesis${pathFor(sessionId)}/assets/${encodeURIComponent(assetId)}`, { headers, signal, redirect: "error" });
  if (!response.ok) throw new Error("This visual could not be loaded. Try again.");
  return response.blob();
}
