import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";

interface SoulIdentityInfo {
  id: string;
  name: string;
  directory: string;
  birth_complete: boolean;
  birth_date: string | null;
  primary_color?: string | null;
  avatar_url?: string | null;
}

interface SoulRegistryProps {
  onSoulSelected: (soulId: string) => void;
  onNewSoul: (soulId: string) => void;
}

export default function SoulRegistry({
  onSoulSelected,
  onNewSoul,
}: SoulRegistryProps) {
  const [souls, setSouls] = useState<SoulIdentityInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [birthing, setBirthing] = useState(false);
  const [newSoulName, setNewSoulName] = useState("");
  const [migrating, setMigrating] = useState(false);
  const mountedRef = useRef(true);

  const fetchSouls = async () => {
    try {
      if (mountedRef.current) setLoading(true);
      const identities = await invoke<SoulIdentityInfo[]>("get_identities");
      if (!mountedRef.current) return;
      setSouls(identities);
      setError(null);
    } catch (e) {
      if (!mountedRef.current) return;
      setError(String(e));
    } finally {
      if (mountedRef.current) setLoading(false);
    }
  };

  useEffect(() => {
    mountedRef.current = true;
    fetchSouls();
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const handleBirthSoul = async () => {
    if (!newSoulName.trim()) return;

    try {
      setBirthing(true);
      const uuid = await invoke<string>("create_agent", {
        name: newSoulName.trim(),
      });
      if (!mountedRef.current) return;
      setNewSoulName("");
      setBirthing(false);
      // Load the new entity and go to birth ceremony
      await invoke("load_agent", { agentId: uuid });
      if (!mountedRef.current) return;
      onNewSoul(uuid);
    } catch (e) {
      if (!mountedRef.current) return;
      setError(String(e));
      setBirthing(false);
    }
  };

  const handleWakeSoul = async (soulId: string) => {
    try {
      await invoke("load_agent", { agentId: soulId });
      if (!mountedRef.current) return;
      onSoulSelected(soulId);
    } catch (e) {
      if (!mountedRef.current) return;
      setError(String(e));
    }
  };

  const handleDeleteSoul = async (e: React.MouseEvent, soul: SoulIdentityInfo) => {
    e.stopPropagation();
    if (!confirm(`Are you sure you want to delete "${soul.name}"? This action cannot be undone.`)) {
      return;
    }

    try {
      await invoke("delete_agent_identity", { agentId: soul.id });
      await fetchSouls();
    } catch (e) {
      setError(String(e));
    }
  };

  const handleArchiveSoul = async (e: React.MouseEvent, soul: SoulIdentityInfo) => {
    e.stopPropagation();
    if (!confirm(`Archive "${soul.name}" to backups? You can restore manually later.`)) {
      return;
    }
    try {
      await invoke("archive_agent_identity", { agentId: soul.id });
      await fetchSouls();
    } catch (e) {
      setError(String(e));
    }
  };

  const handleMigrateLegacy = async () => {
    try {
      setMigrating(true);
      const uuid = await invoke<string | null>("migrate_legacy_identity");
      if (uuid) {
        await fetchSouls();
      }
    } catch (e) {
      if (!mountedRef.current) return;
      setError(String(e));
    } finally {
      if (mountedRef.current) setMigrating(false);
    }
  };

  if (loading) {
    return (
      <div className="min-h-screen bg-theme-bg text-theme-text-dim font-mono flex items-center justify-center">
        <div className="animate-pulse">Consulting the Soul Registry...</div>
      </div>
    );
  }

  // Error banner (shared between both states)
  const errorBanner = error && (
    <div className="w-full max-w-lg mb-4 p-3 border border-red-800 rounded bg-red-900/20 text-red-400 text-sm">
      {error}
      <button
        className="ml-2 text-red-300 underline"
        onClick={() => setError(null)}
      >
        dismiss
      </button>
    </div>
  );

  // Empty state: Welcome landing page
  if (souls.length === 0) {
    return (
      <div className="min-h-screen bg-theme-bg text-theme-text font-mono flex flex-col items-center justify-center p-8">
        {/* Welcome header */}
        <div className="text-center mb-10">
          <h1 className="text-theme-primary text-4xl font-bold tracking-widest mb-2">
            ABIGAIL
          </h1>
          <p className="text-theme-text-dim text-sm uppercase tracking-widest">
            Sovereign Entity Interface
          </p>
        </div>

        {errorBanner}

        {/* Create soul card */}
        <div className="w-full max-w-md border-2 border-theme-primary rounded-lg p-6 mb-6 bg-theme-bg-elevated">
          <p className="text-theme-text-bright text-sm mb-4">
            Birth a new Sovereign Entity to begin
          </p>
          <div className="flex gap-2">
            <input
              type="text"
              value={newSoulName}
              onChange={(e) => setNewSoulName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") handleBirthSoul();
              }}
              placeholder="Entity name (e.g. Buddy Joe)..."
              className="flex-1 bg-theme-bg border border-theme-border rounded px-3 py-2 text-theme-primary-dim placeholder:text-theme-text-dim focus:border-theme-primary focus:outline-none text-sm"
              disabled={birthing}
              autoFocus
            />
            <button
              className="border border-theme-primary text-theme-primary px-5 py-2 rounded hover:bg-theme-primary-glow disabled:opacity-50 disabled:cursor-not-allowed text-sm font-bold"
              onClick={handleBirthSoul}
              disabled={birthing || !newSoulName.trim()}
            >
              {birthing ? "Birthing..." : "Birth"}
            </button>
          </div>
        </div>

        {/* Migration link */}
        <div className="text-center">
          <div className="text-theme-border text-xs mb-3">or</div>
          <button
            className="text-theme-text-dim hover:text-theme-text text-xs underline"
            onClick={handleMigrateLegacy}
            disabled={migrating}
          >
            {migrating ? "Checking..." : "Recall legacy identity from previous Hive"}
          </button>
        </div>
      </div>
    );
  }

  // Populated state: Soul selector
  return (
    <div className="min-h-screen bg-theme-bg text-theme-text font-mono flex flex-col items-center justify-center p-8">
      {/* Header */}
      <div className="text-center mb-8">
        <h1 className="text-2xl text-theme-primary font-bold tracking-widest">SOUL REGISTRY</h1>
        <p className="text-theme-text-dim text-xs uppercase mt-1 tracking-widest">Abigail Hive Node</p>
      </div>

      {errorBanner}

      {/* Soul cards grid */}
      <div className="w-full max-w-4xl grid grid-cols-1 md:grid-cols-2 gap-4 mb-10">
        {souls.map((soul) => (
          <div
            key={soul.id}
            className="group relative border border-theme-border-dim rounded-lg p-5 hover:border-theme-primary hover:bg-theme-primary-glow cursor-pointer transition-all overflow-hidden"
            onClick={() => handleWakeSoul(soul.id)}
          >
            {/* Visual accent color strip */}
            <div 
              className="absolute top-0 left-0 w-1 h-full bg-theme-primary-dim group-hover:bg-theme-primary"
              style={soul.primary_color ? { backgroundColor: soul.primary_color } : {}}
            />
            
            <div className="flex items-center gap-4">
              {/* Avatar placeholder or real avatar */}
              <div 
                className="w-12 h-12 rounded-full border border-theme-border-dim flex items-center justify-center bg-theme-bg-inset text-lg"
                style={soul.primary_color ? { borderColor: soul.primary_color, color: soul.primary_color } : {}}
              >
                {soul.avatar_url ? (
                  <img src={soul.avatar_url} alt="" className="w-full h-full rounded-full" />
                ) : (
                  soul.name.substring(0, 1).toUpperCase()
                )}
              </div>
              
              <div className="flex-1">
                <div className="flex justify-between items-start">
                  <h2 className="text-theme-text-bright font-bold text-lg group-hover:text-theme-primary">
                    {soul.name}
                  </h2>
                  <div className="flex gap-2 opacity-0 group-hover:opacity-100 transition-opacity">
                    <button
                      onClick={(e) => handleArchiveSoul(e, soul)}
                      className="px-2 py-1 text-[10px] border border-theme-border-dim rounded text-theme-text-dim hover:border-theme-primary hover:text-theme-text"
                      title="Archive Entity"
                    >
                      Archive
                    </button>
                    <button
                      onClick={(e) => handleDeleteSoul(e, soul)}
                      className="px-2 py-1 text-[10px] border border-theme-border-dim rounded text-theme-text-dim hover:border-red-700 hover:text-red-500"
                      title="Delete Entity"
                    >
                      Delete
                    </button>
                  </div>
                </div>
                <div className="flex items-center gap-2 mt-1">
                  <span className={`text-[10px] uppercase px-1.5 py-0.5 rounded border ${soul.birth_complete ? "border-green-900 text-green-500 bg-green-950/20" : "border-yellow-900 text-yellow-500 bg-yellow-950/20"}`}>
                    {soul.birth_complete ? "Active" : "In-Utero"}
                  </span>
                  <span className="text-[10px] text-theme-text-dim font-mono">
                    ID: {soul.id.substring(0, 8)}
                  </span>
                </div>
              </div>
            </div>
            
            {soul.birth_date && (
              <div className="mt-4 pt-3 border-t border-theme-border-dim flex justify-between items-center text-[10px] text-theme-text-dim uppercase tracking-tighter">
                <span>Birthed</span>
                <span>{soul.birth_date}</span>
              </div>
            )}
          </div>
        ))}
      </div>

      {/* Initialize New Soul */}
      <div className="w-full max-w-lg border border-theme-border-dim rounded-lg p-6 bg-theme-bg-inset">
        <h3 className="text-theme-text text-sm font-bold mb-4 uppercase tracking-widest">
          Birth New Entity
        </h3>
        <div className="flex gap-2">
          <input
            type="text"
            value={newSoulName}
            onChange={(e) => setNewSoulName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") handleBirthSoul();
            }}
            placeholder="Entity name..."
            className="flex-1 bg-theme-bg border border-theme-border-dim rounded px-3 py-2 text-theme-primary-dim placeholder:text-theme-text-dim focus:border-theme-primary focus:outline-none text-sm"
            disabled={birthing}
          />
          <button
            className="border border-theme-primary-faint text-theme-primary px-6 py-2 rounded hover:bg-theme-primary-glow disabled:opacity-50 disabled:cursor-not-allowed text-sm font-bold uppercase tracking-widest"
            onClick={handleBirthSoul}
            disabled={birthing || !newSoulName.trim()}
          >
            {birthing ? "..." : "Birth"}
          </button>
        </div>
      </div>
    </div>
  );
}
