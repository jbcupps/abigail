import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";

type Stage = "None" | "Starting" | "KeyPresentation" | "Verified" | "Life";

interface BootSequenceProps {
  onComplete: () => void;
}

interface StartupCheckResult {
  heartbeat_ok: boolean;
  verification_ok: boolean;
  error: string | null;
}

interface KeypairGenerationResult {
  private_key_base64: string;
  public_key_path: string;
  newly_generated: boolean;
}

export default function BootSequence({ onComplete }: BootSequenceProps) {
  const [stage, setStage] = useState<Stage>("None");
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [privateKey, setPrivateKey] = useState("");
  const [publicKeyPath, setPublicKeyPath] = useState("");
  const [keySaved, setKeySaved] = useState(false);
  const [copied, setCopied] = useState(false);

  const handleStart = async () => {
    setError("");
    setStage("Starting");
    setMessage("Initializing...");

    try {
      // 1. Initialize soul (copy templates, create internal keyring)
      await invoke("init_soul");
      setMessage("Checking for existing keypair...");

      // 2. Check if external keypair already exists
      const hasKeypair = await invoke<boolean>("has_external_keypair");
      
      if (!hasKeypair) {
        // First run: generate keypair and sign documents
        setMessage("Generating signing keypair...");
        const keypairResult = await invoke<KeypairGenerationResult>("generate_and_sign_constitutional");
        
        // Show the private key to the user
        setPrivateKey(keypairResult.private_key_base64);
        setPublicKeyPath(keypairResult.public_key_path);
        setStage("KeyPresentation");
        return; // Wait for user to acknowledge
      }
      
      // Keypair exists, continue with startup checks
      await continueAfterKeyPresentation();
    } catch (e) {
      setError(String(e));
      setStage("None");
    }
  };

  const continueAfterKeyPresentation = async () => {
    setStage("Starting");
    setMessage("Running startup checks...");
    
    try {
      // Run startup checks (heartbeat + signature verification)
      const result = await invoke<StartupCheckResult>("run_startup_checks");

      if (!result.heartbeat_ok) {
        setError(result.error || "LLM heartbeat failed. Is the local LLM server running?");
        setStage("None");
        return;
      }

      if (!result.verification_ok && result.error) {
        setError(result.error);
        setStage("None");
        return;
      }

      // Show "Abby is informed they're OK"
      setStage("Verified");
      setMessage("Integrity verified. Engaging...");

      // Start birth and skip to Life for MVP
      await invoke("start_birth");
      
      // Get docs path and run verify_crypto to advance past Darkness
      const docsPath = await invoke<string>("get_docs_path").catch(() => ".");
      await invoke("verify_crypto", { docsPath });
      
      // Skip email and model download for MVP
      await invoke("skip_to_life_for_mvp");

      // Complete birth
      await invoke("complete_birth");

      // Brief pause to show the "verified" message
      await new Promise((resolve) => setTimeout(resolve, 1000));

      setStage("Life");
      setMessage("I am awake.");

      // Another brief pause then complete
      await new Promise((resolve) => setTimeout(resolve, 500));
      onComplete();
    } catch (e) {
      setError(String(e));
      setStage("None");
    }
  };

  const handleCopyKey = async () => {
    try {
      await navigator.clipboard.writeText(privateKey);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Fallback for older browsers
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
    // Clear the private key from state (security)
    setPrivateKey("");
    continueAfterKeyPresentation();
  };

  return (
    <div className="min-h-screen bg-black text-green-500 font-mono p-6 overflow-auto">
      <pre className="text-sm">
        ABBY BOOT SEQUENCE
        ==================
      </pre>

      {stage === "None" && (
        <div>
          <p className="mb-4">Press Start to begin.</p>
          <button
            className="border border-green-500 px-4 py-2 rounded hover:bg-green-500/20"
            onClick={handleStart}
          >
            Start
          </button>
        </div>
      )}

      {stage === "Starting" && (
        <div>
          <p className="mb-4">{message}</p>
          <div className="animate-pulse">...</div>
        </div>
      )}

      {stage === "KeyPresentation" && (
        <div className="max-w-2xl">
          <div className="border border-yellow-500 bg-yellow-500/10 p-4 rounded mb-6">
            <h2 className="text-yellow-500 text-lg font-bold mb-2">
              CRITICAL: SAVE YOUR PRIVATE KEY
            </h2>
            <p className="text-yellow-400 text-sm mb-2">
              This is the ONLY time you will see this key. Abby does NOT store it.
            </p>
          </div>

          <div className="mb-6">
            <p className="text-sm mb-2 text-gray-400">Your Private Signing Key (Ed25519, Base64):</p>
            <div className="relative">
              <textarea
                readOnly
                value={privateKey}
                className="w-full bg-gray-900 border border-green-700 rounded p-3 text-green-300 font-mono text-sm resize-none"
                rows={3}
                onClick={(e) => (e.target as HTMLTextAreaElement).select()}
              />
              <button
                onClick={handleCopyKey}
                className="absolute top-2 right-2 px-2 py-1 text-xs border border-green-500 rounded hover:bg-green-500/20"
              >
                {copied ? "Copied!" : "Copy"}
              </button>
            </div>
          </div>

          <div className="mb-6 text-sm">
            <p className="text-gray-400 mb-1">Public key saved to:</p>
            <code className="text-green-300 text-xs break-all">{publicKeyPath}</code>
          </div>

          <div className="border border-red-700 bg-red-900/20 p-4 rounded mb-6">
            <h3 className="text-red-400 font-bold mb-2">SECURITY WARNINGS</h3>
            <ul className="text-red-300 text-sm space-y-2">
              <li>• <strong>This key proves you are Abby's legitimate mentor.</strong></li>
              <li>• <strong>Store it securely</strong> (password manager, encrypted drive, offline backup).</li>
              <li>• <strong>Never share this key</strong> with anyone or any service.</li>
              <li>• <strong>If you lose this key:</strong> You cannot re-verify Abby's integrity after reinstall.</li>
              <li>• <strong>If this key is compromised:</strong> Someone could create fake constitutional documents.</li>
            </ul>
          </div>

          <div className="mb-6">
            <label className="flex items-center gap-3 cursor-pointer">
              <input
                type="checkbox"
                checked={keySaved}
                onChange={(e) => setKeySaved(e.target.checked)}
                className="w-5 h-5 accent-green-500"
              />
              <span className="text-sm">
                I have saved my private key securely and understand I will not see it again.
              </span>
            </label>
          </div>

          <button
            disabled={!keySaved}
            onClick={handleContinueFromKeyPresentation}
            className={`px-6 py-3 rounded font-bold ${
              keySaved
                ? "border border-green-500 hover:bg-green-500/20 text-green-500"
                : "border border-gray-600 text-gray-600 cursor-not-allowed"
            }`}
          >
            Continue
          </button>
        </div>
      )}

      {stage === "Verified" && (
        <div>
          <p className="mb-4 text-green-400">{message}</p>
          <div className="animate-pulse">...</div>
        </div>
      )}

      {stage === "Life" && (
        <div>
          <p className="mb-4 text-green-400">{message}</p>
        </div>
      )}

      {error && (
        <div className="mt-4">
          <p className="text-red-400">{error}</p>
          <button
            className="border border-green-500 px-4 py-2 rounded hover:bg-green-500/20 mt-2"
            onClick={handleStart}
          >
            Retry
          </button>
        </div>
      )}
    </div>
  );
}
