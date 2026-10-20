import { useCallback, useEffect, useState } from "react";
import SplashScreen from "./components/SplashScreen";
import LoaderScreen from "./components/LoaderScreen";
import ChatPanel from "./components/ChatPanel";
import { entityHealth, getSessionStatus, getStatus } from "./lib/daemonClient";
import { resolveEntityUrl } from "./lib/connection";
import { isNativeConnection, nativeRuntimeConnection, ownershipHeaders } from "./lib/nativeConnection";
import { showAppWindow } from "./lib/window";
import { defaultProfile, isEntityProfile, type EntityProfile } from "./presence/profile";

type Phase = "splash" | "loading" | "ready" | "error";
type Readiness = "pending" | "ok" | "failed";

const READY_CEILING_MS = 20_000;
const PROFILE_TIMEOUT_MS = 5_000;

function hiveProfileUrl(entityId: string, session: unknown, override?: string): string {
  // The existing session lease carries the actual native Hive address. Only
  // use it when it belongs to this Entity, with explicit developer overrides
  // retaining precedence for browser runs.
  let leasedHiveUrl: string | undefined;
  if (session && typeof session === "object" && "lease" in session && session.lease && typeof session.lease === "object") {
    const lease = session.lease;
    if ("entity_id" in lease && lease.entity_id === entityId && "hive_url" in lease && typeof lease.hive_url === "string") leasedHiveUrl = lease.hive_url;
  }
  const configured = override || leasedHiveUrl || "http://127.0.0.1:43141";
  const base = new URL(configured);
  if (!/^https?:$/.test(base.protocol) || base.username || base.password) {
    throw new Error("The Hive address must be an HTTP or HTTPS URL without credentials.");
  }
  return `${base.origin}${base.pathname.replace(/\/+$/, "")}/v1/entities/${encodeURIComponent(entityId)}/presence`;
}

export default function App() {
  const [phase, setPhase] = useState<Phase>("splash");
  const [readiness, setReadiness] = useState<Readiness>("pending");
  const [connectionError, setConnectionError] = useState<string>();
  const [baseUrl, setBaseUrl] = useState<string>("");
  const [runtimeEntityId, setRuntimeEntityId] = useState<string>();
  const [identityError, setIdentityError] = useState(false);
  const [identityAttempt, setIdentityAttempt] = useState(0);
  const [profile, setProfile] = useState<EntityProfile>();
  const [profileNotice, setProfileNotice] = useState<string | null>(null);

  useEffect(() => {
    if (readiness !== "ok") return;
    const controller = new AbortController();
    let disposed = false;
    let identityConfirmed = false;
    const timeout = setTimeout(() => {
      controller.abort();
      if (!disposed) {
        if (!identityConfirmed) setIdentityError(true);
        else setProfileNotice("Visual profile unavailable. Using the default appearance.");
      }
    }, PROFILE_TIMEOUT_MS);
    setRuntimeEntityId(undefined);
    setIdentityError(false);
    setProfile(undefined);
    setProfileNotice(null);
    // Appearance belongs to Hive storage. Fetch it separately so a missing
    // profile never prevents the Entity's working text connection from opening.
    void (async () => {
      try {
        const status = await getStatus(controller.signal);
        if (!status || typeof status !== "object" || !("entity_id" in status) || typeof status.entity_id !== "string" || !status.entity_id) {
          throw new Error("Entity status did not include an identity.");
        }
        if (disposed || controller.signal.aborted) return;
        // Pin the confirmed Entity ID as well as the native process connection
        // so chat/history cannot restore another Entity under this appearance.
        identityConfirmed = true;
        setRuntimeEntityId(status.entity_id);
        if ("name" in status && typeof status.name === "string") {
          setProfile(defaultProfile(status.name));
        }
        let profileUrl: string;
        let headers: HeadersInit = { Accept: "application/json" };
        if (isNativeConnection()) {
          const connection = await nativeRuntimeConnection(baseUrl, undefined, controller.signal);
          if (!connection.hive_url) throw new Error("Native Runtime did not verify its Hive.");
          profileUrl = `${connection.hive_url}/v1/entities/${encodeURIComponent(status.entity_id)}/presence`;
          headers = ownershipHeaders(connection.hive_process_identity, headers);
        } else {
          const override = new URLSearchParams(window.location.search).get("hiveUrl") || import.meta.env.VITE_HIVE_DAEMON_URL;
          // Optional session discovery must not consume the profile deadline
          // when the browser fixture already supplies an explicit address.
          const session = override ? null : await getSessionStatus(controller.signal).catch(() => null);
          profileUrl = hiveProfileUrl(status.entity_id, session, override);
        }
        if (controller.signal.aborted) return;
        const response = await fetch(profileUrl, {
          headers, signal: controller.signal, redirect: isNativeConnection() ? "error" : undefined,
        });
        const envelope: unknown = await response.json();
        if (!response.ok || !envelope || typeof envelope !== "object" || !("ok" in envelope) || envelope.ok !== true || !("data" in envelope) || !isEntityProfile(envelope.data)) {
          throw new Error("Hive could not provide a valid visual profile.");
        }
        if (!disposed && !controller.signal.aborted) setProfile(envelope.data);
      } catch {
        if (!disposed) {
          if (!identityConfirmed) setIdentityError(true);
          else setProfileNotice("Visual profile unavailable. Using the default appearance.");
        }
      } finally {
        clearTimeout(timeout);
      }
    })();
    return () => { disposed = true; clearTimeout(timeout); controller.abort(); };
  }, [readiness, identityAttempt, baseUrl]);

  const checkReadiness = useCallback(async () => {
    setReadiness("pending");
    setConnectionError(undefined);
    try {
      setBaseUrl(await resolveEntityUrl());
    } catch (error) {
      setConnectionError(error instanceof Error ? error.message : String(error));
      setReadiness("failed");
      return;
    }
    const deadline = Date.now() + READY_CEILING_MS;
    while (Date.now() < deadline) {
      if (await entityHealth()) {
        setReadiness("ok");
        return;
      }
      await new Promise((resolve) => setTimeout(resolve, 500));
    }
    setReadiness("failed");
  }, []);

  // Reveal the window with the splash already painted, then begin readiness.
  useEffect(() => {
    void showAppWindow();
    void checkReadiness();
  }, [checkReadiness]);

  // Once past the splash, follow readiness into chat or the retry screen.
  useEffect(() => {
    if (phase !== "loading") return;
    if (readiness === "ok") setPhase("ready");
    else if (readiness === "failed") setPhase("error");
  }, [phase, readiness]);

  const onSplashComplete = useCallback(() => {
    if (readiness === "ok") setPhase("ready");
    else if (readiness === "failed") setPhase("error");
    else setPhase("loading");
  }, [readiness]);

  if (phase === "splash") return <SplashScreen onComplete={onSplashComplete} />;
  if (phase === "loading") return <LoaderScreen message="Waking up your Entity…" />;
  if (phase === "error") {
    return (
      <LoaderScreen
        error
        message={connectionError ?? "Your Entity isn't responding yet."}
        onRetry={() => {
          setPhase("loading");
          void checkReadiness();
        }}
      />
    );
  }
  if (!runtimeEntityId) {
    return <LoaderScreen
      error={identityError}
      message={identityError ? "Your Entity's identity could not be confirmed. No conversation has been opened." : "Confirming your Entity."}
      onRetry={identityError ? () => setIdentityAttempt(value => value + 1) : undefined}
    />;
  }

  return (
    <div className="h-screen">
      <ChatPanel baseUrl={baseUrl} expectedEntityId={runtimeEntityId} profile={profile} profileNotice={profileNotice} />
    </div>
  );
}
