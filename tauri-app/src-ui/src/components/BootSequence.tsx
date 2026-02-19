import { invoke } from "@tauri-apps/api/core";
import { useState, useEffect, useRef } from "react";
import LlmSetupPanel from "./LlmSetupPanel";
import BirthChat, { BirthChatHandle } from "./BirthChat";
import ApiKeyModal from "./ApiKeyModal";
import SoulCrystallization from "./SoulCrystallization";
import GenesisPathSelector from "./GenesisPathSelector";
import GenesisChat from "./GenesisChat";
import ForgeScenario from "./ForgeScenario";

type Stage =
  | "Darkness"
  | "KeyPresentation"
  | "Ignition"
  | "Connectivity"
  | "Genesis"
  | "GenesisChat"
  | "GenesisForge"
  | "Crystallization"
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
  const [crystalName, setCrystalName] = useState("");
  const [crystalPurpose, setCrystalPurpose] = useState("");
  const [crystalPersonality, setCrystalPersonality] = useState("");
  const [crystalMentorName, setCrystalMentorName] = useState("");
  const [genesisPath, setGenesisPath] = useState<string | null>(null);

  // Ref to BirthChat for injecting key confirmations
  const birthChatRef = useRef<BirthChatHandle>(null);
  const mountedRef = useRef(true);

  // Auto-start boot sequence on mount
  useEffect(() => {
    handleStart();
    return () => { mountedRef.current = false; };
  }, []);

  const handleStart = async () => {
    setError("");
    setStage("Darkness");
    setMessage("Preparing secure environment...");

    try {
      // 1. Initialize soul (copy templates, create internal keyring)
      await invoke("init_soul");
      if (!mountedRef.current) return;
      setMessage("Checking identity status...");

      // 2. Check for interrupted birth (closed app mid-way through first run)
      interface InterruptedBirthInfo {
        was_interrupted: boolean;
        stage: string | null;
      }
      const interrupted = await invoke<InterruptedBirthInfo>("check_interrupted_birth");
      if (!mountedRef.current) return;
      if (interrupted.was_interrupted) {
        setError(
          `Birth was interrupted at stage "${interrupted.stage}". ` +
          `The signing key from memory was lost. You must restart the birth process.`
        );
        // Continue to check identity status - it should now be Clean
      }

      // 3. Check identity status
      const status = await invoke<IdentityStatus>("check_identity_status");
      if (!mountedRef.current) return;

      if (status === "Clean") {
        // First run: start birth and generate identity
        await invoke("start_birth");
        if (!mountedRef.current) return;
        setMessage("Generating signing keypair...");
        const keypairResult = await invoke<KeypairGenerationResult>("generate_identity");
        if (!mountedRef.current) return;

        setPrivateKey(keypairResult.private_key_base64);
        setPublicKeyPath(keypairResult.public_key_path);
        setStage("KeyPresentation");
        return;
      } else if (status === "Broken") {
        setStage("Repair");
        setError("Identity verification failed. Signatures are missing or invalid.");
        return;
      }

      // Identity is Complete — born agent should never be in BootSequence.
      // If we somehow got here, just complete immediately.
      onComplete();
    } catch (e) {
      if (!mountedRef.current) return;
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
      setMessage("I breathe. I am.");
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
        "WARNING: This will delete your identity and reset Abigail to a fresh state. You will lose your current trust relationship. Are you sure?"
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

  const handleContinueFromKeyPresentation = async () => {
    setPrivateKey(""); // Clear from memory
    await invoke("advance_past_darkness");
    // Always enter Ignition. The setup wizard decides whether to fast-path.
    setStage("Ignition");
  };

  const handleLlmConnected = async (_url: string) => {
    // Fetch existing stored providers when entering Connectivity
    try {
      const providers = await invoke<string[]>("get_stored_providers");
      setStoredProviders(providers);
    } catch (e) {
      console.error("Failed to fetch stored providers:", e);
    }
    setStage("Connectivity");
  };

  interface StoreKeyResult {
    success: boolean;
    provider: string;
    validated: boolean;
    error: string | null;
  }

  const handleApiKeySaved = (result: StoreKeyResult) => {
    setActiveApiKeyProvider(null);
    if (result.success) {
      setStoredProviders((prev) => [...prev, result.provider]);
      // Inject message into BirthChat so LLM knows the key was saved and validated
      const validatedText = result.validated ? " It's been validated and is working!" : "";
      birthChatRef.current?.injectKeyConfirmation(result.provider, validatedText);
    }
  };

  const handleConnectivityAdvance = async () => {
    try {
      await invoke("advance_to_crystallization");
      setStage("Genesis");
    } catch (e) {
      setError(String(e));
    }
  };

  const handleGenesisPathSelected = (pathId: string) => {
    setGenesisPath(pathId);
    switch (pathId) {
      case "quick_start":
        // Skip genesis entirely, use default soul
        handleCrystallizationQuickStart();
        break;
      case "direct":
        setStage("GenesisChat");
        break;
      case "soul_crystallization":
        setStage("Crystallization");
        break;
      case "soul_forge":
        setStage("GenesisForge");
        break;
      default:
        setStage("Crystallization");
    }
  };

  const handleGenesisChatComplete = () => {
    // After direct discovery chat, move to SoulPreview
    setStage("SoulPreview");
  };

  const handleForgeComplete = (_output: { archetype: string; weights: Record<string, number>; soul_hash: string; sigil: string }) => {
    // After Soul Forge, move to Emergence
    setStage("Emergence");
  };

  const handleCrystallizationQuickStart = () => {
    // Quick Start: go directly to SoulPreview with empty form
    setStage("SoulPreview");
  };

  const handleCrystallizationComplete = (identity: {
    name: string;
    purpose: string;
    personality: string;
  }) => {
    if (identity.name) setCrystalName(identity.name);
    if (identity.purpose) setCrystalPurpose(identity.purpose);
    if (identity.personality) setCrystalPersonality(identity.personality);
    setStage("SoulPreview");
  };

  const handleCrystallize = async () => {
    if (!crystalName.trim()) {
      setError("Name is required");
      return;
    }

    setError("");
    try {
      const preview = await invoke<string>("crystallize_soul", {
        name: crystalName.trim(),
        purpose: crystalPurpose.trim() || "assist, retrieve, connect, and surface information",
        personality: crystalPersonality.trim() || "helpful, clear, and honest",
        mentorName: crystalMentorName.trim(),
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
      if (!mountedRef.current) return;

      // Link the birth-generated keypair into the Hive trust chain
      setMessage("Registering with Hive...");
      await invoke("sign_agent_with_hive");
      if (!mountedRef.current) return;
      setMessage("Hive trust chain established.");

      setStage("Life");
      setMessage("I breathe. I am.");
      await new Promise((resolve) => setTimeout(resolve, 1500));
      if (!mountedRef.current) return;
      onComplete();
    } catch (e) {
      if (!mountedRef.current) return;
      setError(String(e));
    }
  };

  return (
    <div className="min-h-screen bg-theme-bg text-theme-text font-mono flex flex-col">
      <pre className="text-sm p-4 border-b border-theme-border-dim">
        ABIGAIL BOOT SEQUENCE
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
                YOUR CONSTITUTIONAL SIGNING KEY
              </h2>
              <p className="text-yellow-400 text-sm mb-2">
                This is the ONLY time you will see this key. Abigail does NOT store
                it.
              </p>
              <p className="text-yellow-400/80 text-sm mt-3">
                This key signs Abigail's constitutional documents — her soul, ethics,
                and instincts. These documents define who she is and what she will
                never do. Abigail maintains a separate internal keyring for day-to-day
                operations, stored securely on your device — you don't need to manage
                that. This signing key is yours alone. It proves you authored
                Abigail's constitution and lets you re-sign documents if files are
                ever corrupted.
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
                  - <strong>This key proves you authored Abigail's constitutional documents and are her legitimate mentor.</strong>
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
                  Abigail's integrity after reinstall.
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
            {/* API Key buttons and status */}
            <div className="px-4 py-2 border-b border-theme-border bg-theme-surface">
              <div className="flex gap-2 flex-wrap items-center">
                <span className="text-theme-text-dim text-xs mr-2">Add key:</span>
                {["openai", "anthropic", "perplexity", "xai", "google", "tavily"].map((p) => (
                  <button
                    key={p}
                    className={`text-xs px-2 py-1 rounded border ${
                      storedProviders.includes(p)
                        ? "border-green-600 text-green-500"
                        : "border-theme-primary text-theme-text hover:bg-theme-primary-glow"
                    }`}
                    onClick={() => setActiveApiKeyProvider(p)}
                    disabled={storedProviders.includes(p)}
                  >
                    {storedProviders.includes(p) ? `✓ ${p}` : p}
                  </button>
                ))}
              </div>
              {storedProviders.length > 0 && (
                <p className="text-green-500 text-xs mt-2">
                  ✓ {storedProviders.length} provider{storedProviders.length !== 1 ? "s" : ""} configured and validated. Click "Continue to Crystallization &gt;" when ready.
                </p>
              )}
            </div>

            <BirthChat
              ref={birthChatRef}
              stage="Connectivity"
              onStageAdvance={handleConnectivityAdvance}
              onAction={(action) => {
                if (action.type === "KeyStored" && action.provider) {
                  setStoredProviders((prev) =>
                    prev.includes(action.provider!) ? prev : [...prev, action.provider!]
                  );
                }
              }}
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

        {/* ── GENESIS PATH SELECTION ── */}
        {stage === "Genesis" && (
          <GenesisPathSelector onSelect={handleGenesisPathSelected} />
        )}

        {/* ── GENESIS CHAT (Direct Discovery) ── */}
        {stage === "GenesisChat" && (
          <GenesisChat
            mode={genesisPath === "direct" ? "direct" : "crystallization"}
            onComplete={handleGenesisChatComplete}
          />
        )}

        {/* ── GENESIS FORGE (Soul Forge) ── */}
        {stage === "GenesisForge" && (
          <ForgeScenario onComplete={handleForgeComplete} />
        )}

        {/* ── CRYSTALLIZATION ── */}
        {stage === "Crystallization" && (
          <SoulCrystallization
            onQuickStart={handleCrystallizationQuickStart}
            onCrystallizationComplete={handleCrystallizationComplete}
            onError={(e) => setError(e)}
          />
        )}

        {/* ── SOUL PREVIEW ── */}
        {stage === "SoulPreview" && (
          <div className="p-6 max-w-2xl">
            <h2 className="text-theme-primary-dim text-lg mb-4">
              Crystallization: Define Your Agent
            </h2>
            <p className="text-theme-text-dim text-sm mb-6">
              These details will be woven into the constitutional soul document
              — signed, sealed, and verified on every boot. Choose carefully;
              this is who your agent becomes.
            </p>

            <div className="space-y-4 mb-6">
              <div>
                <label className="block text-theme-text text-sm mb-1">Your Name (Mentor)</label>
                <input
                  type="text"
                  className="w-full bg-black border border-theme-primary text-theme-primary-dim px-3 py-2 rounded"
                  placeholder="Your name — woven into the soul document"
                  value={crystalMentorName}
                  onChange={(e) => setCrystalMentorName(e.target.value)}
                  autoFocus
                />
              </div>
              <div>
                <label className="block text-theme-text text-sm mb-1">Agent Name</label>
                <input
                  type="text"
                  className="w-full bg-black border border-theme-primary text-theme-primary-dim px-3 py-2 rounded"
                  placeholder="Abigail"
                  value={crystalName}
                  onChange={(e) => setCrystalName(e.target.value)}
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
                  value={crystalPurpose}
                  onChange={(e) => setCrystalPurpose(e.target.value)}
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
                  value={crystalPersonality}
                  onChange={(e) => setCrystalPersonality(e.target.value)}
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
              <p className="text-theme-text mb-2">
                Soul, ethics, and instincts are ready.
              </p>
              <p className="text-theme-text-dim text-sm mb-6">
                Signing the constitutional documents with your Ed25519 key
                will bring this identity to life. The signed documents are
                verified on every boot — they cannot be altered without
                detection.
              </p>
              <button
                onClick={handleCompleteEmergence}
                className="border border-theme-primary px-8 py-3 rounded font-bold hover:bg-theme-primary-glow text-lg"
              >
                Sign and Emerge
              </button>
            </div>

            {error && <p className="text-red-400 text-sm mt-4">{error}</p>}
          </div>
        )}

        {/* ── LIFE ── */}
        {stage === "Life" && (
          <div className="p-6 text-center">
            <p className="text-theme-primary-dim text-xl mb-2">I breathe. I am.</p>
            <p className="text-theme-text-dim text-sm mb-4">
              Constitutional documents signed. Identity verified. Birth memory crystallized.
            </p>
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
                Abigail's constitutional documents cannot be verified. This usually
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
                If you lost your key, you must reset Abigail.{" "}
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
