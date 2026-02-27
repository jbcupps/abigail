import { invoke } from "@tauri-apps/api/core";
import { useState, useEffect, useRef } from "react";
import LlmSetupPanel from "./LlmSetupPanel";
import BirthChat, { BirthChatHandle } from "./BirthChat";
import ApiKeyModal from "./ApiKeyModal";
import SoulCrystallization from "./SoulCrystallization";
import GenesisPathSelector from "./GenesisPathSelector";
import GenesisChat from "./GenesisChat";
import ForgeScenario from "./ForgeScenario";
import type { CrystallizationIdentityDraft } from "./crystallizationPaths";
import CrystallizationPathFast from "./CrystallizationPathFast";
import CrystallizationPathDialog from "./CrystallizationPathDialog";
import CrystallizationPathImage from "./CrystallizationPathImage";
import CrystallizationPathPsychQuestions from "./CrystallizationPathPsychQuestions";
import CrystallizationPathTemplateEdit from "./CrystallizationPathTemplateEdit";

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

/** Wrap an invoke call with a timeout (ms). Rejects if the command doesn't return in time. */
function invokeWithTimeout<T>(cmd: string, args?: Record<string, unknown>, ms = 15000): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    let settled = false;
    const timer = setTimeout(() => {
      if (!settled) {
        settled = true;
        reject(new Error(`Command "${cmd}" timed out after ${ms / 1000}s`));
      }
    }, ms);

    invoke<T>(cmd, args)
      .then((v) => {
        if (!settled) { settled = true; clearTimeout(timer); resolve(v); }
      })
      .catch((e) => {
        if (!settled) { settled = true; clearTimeout(timer); reject(e); }
      });
  });
}

export default function BootSequence({ onComplete }: BootSequenceProps) {
  const [stage, setStage] = useState<Stage>("Darkness");
  const [message, setMessage] = useState("");
  const [bootStep, setBootStep] = useState("");
  const [error, setError] = useState("");
  const [timedOut, setTimedOut] = useState(false);
  const [privateKey, setPrivateKey] = useState("");
  const [publicKeyPath, setPublicKeyPath] = useState("");
  const [keySaved, setKeySaved] = useState(false);
  const [copied, setCopied] = useState(false);
  const [autoSavedPath, setAutoSavedPath] = useState("");
  const [repairKey, setRepairKey] = useState("");
  const [activeApiKeyProvider, setActiveApiKeyProvider] = useState<string | null>(null);
  const [storedProviders, setStoredProviders] = useState<string[]>([]);
  const [soulPreview, setSoulPreview] = useState("");
  const [crystalName, setCrystalName] = useState("");
  const [crystalPurpose, setCrystalPurpose] = useState("");
  const [crystalPersonality, setCrystalPersonality] = useState("");
  const [crystalMentorName, setCrystalMentorName] = useState("");
  const [crystalPrimaryColor, setCrystalPrimaryColor] = useState("#00ffcc");
  const [crystalAvatarUrl, setCrystalAvatarUrl] = useState("");
  const [genesisPath, setGenesisPath] = useState<string | null>(null);
  const [visualizing, setVisualizing] = useState(false);
  const [cliDetections, setCliDetections] = useState<Array<{
    provider_name: string; binary: string; on_path: boolean;
    is_official: boolean; is_authenticated: boolean;
    version: string | null; auth_hint: string | null;
  }>>([]);

  // Ref to BirthChat for injecting key confirmations
  const birthChatRef = useRef<BirthChatHandle>(null);
  const mountedRef = useRef(true);

  // Auto-start boot sequence on mount
  // NOTE: mountedRef must be reset to true here because React.StrictMode
  // (dev only) runs effects twice: mount → cleanup (sets false) → remount.
  // Without this reset, both handleStart() calls abort early.
  useEffect(() => {
    mountedRef.current = true;
    handleStart();
    return () => { mountedRef.current = false; };
  }, []);

  const handleStart = async () => {
    setError("");
    setTimedOut(false);
    setStage("Darkness");
      setMessage("Preparing secure environment and validating first-run prerequisites...");
    setBootStep("init_soul");

    try {
      // 1. Initialize soul (copy templates, create internal keyring)
      await invokeWithTimeout("init_soul");
      if (!mountedRef.current) return;

      // 2. Check for interrupted birth (closed app mid-way through first run)
      setBootStep("check_interrupted_birth");
      setMessage("Checking for interrupted birth...");
      interface InterruptedBirthInfo {
        was_interrupted: boolean;
        stage: string | null;
      }
      const interrupted = await invokeWithTimeout<InterruptedBirthInfo>("check_interrupted_birth");
      if (!mountedRef.current) return;
      if (interrupted.was_interrupted) {
        setError(
          `Birth was interrupted at stage "${interrupted.stage}". ` +
          `The signing key from memory was lost. You must restart the birth process.`
        );
        // Set a special flag or just rely on the error UI showing a retry
        return; 
      }

      // 3. Check identity status
      setBootStep("check_identity_status");
      setMessage("Checking identity status...");
      const status = await invokeWithTimeout<IdentityStatus>("check_identity_status");
      if (!mountedRef.current) return;

      if (status === "Clean") {
        // First run: start birth and generate identity
        setBootStep("start_birth");
        setMessage("Starting first-run initialization...");
        await invokeWithTimeout("start_birth");
        if (!mountedRef.current) return;

        setBootStep("generate_identity");
        setMessage("Generating constitutional signing keypair...");
        const keypairResult = await invokeWithTimeout<KeypairGenerationResult>("generate_identity");
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
      const errMsg = String(e);
      const isTimeout = errMsg.includes("timed out");
      setTimedOut(isTimeout);
      setError(errMsg);
      setStage("Darkness");
    }
  };

  const handleSkipInteractive = async () => {
    try {
      if (stage === "Ignition") {
        // Cloud-First path: jump to Connectivity to enter keys
        await invoke("advance_to_connectivity");
        setStage("Connectivity");
        return;
      }

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

  const handleResetBirth = async () => {
    try {
      await invoke("reset_birth");
      handleStart();
    } catch (e) {
      setError(String(e));
    }
  };

  const handleAutoSaveKey = async () => {
    try {
      const path = await invoke<string>("save_recovery_key", { privateKey });
      setAutoSavedPath(path);
      setKeySaved(true);
    } catch (e) {
      setError(String(e));
    }
  };

  const handleContinueFromKeyPresentation = async () => {
    setPrivateKey(""); // Clear from memory
    await invoke("advance_past_darkness");
    // Always enter Ignition. The setup wizard decides whether to fast-path.
    setStage("Ignition");
  };

  const handleLlmConnected = async (_url: string) => {
    try {
      await invoke("advance_to_connectivity");
      const [providers, cliResults] = await Promise.all([
        invoke<string[]>("get_stored_providers"),
        invoke<typeof cliDetections>("detect_cli_providers_full"),
      ]);
      setStoredProviders(providers);
      setCliDetections(cliResults);
    } catch (e) {
      console.error("Failed to advance to connectivity or fetch providers:", e);
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
    setError("");
    setActiveApiKeyProvider(null);
    if (result && result.success) {
      setStoredProviders((prev) => {
        const next = [...prev];
        if (!next.includes(result.provider)) {
          next.push(result.provider);
        }
        
        // Auto-add linked providers
        const mapping: Record<string, string> = {
          "openai": "codex-cli", "anthropic": "claude-cli", "google": "gemini-cli", "xai": "grok-cli",
          "codex-cli": "openai", "claude-cli": "anthropic", "gemini-cli": "google", "grok-cli": "xai"
        };
        const linked = mapping[result.provider];
        if (linked && !next.includes(linked)) {
          next.push(linked);
        }
        return next;
      });
      
      // Inject message into BirthChat with safety check
      if (birthChatRef.current) {
        const validatedText = result.validated ? " It's been validated and is working!" : "";
        birthChatRef.current.injectKeyConfirmation(result.provider, validatedText);
      }
    }
  };

  const handleConnectivityAdvance = async () => {
    const hasAuthedCli = cliDetections.some(d => d.on_path && d.is_official && d.is_authenticated);
    if (storedProviders.length === 0 && !hasAuthedCli) {
      setError("At least one provider must be configured before crystallization can begin.");
      return;
    }
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
      case "fast_template":
      case "guided_dialog":
      case "image_archetype":
      case "psych_moral":
      case "editable_template":
        setStage("Crystallization");
        break;
      case "direct":
        setStage("GenesisChat");
        break;
      case "soul_forge":
        setStage("GenesisForge");
        break;
      default:
        setStage("Crystallization");
    }
  };

  const handlePathComplete = (identity: CrystallizationIdentityDraft) => {
    if (identity.name) setCrystalName(identity.name);
    if (identity.purpose) setCrystalPurpose(identity.purpose);
    if (identity.personality) setCrystalPersonality(identity.personality);
    if (identity.primaryColor) setCrystalPrimaryColor(identity.primaryColor);
    if (identity.avatarUrl) setCrystalAvatarUrl(identity.avatarUrl);
    setStage("SoulPreview");
  };

  const handleGenesisChatComplete = async () => {
    // After direct discovery chat, extract identity and move to SoulPreview
    try {
      interface CrystallizationIdentity {
        name?: string;
        purpose?: string;
        personality?: string;
        primary_color?: string;
        avatar_url?: string;
      }
      const identity = await invoke<CrystallizationIdentity>("extract_crystallization_identity");
      if (identity.name) setCrystalName(identity.name);
      if (identity.purpose) setCrystalPurpose(identity.purpose);
      if (identity.personality) setCrystalPersonality(identity.personality);
      if (identity.primary_color) setCrystalPrimaryColor(identity.primary_color);
      if (identity.avatar_url) setCrystalAvatarUrl(identity.avatar_url);
    } catch (e) {
      console.warn("Could not extract identity from GenesisChat:", e);
    }
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
    primaryColor: string;
    avatarUrl: string;
  }) => {
    if (identity.name) setCrystalName(identity.name);
    if (identity.purpose) setCrystalPurpose(identity.purpose);
    if (identity.personality) setCrystalPersonality(identity.personality);
    if (identity.primaryColor) setCrystalPrimaryColor(identity.primaryColor);
    if (identity.avatarUrl) setCrystalAvatarUrl(identity.avatarUrl);
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
        primaryColor: crystalPrimaryColor,
        avatarUrl: crystalAvatarUrl,
      });
      setSoulPreview(preview);
      setStage("Emergence");
    } catch (e) {
      setError(String(e));
    }
  };

  const handleVisualizeSoul = async () => {
    setVisualizing(true);
    try {
      // Use Ego LLM to propose a visual identity based on current soul details
      const proposal = await invoke<{ primary_color: string; avatar_url?: string }>("propose_entity_visuals", {
        name: crystalName,
        personality: crystalPersonality,
        purpose: crystalPurpose
      });
      if (proposal.primary_color) setCrystalPrimaryColor(proposal.primary_color);
      if (proposal.avatar_url) setCrystalAvatarUrl(proposal.avatar_url);
    } catch (e) {
      console.warn("Failed to propose visuals:", e);
    } finally {
      setVisualizing(false);
    }
  };

  const handleCompleteEmergence = async () => {
    setMessage("Ceremony step 1/3: Signing constitutional documents...");
    try {
      await invoke("complete_emergence");
      if (!mountedRef.current) return;

      // Link the birth-generated keypair into the Hive trust chain
      setMessage("Ceremony step 2/3: Registering with Hive trust chain...");
      await invoke("sign_agent_with_hive");
      if (!mountedRef.current) return;
      setMessage("Ceremony step 3/3: Finalizing emergence.");

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
      <div className="px-4 py-3 border-b border-theme-border-dim bg-theme-bg-elevated">
        <pre className="text-sm text-theme-primary">
          ABIGAIL BOOT SEQUENCE
          ==================
        </pre>
      </div>

      <div className="flex-1 overflow-auto">
        {/* ── DARKNESS ── */}
        {stage === "Darkness" && !error && (
          <div className="p-6">
            <p className="mb-2">{message || "Preparing to start..."}</p>
            {bootStep && (
              <p className="text-theme-text-dim text-xs mb-2">Step: {bootStep}</p>
            )}
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
              <p className="text-sm mb-2 text-theme-text-dim">
                Your Private Signing Key (Ed25519, Base64):
              </p>
              <div className="relative">
                <textarea
                  readOnly
                  value={privateKey}
                  className="w-full bg-theme-bg-inset border border-theme-primary-faint rounded p-3 text-theme-text-bright font-mono text-sm resize-none"
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
              <button
                onClick={handleAutoSaveKey}
                className="mt-3 text-xs border border-theme-primary-faint px-3 py-1.5 rounded hover:bg-theme-surface flex items-center gap-2"
              >
                <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z"></path><polyline points="17 21 17 13 7 13 7 21"></polyline><polyline points="7 3 7 8 15 8"></polyline></svg>
                Auto-save to Documents
              </button>
              {autoSavedPath && (
                <p className="mt-2 text-[10px] text-green-500 bg-green-950/20 p-2 rounded border border-green-900">
                  ✓ Saved to: <span className="select-all">{autoSavedPath}</span>
                </p>
              )}
            </div>

            <div className="mb-6 text-sm">
              <p className="text-theme-text-dim mb-1">Public key saved to:</p>
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
                  : "border border-theme-border-dim text-theme-text-dim cursor-not-allowed"
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
          <div className="flex flex-col h-full bg-theme-bg" style={{ minHeight: "80vh" }}>
            {/* Command Center Dashboard */}
            <div className="p-6 border-b border-theme-border-dim bg-theme-bg-elevated">
              <div className="flex justify-between items-center mb-6">
                <div>
                  <h2 className="text-theme-primary text-xl font-bold tracking-tighter">CONNECTIVITY COMMAND CENTER</h2>
                  <p className="text-theme-text-dim text-xs">Establish trust with cloud intelligence providers.</p>
                </div>
                <div className="text-right">
                  <div className="text-[10px] text-theme-text-dim uppercase mb-1 font-mono">Overall Linkage</div>
                  <div className="w-32 h-2 bg-theme-bg-inset rounded-full overflow-hidden border border-theme-border-dim">
                    <div 
                      className="h-full bg-theme-primary transition-all duration-1000" 
                      style={{ width: `${Math.min(100, (storedProviders.length / 6) * 100)}%` }} 
                    />
                  </div>
                </div>
              </div>

              <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
                {/* Core Providers */}
                <div className="space-y-3">
                  <h3 className="text-[10px] text-theme-primary-dim font-bold uppercase tracking-widest border-b border-theme-border-dim pb-1">Core Cloud Minds</h3>
                  <div className="grid grid-cols-2 gap-2">
                    {["openai", "anthropic", "perplexity", "xai", "google", "tavily"].map((p) => (
                      <button
                        key={p}
                        className={`text-left px-3 py-2 rounded border transition-all ${
                          storedProviders.includes(p)
                            ? "border-green-600 bg-green-950/20 text-green-500"
                            : "border-theme-border-dim bg-theme-bg-inset text-theme-text-dim hover:border-theme-primary hover:text-theme-text"
                        }`}
                        onClick={() => {
                          setError("");
                          setActiveApiKeyProvider(p);
                        }}
                        disabled={storedProviders.includes(p)}
                      >
                        <div className="flex justify-between items-center">
                          <span className="text-xs font-bold uppercase">{p}</span>
                          {storedProviders.includes(p) && <span className="text-[10px]">✓</span>}
                        </div>
                        <div className="mt-1.5 w-full h-1 bg-black/20 rounded-full overflow-hidden">
                          <div className={`h-full transition-all duration-500 ${storedProviders.includes(p) ? "bg-green-500 w-full" : "bg-theme-primary-faint w-0"}`} />
                        </div>
                      </button>
                    ))}
                  </div>
                </div>

                {/* CLI & Tooling */}
                <div className="space-y-3">
                  <h3 className="text-[10px] text-theme-text-dim font-bold uppercase tracking-widest border-b border-theme-border-dim pb-1">External CLI Tools</h3>
                  <div className="grid grid-cols-2 gap-2">
                    {["claude-cli", "gemini-cli", "codex-cli", "grok-cli"].map((p) => {
                      const det = cliDetections.find(d => d.provider_name === p);
                      const active = storedProviders.includes(p);
                      const authed = det?.on_path && det?.is_official && det?.is_authenticated;
                      const detected = det?.on_path ?? false;
                      return (
                        <button
                          key={p}
                          className={`text-left px-3 py-2 rounded border transition-all ${
                            active
                              ? "border-green-600 bg-green-950/20 text-green-500"
                              : authed
                                ? "border-green-700/50 bg-green-950/10 text-green-400 hover:border-green-600"
                                : "border-theme-border-dim bg-theme-bg-inset text-theme-text-dim hover:border-theme-primary hover:text-theme-text"
                          }`}
                          onClick={() => {
                            setError("");
                            if (authed && !active) {
                              invoke("use_stored_provider", { provider: p }).then(() => {
                                setStoredProviders(prev => prev.includes(p) ? prev : [...prev, p]);
                              }).catch(e => setError(String(e)));
                            } else {
                              setActiveApiKeyProvider(p);
                            }
                          }}
                          disabled={active}
                        >
                          <div className="flex justify-between items-center">
                            <span className="text-xs font-bold uppercase">{p.replace("-cli", "")}</span>
                            <span className="text-[10px]">
                              {active ? "✓" : authed ? "Authed" : detected ? "No Auth" : ""}
                            </span>
                          </div>
                          <div className="mt-1.5 w-full h-1 bg-black/20 rounded-full overflow-hidden">
                            <div className={`h-full transition-all duration-500 ${active ? "bg-green-500 w-full" : authed ? "bg-green-700 w-3/4" : "bg-theme-primary-faint w-0"}`} />
                          </div>
                        </button>
                      );
                    })}
                  </div>
                </div>
              </div>
              
              {storedProviders.length > 0 && (
                <div className="mt-6 flex justify-center">
                  <button
                    className="px-8 py-2 bg-theme-primary text-theme-bg font-bold rounded-full hover:bg-theme-text transition-colors flex items-center gap-2 text-sm"
                    onClick={handleConnectivityAdvance}
                  >
                    ESTABLISH LINKAGE &rsaquo;
                  </button>
                </div>
              )}
            </div>

            <div className="flex-1 min-h-0 bg-theme-bg-inset">
              <BirthChat
                ref={birthChatRef}
                stage="Connectivity"
                onStageAdvance={handleConnectivityAdvance}
                onAction={(action) => {
                  if (action.type === "KeyStored" && action.provider) {
                    setStoredProviders((prev) => {
                      const newProviders = [...prev];
                      if (!newProviders.includes(action.provider!)) {
                        newProviders.push(action.provider!);
                      }
                      // Also auto-add the linked provider for UI consistency
                      const mapping: Record<string, string> = {
                        "openai": "codex-cli", "anthropic": "claude-cli", "google": "gemini-cli", "xai": "grok-cli",
                        "codex-cli": "openai", "claude-cli": "anthropic", "gemini-cli": "google", "grok-cli": "xai"
                      };
                      const linked = mapping[action.provider!];
                      if (linked && !newProviders.includes(linked)) {
                        newProviders.push(linked);
                      }
                      return newProviders;
                    });
                  }
                }}
              />
            </div>

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
          <>
            {genesisPath === "fast_template" && (
              <CrystallizationPathFast onComplete={handlePathComplete} />
            )}
            {genesisPath === "guided_dialog" && (
              <CrystallizationPathDialog onComplete={handlePathComplete} />
            )}
            {genesisPath === "image_archetype" && (
              <CrystallizationPathImage onComplete={handlePathComplete} />
            )}
            {genesisPath === "psych_moral" && (
              <CrystallizationPathPsychQuestions onComplete={handlePathComplete} />
            )}
            {genesisPath === "editable_template" && (
              <CrystallizationPathTemplateEdit onComplete={handlePathComplete} />
            )}
            {(!genesisPath || genesisPath === "soul_crystallization" || genesisPath === "quick_start") && (
              <SoulCrystallization
                onQuickStart={handleCrystallizationQuickStart}
                onCrystallizationComplete={handleCrystallizationComplete}
                onError={(e) => setError(e)}
              />
            )}
          </>
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
                  className="w-full bg-theme-input-bg border border-theme-border-dim text-theme-primary-dim px-3 py-2 rounded focus:border-theme-primary focus:ring-1 focus:ring-theme-focus-ring"
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
                  className="w-full bg-theme-input-bg border border-theme-border-dim text-theme-primary-dim px-3 py-2 rounded focus:border-theme-primary focus:ring-1 focus:ring-theme-focus-ring"
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
                  className="w-full bg-theme-input-bg border border-theme-border-dim text-theme-primary-dim px-3 py-2 rounded focus:border-theme-primary focus:ring-1 focus:ring-theme-focus-ring"
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
                  className="w-full bg-theme-input-bg border border-theme-border-dim text-theme-primary-dim px-3 py-2 rounded focus:border-theme-primary focus:ring-1 focus:ring-theme-focus-ring"
                  placeholder="helpful, clear, and honest"
                  value={crystalPersonality}
                  onChange={(e) => setCrystalPersonality(e.target.value)}
                />
              </div>

              {/* ── Visual Identity ── */}
              <div className="pt-4 border-t border-theme-border-dim">
                <div className="flex items-center justify-between mb-3">
                  <h3 className="text-theme-text text-sm font-bold uppercase tracking-widest">Visual Identity</h3>
                  <button 
                    onClick={handleVisualizeSoul}
                    disabled={visualizing}
                    className="text-[10px] border border-theme-primary px-2 py-1 rounded hover:bg-theme-primary-glow disabled:opacity-50"
                  >
                    {visualizing ? "Visualizing..." : "Visualize Your Soul"}
                  </button>
                </div>
                
                <div className="flex gap-4 items-start">
                  <div className="flex flex-col gap-2">
                    <label className="text-[10px] text-theme-text-dim uppercase">Accent Color</label>
                    <div className="flex items-center gap-2">
                      <input
                        type="color"
                        value={crystalPrimaryColor}
                        onChange={(e) => setCrystalPrimaryColor(e.target.value)}
                        className="w-10 h-10 bg-transparent border-none cursor-pointer"
                      />
                      <input
                        type="text"
                        value={crystalPrimaryColor}
                        onChange={(e) => setCrystalPrimaryColor(e.target.value)}
                        className="bg-theme-input-bg border border-theme-border-dim text-theme-text px-2 py-1 rounded text-xs w-20 font-mono"
                      />
                    </div>
                  </div>
                  
                  <div className="flex-1 flex flex-col gap-2">
                    <label className="text-[10px] text-theme-text-dim uppercase">Avatar URL (Optional)</label>
                    <input
                      type="text"
                      value={crystalAvatarUrl}
                      onChange={(e) => setCrystalAvatarUrl(e.target.value)}
                      placeholder="https://... or data:image/..."
                      className="w-full bg-theme-input-bg border border-theme-border-dim text-theme-text px-3 py-2 rounded text-xs focus:border-theme-primary focus:outline-none"
                    />
                  </div>
                </div>
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
                Constitutional artifacts are prepared.
              </p>
              <p className="text-theme-text-dim text-sm mb-6">
                Final emergence will sign soul, ethics, and instincts with your Ed25519 key.
                Each milestone is recorded and verified at startup so integrity drift is always visible.
              </p>
              <button
                onClick={handleCompleteEmergence}
                className="border border-theme-primary px-8 py-3 rounded font-bold hover:bg-theme-primary-glow text-lg"
              >
                Begin Emergence Ceremony
              </button>
            </div>

            {error && <p className="text-red-400 text-sm mt-4">{error}</p>}
          </div>
        )}

        {/* ── LIFE ── */}
        {stage === "Life" && (
          <div className="p-6 text-center">
            <p className="text-theme-primary-dim text-xl mb-2">Emergence complete. I breathe. I am.</p>
            <p className="text-theme-text-dim text-sm mb-4">
              Constitutional documents signed. Trust chain established. Birth memory crystallized.
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
              <p className="text-theme-text-dim text-sm">
                Abigail's constitutional documents cannot be verified. This usually
                happens if files were corrupted or tampered with.
              </p>
            </div>

            <div className="mb-8">
              <h3 className="text-theme-text font-bold mb-2">
                Option 1: Recover Identity
              </h3>
              <p className="text-sm text-theme-text-dim mb-2">
                If you have your <strong>Private Key</strong> (saved from first
                run), enter it below to re-sign the documents.
              </p>
              <textarea
                value={repairKey}
                onChange={(e) => setRepairKey(e.target.value)}
                placeholder="Paste your private key here..."
                className="w-full bg-theme-bg-inset border border-theme-primary-faint rounded p-3 text-theme-text-bright font-mono text-sm resize-none mb-2"
                rows={3}
              />
              <button
                onClick={handleRepair}
                disabled={!repairKey.trim()}
                className={`px-4 py-2 rounded font-bold text-sm ${
                  repairKey.trim()
                    ? "bg-theme-surface border border-theme-primary text-theme-text hover:bg-theme-primary-glow"
                    : "bg-theme-bg-inset border border-theme-border-dim text-theme-text-dim cursor-not-allowed"
                }`}
              >
                Recover Identity
              </button>
            </div>

            <div className="border-t border-theme-border-dim pt-6">
              <h3 className="text-red-400 font-bold mb-2">
                Option 2: Hard Reset
              </h3>
              <p className="text-sm text-theme-text-dim mb-4">
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
            <div className="flex gap-2 mt-2">
              <button
                className="border border-theme-primary px-4 py-2 rounded hover:bg-theme-primary-glow"
                onClick={handleStart}
              >
                Retry
              </button>
              <button
                className="border border-theme-danger text-red-400 px-4 py-2 rounded hover:bg-red-950/20"
                onClick={handleResetBirth}
              >
                Fresh Start
              </button>
            </div>
          </div>
        )}

        {error && stage === "Darkness" && (
          <div className="p-6">
            <p className="text-red-400 mb-2">{error}</p>
            {timedOut && bootStep && (
              <p className="text-yellow-500 text-xs mb-4">
                The boot sequence stalled at step &quot;{bootStep}&quot;.
                Check the Rust console for diagnostics. You can retry or skip to
                start fresh.
              </p>
            )}
            <div className="flex gap-3">
              <button
                className="border border-theme-primary px-4 py-2 rounded hover:bg-theme-primary-glow"
                onClick={handleStart}
              >
                Retry
              </button>
              {timedOut && (
                <button
                  className="border border-yellow-600 text-yellow-500 px-4 py-2 rounded hover:bg-yellow-600/20"
                  onClick={handleSkipInteractive}
                >
                  Skip to defaults
                </button>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
