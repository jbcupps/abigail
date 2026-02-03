import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

type Stage = "Darkness" | "Awakening" | "Cognition" | "Life" | "None";

interface BootSequenceProps {
  onComplete: () => void;
}

export default function BootSequence({ onComplete }: BootSequenceProps) {
  const [stage, setStage] = useState<Stage>("None");
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [email, setEmail] = useState({ address: "", imapHost: "mail.proton.me", imapPort: 993, smtpHost: "mail.proton.me", smtpPort: 587, password: "" });
  const [apiKey, setApiKey] = useState("");
  const [downloadProgress, setDownloadProgress] = useState<{ written: number; total: number | null } | null>(null);

  useEffect(() => {
    const unlisten = listen<{ written: number; total: number | null }>("download-progress", (e) => {
      setDownloadProgress(e.payload);
    });
    return () => {
      unlisten.then((u) => u());
    };
  }, []);

  const startBirth = async () => {
    setError("");
    try {
      await invoke("start_birth");
      const s = await invoke<string>("get_birth_stage");
      const m = await invoke<string>("get_birth_message");
      setStage(s as Stage);
      setMessage(m);
      if (s === "Darkness") {
        const docsPath = await invoke<string>("get_docs_path").catch(() => "");
        await invoke("verify_crypto", { docsPath: docsPath || "." });
        setStage("Awakening");
        setMessage("Configure Abby's email account.");
      }
    } catch (e) {
      setError(String(e));
    }
  };

  const verifyCrypto = async () => {
    setError("");
    try {
      const docsPath = await invoke<string>("get_docs_path").catch(() => "");
      await invoke("verify_crypto", { docsPath: docsPath || "." });
      const m = await invoke<string>("get_birth_message");
      setStage("Awakening");
      setMessage(m);
    } catch (e) {
      setError(String(e));
    }
  };

  const configureEmail = async () => {
    setError("");
    try {
      await invoke("configure_email", {
        address: email.address,
        imapHost: email.imapHost,
        imapPort: email.imapPort,
        smtpHost: email.smtpHost,
        smtpPort: email.smtpPort,
        password: email.password,
      });
      setStage("Cognition");
      setMessage("Loading the mind...");
    } catch (e) {
      setError(String(e));
    }
  };

  const downloadModel = async () => {
    setError("");
    setDownloadProgress({ written: 0, total: null });
    try {
      await invoke("download_model");
      setStage("Life");
      setMessage("I am awake.");
    } catch (e) {
      setError(String(e));
    } finally {
      setDownloadProgress(null);
    }
  };

  const setOpenAIKey = async () => {
    setError("");
    try {
      await invoke("set_api_key", { key: apiKey });
    } catch (e) {
      setError(String(e));
    }
  };

  const completeBirth = async () => {
    setError("");
    try {
      await invoke("complete_birth");
      onComplete();
    } catch (e) {
      setError(String(e));
    }
  };

  const handleStart = async () => {
    setError("");
    try {
      await invoke("init_soul");
      await startBirth();
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div className="min-h-screen bg-black text-green-500 font-mono p-6">
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
      {stage === "Darkness" && (
        <div>
          <p className="mb-4">{message}</p>
          <button
            className="border border-green-500 px-4 py-2 rounded hover:bg-green-500/20"
            onClick={verifyCrypto}
          >
            Verify integrity
          </button>
        </div>
      )}
      {stage === "Awakening" && (
        <div className="space-y-2 max-w-md">
          <p className="mb-4">{message}</p>
          <input
            type="text"
            placeholder="Email address"
            className="w-full bg-black border border-green-500 text-green-500 px-2 py-1 rounded"
            value={email.address}
            onChange={(e) => setEmail((x) => ({ ...x, address: e.target.value }))}
          />
          <input
            type="password"
            placeholder="Password"
            className="w-full bg-black border border-green-500 text-green-500 px-2 py-1 rounded"
            value={email.password}
            onChange={(e) => setEmail((x) => ({ ...x, password: e.target.value }))}
          />
          <button
            className="border border-green-500 px-4 py-2 rounded hover:bg-green-500/20"
            onClick={configureEmail}
          >
            Save email
          </button>
        </div>
      )}
      {stage === "Cognition" && (
        <div className="space-y-2 max-w-md">
          <p className="mb-4">{message}</p>
          <button
            className="border border-green-500 px-4 py-2 rounded hover:bg-green-500/20 mr-2"
            onClick={downloadModel}
          >
            Download model
          </button>
          <input
            type="password"
            placeholder="OpenAI API key (optional)"
            className="w-full bg-black border border-green-500 text-green-500 px-2 py-1 rounded"
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
          />
          <button
            className="border border-green-500 px-4 py-2 rounded hover:bg-green-500/20"
            onClick={setOpenAIKey}
          >
            Set API key
          </button>
          {downloadProgress && (
            <p>Downloaded: {downloadProgress.written} bytes {downloadProgress.total != null ? `/ ${downloadProgress.total}` : ""}</p>
          )}
          <button
            className="border border-green-500 px-4 py-2 rounded hover:bg-green-500/20 mt-4"
            onClick={() => { setStage("Life"); setMessage("I am awake."); }}
          >
            Continue
          </button>
        </div>
      )}
      {stage === "Life" && (
        <div>
          <p className="mb-4">{message}</p>
          <button
            className="border border-green-500 px-4 py-2 rounded hover:bg-green-500/20"
            onClick={completeBirth}
          >
            Complete birth
          </button>
        </div>
      )}
      {error && <p className="text-red-400 mt-4">{error}</p>}
    </div>
  );
}
