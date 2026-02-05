import { invoke } from "@tauri-apps/api/core";
import { useState, useEffect, useRef } from "react";
import LlmSetupPanel from "./LlmSetupPanel";
import BirthChat, { BirthChatHandle } from "./BirthChat";
import ApiKeyModal from "./ApiKeyModal";

type Stage =
  | "Darkness"
  | "KeyPresentation"
  | "Ignition"
  | "Connectivity"
  | "Genesis"
  | "SoulPreview"
  | "Emergence"
  | "Life"
  | "Repair";

type IdentityStatus = "Clean" | "Complete" | "Broken";

interface BootSequenceProps {
  onComplete: () => void;
}

interface KeypairGenerationResult {
  private_key_base64: string;
  public_key_path: string;
  newly_generated: boolean;
}

export default function BootSequence({ onComplete }: BootSequenceProps) {
  const [stage, setStage] = useState<Stage>("Darkness");
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [privateKey, setPrivateKey] = useState("");
  const [publicKeyPath, setPublicKeyPath] = useState("");
  const [keySaved, setKeySaved] = useState(false);
  const [copied, setCopied] = useState(false);
  const [repairKey, setRepairKey] = useState("");
  const [activeApiKeyProvider, setActiveApiKeyProvider] = useState<string | null>(null);
  const [storedProviders, setStoredProviders] = useState<string[]>([]);
  const [soulPreview, setSoulPreview] = useState("");
  const [genesisName, setGenesisName] = useState("");
  const [genesisPurpose, setGenesisPurpose] = useState("");
  const [genesisPersonality, setGenesisPersonality] = useState("");

  // Ref to BirthChat for injecting key confirmations
  const birthChatRef = useRef<BirthChatHandle>(null);

  // Auto-start boot sequence on mount
  useEffect(() => {
    handleStart();
  }, []);

  const handleStart = async () => {
    setError("");
    setStage("Darkness");
    setMessage("Initializing...");

    try {
      // 1. Initialize soul (copy templates, create internal keyring)
      await invoke("init_soul");
      setMessage("Checking identity status...");

      // 2. Check identity status
      const status = await invoke<IdentityStatus>("check_identity_status");

      if (status === "Clean") {
        // First run: start birth and generate identity
        await invoke("start_birth");
        setMessage("Generating signing keypair...");
        const keypairResult = await invoke<KeypairGenerationResult>("generate_identity");

        setPrivateKey(keypairResult.private_key_base64);
        setPublicKeyPath(keypairResult.public_key_path);
        setStage("KeyPresentation");
        return;
      } else if (status === "Broken") {
        setStage("Repair");
        setError("Identity verification failed. Signatures are missing or invalid.");
        return;
      }

      // Identity is Complete — run legacy flow
      await runLegacyBoot();
    } catch (e) {
      setError(String(e));
      setStage("Darkness");
    }
  };

  const runLegacyBoot = async () => {
    try {
      setMessage("Running startup checks...");

      const result = await invoke<{
        heartbeat_ok: boolean;
        verification_ok: boolean;
        error: string | null;
      }>("run_startup_checks");

      if (!result.heartbeat_ok) {
        setError(result.error || "LLM heartbeat failed. Is the local LLM server running?");
        setStage("Darkness");
        return;
      }

      if (!result.verification_ok && result.error) {
        setError(result.error);
        setStage("Repair");
        return;
      }

      // Start birth, skip to life, complete
      await invoke("start_birth");
      await invoke("verify_crypto");
      await invoke("skip_to_life_for_mvp");
      await invoke("complete_birth");

      setStage("Life");
      setMessage("I am awake.");
      await new Promise((resolve) => setTimeout(resolve, 500));
      onComplete();
    } catch (e) {
      setError(String(e));
      setStage("Darkness");
    }
  };

  const handleSkipInteractive = async () => {
    try {
      setMessage("Completing default birth...");
      setStage("Emergence");

      // Make sure we have a birth orchestrator
      try {
        await invoke("start_birth");
      } catch {
        // Already started, that's fine
      }

      await invoke("skip_to_life_for_mvp");
      await invoke("complete_birth");

      setStage("Life");
      setMessage("I am awake.");
      await new Promise((resolve) => setTimeout(resolve, 500));
      onComplete();
    } catch (e) {
      setError(String(e));
    }
  };

  const handleRepair = async () => {
    setError("");
    setMessage("Attempting repair...");

    try {
      await invoke("repair_identity", {
        params: {
          private_key: repairKey.trim(),
          reset: false,
        },
      });
      setRepairKey("");
      handleStart();
    } catch (e) {
      setError(String(e));
    }
  };

  const handleReset = async () => {
    if (
      !confirm(
        "WARNING: This will delete your identity and reset AO to a fresh state. You will lose your current trust relationship. Are you sure?"
      )
    ) {
      return;
    }

    setError("");
    setMessage("Resetting identity...");

    try {
      await invoke("repair_identity", {
        params: {
          private_key: null,
          reset: true,
        },
      });
      handleStart();
    } catch (e) {
      setError(String(e));
    }
  };

  const handleCopyKey = async () => {
    try {
      await navigator.clipboard.writeText(privateKey);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      const textArea = document.createElement("textarea");
      textArea.value = privateKey;
      document.body.appendChild(textArea);
      textArea.select();
      document.execCommand("copy");
      document.body.removeChild(textArea);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  };

  const handleContinueFromKeyPresentation = () => {
    setPrivateKey(""); // Clear from memory
    invoke("advance_past_darkness").catch(console.error);
    setStage("Ignition");
  };

  const handleLlmConnected = (_url: string) => {
    setStage("Connectivity");
  };

  const handleApiKeySaved = () => {
    const provider = activeApiKeyProvider;
    setActiveApiKeyProvider(null);
    if (provider) {
      setStoredProviders((prev) => [...prev, provider]);
      // Inject message into BirthChat so LLM knows the key was saved
      birthChatRef.current?.injectKeyConfirmation(provider);
    }
  };

  const handleConnectivityAdvance = async () => {
    try {
      await invoke("advance_to_genesis");
      setStage("Genesis");
    } catch (e) {
      setError(String(e));
    }
  };

  const handleGenesisDone = () => {
    // Show a form for name/purpose/personality extracted from conversation
    setStage("SoulPreview");
  };

  const handleCrystallize = async () => {
    if (!genesisName.trim()) {
      setError("Name is required");
      return;
    }

    setError("");
    try {
      const preview = await invoke<string>("crystallize_soul", {
        name: genesisName.trim(),
        purpose: genesisPurpose.trim() || "assist, retrieve, connect, and surface information",
        personality: genesisPersonality.trim() || "helpful, clear, and honest",
      });
      setSoulPreview(preview);
      setStage("Emergence");
    } catch (e) {
      setError(String(e));
    }
  };

  const handleCompleteEmergence = async () => {
    setMessage("Signing constitutional documents...");
    try {
      await invoke("complete_emergence");
      setStage("Life");
      setMessage("I am awake.");
      await new Promise((resolve) => setTimeout(resolve, 1500));
      onComplete();
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div className="min-h-screen bg-black text-theme-text font-mono flex flex-col">
      <pre className="text-sm p-4 border-b border-theme-border-dim">
        AO BOOT SEQUENCE
        ==================
      </pre>

      <div className="flex-1 overflow-auto">
        {/* ── DARKNESS ── */}
        {stage === "Darkness" && !error && (
          <div className="p-6">
            <p className="mb-4">{message || "Preparing to start..."}</p>
            <div className="animate-pulse">...</div>
          </div>
        )}

        {/* ── KEY PRESENTATION ── */}
        {stage === "KeyPresentation" && (
          <div className="p-6 max-w-2xl">
            <div className="border border-yellow-500 bg-yellow-500/10 p-4 rounded mb-6">
              <h2 className="text-yellow-500 text-lg font-bold mb-2">
                CRITICAL: SAVE YOUR PRIVATE KEY
              </h2>
              <p className="text-yellow-400 text-sm mb-2">
                This is the ONLY time you will see this key. AO does NOT store
                it.
              </p>
            </div>

            <div className="mb-6">
              <p className="text-sm mb-2 text-gray-400">
                Your Private Signing Key (Ed25519, Base64):
              </p>
              <div className="relative">
                <textarea
                  readOnly
                  value={privateKey}
                  className="w-full bg-gray-900 border border-theme-primary-faint rounded p-3 text-theme-text-bright font-mono text-sm resize-none"
                  rows={3}
                  onClick={(e) => (e.target as HTMLTextAreaElement).select()}
                />
                <button
                  onClick={handleCopyKey}
                  className="absolute top-2 right-2 px-2 py-1 text-xs border border-theme-primary rounded hover:bg-theme-primary-glow"
                >
                  {copied ? "Copied!" : "Copy"}
                </button>
              </div>
            </div>

            <div className="mb-6 text-sm">
              <p className="text-gray-400 mb-1">Public key saved to:</p>
              <code className="text-theme-text-bright text-xs break-all">
                {publicKeyPath}
              </code>
            </div>

            <div className="border border-red-700 bg-red-900/20 p-4 rounded mb-6">
              <h3 className="text-red-400 font-bold mb-2">SECURITY WARNINGS</h3>
              <ul className="text-red-300 text-sm space-y-2">
                <li>
                  - <strong>This key proves you are AO's legitimate mentor.</strong>
                </li>
                <li>
                  - <strong>Store it securely</strong> (password manager, encrypted
                  drive, offline backup).
                </li>
                <li>
                  - <strong>Never share this key</strong> with anyone or any
                  service.
                </li>
                <li>
                  - <strong>If you lose this key:</strong> You cannot re-verify
                  AO's integrity after reinstall.
                </li>
                <li>
                  - <strong>If this key is compromised:</strong> Someone could
                  create fake constitutional documents.
                </li>
              </ul>
            </div>

            <div className="mb-6">
              <label className="flex items-center gap-3 cursor-pointer">
                <input
                  type="checkbox"
                  checked={keySaved}
                  onChange={(e) => setKeySaved(e.target.checked)}
                  className="w-5 h-5 accent-[var(--color-primary)]"
                />
                <span className="text-sm">
                  I have saved my private key securely and understand I will not
                  see it again.
                </span>
              </label>
            </div>

            <button
              disabled={!keySaved}
              onClick={handleContinueFromKeyPresentation}
              className={`px-6 py-3 rounded font-bold ${
                keySaved
                  ? "border border-theme-primary hover:bg-theme-primary-glow text-theme-text"
                  : "border border-gray-600 text-gray-600 cursor-not-allowed"
              }`}
            >
              Continue
            </button>
          </div>
        )}

        {/* ── IGNITION ── */}
        {stage === "Ignition" && (
          <LlmSetupPanel
            onConnected={handleLlmConnected}
            onSkip={handleSkipInteractive}
            showSkip={true}
          />
        )}

        {/* ── CONNECTIVITY ── */}
        {stage === "Connectivity" && (
          <div className="flex flex-col h-full" style={{ minHeight: "60vh" }}>
            {/* API Key buttons */}
            <div className="px-4 py-2 border-b border-theme-border bg-theme-surface flex gap-2 flex-wrap">
              <span className="text-theme-text-dim text-xs self-center mr-2">Add key:</span>
              {["openai", "anthropic", "xai", "google"].map((p) => (
                <button
                  key={p}
                  className={`text-xs px-2 py-1 rounded border ${
                    storedProviders.includes(p)
                      ? "border-theme-primary-faint text-theme-primary-faint"
                      : "border-theme-primary text-theme-text hover:bg-theme-primary-glow"
                  }`}
                  onClick={() => setActiveApiKeyProvider(p)}
                  disabled={storedProviders.includes(p)}
                >
                  {storedProviders.includes(p) ? `${p} [saved]` : p}
                </button>
              ))}
            </div>

            <BirthChat
              ref={birthChatRef}
              stage="Connectivity"
              onStageAdvance={handleConnectivityAdvance}
            />

            {activeApiKeyProvider && (
              <ApiKeyModal
                provider={activeApiKeyProvider}
                onSaved={handleApiKeySaved}
                onCancel={() => setActiveApiKeyProvider(null)}
              />
            )}
          </div>
        )}

        {/* ── GENESIS ── */}
        {stage === "Genesis" && (
          <div className="flex flex-col h-full" style={{ minHeight: "60vh" }}>
            <BirthChat
              stage="Genesis"
              onStageAdvance={handleGenesisDone}
            />
          </div>
        )}

        {/* ── SOUL PREVIEW ── */}
        {stage === "SoulPreview" && (
          <div className="p-6 max-w-2xl">
            <h2 className="text-theme-primary-dim text-lg mb-4">
              Genesis: Define Your Agent
            </h2>
            <p className="text-theme-text-dim text-sm mb-6">
              Based on your conversation, fill in the details below. These will
              become part of AO's soul document.
            </p>

            <div className="space-y-4 mb-6">
              <div>
                <label className="block text-theme-text text-sm mb-1">Name</label>
                <input
                  type="text"
                  className="w-full bg-black border border-theme-primary text-theme-primary-dim px-3 py-2 rounded"
                  placeholder="AO"
                  value={genesisName}
                  onChange={(e) => setGenesisName(e.target.value)}
                  autoFocus
                />
              </div>
              <div>
                <label className="block text-theme-text text-sm mb-1">
                  Purpose
                </label>
                <input
                  type="text"
                  className="w-full bg-black border border-theme-primary text-theme-primary-dim px-3 py-2 rounded"
                  placeholder="assist, retrieve, connect, and surface information"
                  value={genesisPurpose}
                  onChange={(e) => setGenesisPurpose(e.target.value)}
                />
              </div>
              <div>
                <label className="block text-theme-text text-sm mb-1">
                  Personality / Tone
                </label>
                <input
                  type="text"
                  className="w-full bg-black border border-theme-primary text-theme-primary-dim px-3 py-2 rounded"
                  placeholder="helpful, clear, and honest"
                  value={genesisPersonality}
                  onChange={(e) => setGenesisPersonality(e.target.value)}
                />
              </div>
            </div>

            {error && <p className="text-red-400 text-sm mb-4">{error}</p>}

            <button
              onClick={handleCrystallize}
              className="border border-theme-primary px-6 py-3 rounded font-bold hover:bg-theme-primary-glow"
            >
              Crystallize Soul
            </button>
          </div>
        )}

        {/* ── EMERGENCE ── */}
        {stage === "Emergence" && (
          <div className="p-6">
            {soulPreview && (
              <div className="mb-6">
                <h2 className="text-theme-primary-dim text-lg mb-2">Soul Document</h2>
                <pre className="bg-theme-surface border border-theme-border rounded p-4 text-theme-text-bright text-sm whitespace-pre-wrap max-h-64 overflow-auto">
                  {soulPreview}
                </pre>
              </div>
            )}

            <div className="text-center">
              <p className="text-theme-text mb-4">
                Ready to sign and come alive.
              </p>
              <button
                onClick={handleCompleteEmergence}
                className="border border-theme-primary px-8 py-3 rounded font-bold hover:bg-theme-primary-glow text-lg"
              >
                Emerge
              </button>
            </div>

            {error && <p className="text-red-400 text-sm mt-4">{error}</p>}
          </div>
        )}

        {/* ── LIFE ── */}
        {stage === "Life" && (
          <div className="p-6 text-center">
            <p className="text-theme-primary-dim text-xl mb-2">{message}</p>
            <div className="animate-pulse text-theme-text-dim">...</div>
          </div>
        )}

        {/* ── REPAIR ── */}
        {stage === "Repair" && (
          <div className="p-6 max-w-2xl">
            <div className="border border-red-500 bg-red-900/20 p-4 rounded mb-6">
              <h2 className="text-red-500 text-lg font-bold mb-2">
                IDENTITY VERIFICATION FAILED
              </h2>
              <p className="text-red-400 text-sm mb-4">{error}</p>
              <p className="text-gray-400 text-sm">
                AO's constitutional documents cannot be verified. This usually
                happens if files were corrupted or tampered with.
              </p>
            </div>

            <div className="mb-8">
              <h3 className="text-theme-text font-bold mb-2">
                Option 1: Recover Identity
              </h3>
              <p className="text-sm text-gray-400 mb-2">
                If you have your <strong>Private Key</strong> (saved from first
                run), enter it below to re-sign the documents.
              </p>
              <textarea
                value={repairKey}
                onChange={(e) => setRepairKey(e.target.value)}
                placeholder="Paste your private key here..."
                className="w-full bg-gray-900 border border-theme-primary-faint rounded p-3 text-theme-text-bright font-mono text-sm resize-none mb-2"
                rows={3}
              />
              <button
                onClick={handleRepair}
                disabled={!repairKey.trim()}
                className={`px-4 py-2 rounded font-bold text-sm ${
                  repairKey.trim()
                    ? "bg-theme-surface border border-theme-primary text-theme-text hover:bg-theme-primary-glow"
                    : "bg-gray-900 border border-gray-700 text-gray-600 cursor-not-allowed"
                }`}
              >
                Recover Identity
              </button>
            </div>

            <div className="border-t border-gray-800 pt-6">
              <h3 className="text-red-400 font-bold mb-2">
                Option 2: Hard Reset
              </h3>
              <p className="text-sm text-gray-400 mb-4">
                If you lost your key, you must reset AO.{" "}
                <strong>
                  This destroys the current trust relationship.
                </strong>{" "}
                You will be treated as a new mentor.
              </p>
              <button
                onClick={handleReset}
                className="px-4 py-2 rounded font-bold text-sm border border-red-700 text-red-500 hover:bg-red-900/20"
              >
                Reset Identity (Destructive)
              </button>
            </div>
          </div>
        )}

        {/* ── GENERAL ERROR ── */}
        {error && !["Repair", "SoulPreview", "Emergence"].includes(stage) && stage !== "Darkness" && (
          <div className="p-4">
            <p className="text-red-400">{error}</p>
            <button
              className="border border-theme-primary px-4 py-2 rounded hover:bg-theme-primary-glow mt-2"
              onClick={handleStart}
            >
              Retry
            </button>
          </div>
        )}

        {error && stage === "Darkness" && (
          <div className="p-6">
            <p className="text-red-400 mb-4">{error}</p>
            <button
              className="border border-theme-primary px-4 py-2 rounded hover:bg-theme-primary-glow"
              onClick={handleStart}
            >
              Retry
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
