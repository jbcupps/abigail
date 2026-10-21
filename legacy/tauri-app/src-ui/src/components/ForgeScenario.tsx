import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

interface ForgeChoice {
  id: string;
  label: string;
  description: string;
}

interface ForgeScenarioInfo {
  id: string;
  title: string;
  description: string;
  choices: ForgeChoice[];
}

interface SoulOutput {
  archetype: string;
  weights: { deontology: number; teleology: number; areteology: number; welfare: number };
  soul_hash: string;
  sigil: string;
}

interface ToolOutput {
  success: boolean;
  data: Record<string, unknown> | null;
  error: string | null;
}

interface Props {
  onComplete: (output: SoulOutput) => void;
}

export default function ForgeScenario({ onComplete }: Props) {
  const [scenarios, setScenarios] = useState<ForgeScenarioInfo[]>([]);
  const [currentIndex, setCurrentIndex] = useState(0);
  const [selections, setSelections] = useState<Record<string, string>>({});
  const [soulOutput, setSoulOutput] = useState<SoulOutput | null>(null);
  const [loading, setLoading] = useState(false);
  const [testResult, setTestResult] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);

  useEffect(() => {
    invoke<ForgeScenarioInfo[]>("get_forge_scenarios")
      .then(setScenarios)
      .catch(console.error);
  }, []);

  const current = scenarios[currentIndex];

  const handleChoice = (scenarioId: string, choiceId: string) => {
    setSelections((prev) => ({ ...prev, [scenarioId]: choiceId }));
  };

  const handleNext = () => {
    if (currentIndex < scenarios.length - 1) {
      setCurrentIndex(currentIndex + 1);
    }
  };

  const handleBack = () => {
    if (currentIndex > 0) {
      setCurrentIndex(currentIndex - 1);
    }
  };

  const handleCrystallize = async () => {
    setLoading(true);
    try {
      // Send all selections to backend
      const choiceIds = scenarios.map((s) => selections[s.id]).filter(Boolean);
      const output = await invoke<SoulOutput>("crystallize_forge", { choices: choiceIds });
      setSoulOutput(output);
    } catch (e) {
      console.error("Failed to crystallize:", e);
    } finally {
      setLoading(false);
    }
  };

  const allSelected = scenarios.length > 0 && scenarios.every((s) => selections[s.id]);

  const handleTestAgent = async () => {
    setTesting(true);
    setTestResult(null);
    try {
      const output = await invoke<ToolOutput>("execute_tool", {
        skillId: "com.abigail.skills.shell",
        toolName: "execute",
        params: { command: "echo Abigail skill system is operational.", timeout_ms: 5000 },
      });
      if (output.success) {
        const stdout = output.data?.formatted ?? output.data?.stdout ?? "OK";
        setTestResult(`${stdout}`);
      } else {
        setTestResult(`Error: ${output.error ?? "unknown"}`);
      }
    } catch (e) {
      setTestResult(`Skill test failed: ${e}`);
    } finally {
      setTesting(false);
    }
  };

  if (soulOutput) {
    return (
      <div className="flex flex-col items-center gap-6 p-8 max-w-2xl mx-auto">
        <h2 className="text-2xl font-bold text-theme-text-bright">Soul Forged</h2>
        <div className="bg-theme-bg-elevated rounded-lg p-6 w-full text-center">
          <p className="text-theme-text-dim text-sm mb-2">Archetype</p>
          <h3 className="text-xl font-bold text-theme-info mb-4">{soulOutput.archetype}</h3>

          <div className="grid grid-cols-2 gap-3 mb-4">
            <div className="bg-theme-surface rounded p-2">
              <p className="text-xs text-theme-text-dim">Deontology</p>
              <p className="text-theme-text-bright font-mono">{(soulOutput.weights.deontology * 100).toFixed(0)}%</p>
            </div>
            <div className="bg-theme-surface rounded p-2">
              <p className="text-xs text-theme-text-dim">Teleology</p>
              <p className="text-theme-text-bright font-mono">{(soulOutput.weights.teleology * 100).toFixed(0)}%</p>
            </div>
            <div className="bg-theme-surface rounded p-2">
              <p className="text-xs text-theme-text-dim">Areteology</p>
              <p className="text-theme-text-bright font-mono">{(soulOutput.weights.areteology * 100).toFixed(0)}%</p>
            </div>
            <div className="bg-theme-surface rounded p-2">
              <p className="text-xs text-theme-text-dim">Welfare</p>
              <p className="text-theme-text-bright font-mono">{(soulOutput.weights.welfare * 100).toFixed(0)}%</p>
            </div>
          </div>

          <pre className="text-theme-success text-xs font-mono whitespace-pre mb-4">{soulOutput.sigil}</pre>

          <p className="text-xs text-theme-text-dim font-mono break-all">
            Hash: {soulOutput.soul_hash}
          </p>
        </div>

        {testResult && (
          <div className="bg-theme-bg-elevated border border-theme-border-dim rounded-lg p-4 w-full">
            <p className="text-xs text-theme-text-dim mb-1">Skill Test Result</p>
            <pre className="text-theme-success text-sm font-mono whitespace-pre-wrap">{testResult}</pre>
          </div>
        )}

        <div className="flex gap-3">
          <button
            className="border border-theme-border-dim hover:border-theme-primary text-theme-text-dim hover:text-theme-text-bright transition-colors px-6 py-3 rounded-lg disabled:opacity-50"
            onClick={handleTestAgent}
            disabled={testing}
          >
            {testing ? "Testing..." : "Test Agent"}
          </button>
          <button
            className="border border-theme-primary text-theme-primary hover:bg-theme-primary-glow transition-colors px-8 py-3 rounded-lg"
            onClick={() => onComplete(soulOutput)}
          >
            Accept Soul
          </button>
        </div>
      </div>
    );
  }

  if (scenarios.length === 0) {
    return (
      <div className="flex items-center justify-center p-8">
        <p className="text-theme-text-dim animate-pulse">Loading scenarios...</p>
      </div>
    );
  }

  return (
    <div className="flex flex-col items-center gap-6 p-8 max-w-2xl mx-auto">
      <h2 className="text-2xl font-bold text-theme-text-bright">Soul Forge</h2>
      <p className="text-theme-text-dim text-center text-sm">
        Scenario {currentIndex + 1} of {scenarios.length}
      </p>

      {current && (
        <div className="bg-theme-bg-elevated rounded-lg p-6 w-full">
          <h3 className="text-lg font-semibold text-theme-text-bright mb-2">{current.title}</h3>
          <p className="text-theme-text-dim text-sm mb-4">{current.description}</p>

          <div className="space-y-2">
            {current.choices.map((choice) => (
              <button
                key={choice.id}
                className={`w-full text-left p-3 rounded-lg border-2 transition-all ${
                  selections[current.id] === choice.id
                    ? "border-theme-primary bg-theme-primary-glow"
                    : "border-theme-border-dim bg-theme-surface hover:border-theme-border"
                }`}
                onClick={() => handleChoice(current.id, choice.id)}
              >
                <span className="text-theme-text-bright text-sm font-medium">{choice.label}</span>
                <p className="text-theme-text-dim text-xs mt-1">{choice.description}</p>
              </button>
            ))}
          </div>
        </div>
      )}

      <div className="flex gap-3">
        <button
          className="px-4 py-2 text-theme-text-dim hover:text-theme-text-bright disabled:opacity-30"
          onClick={handleBack}
          disabled={currentIndex === 0}
        >
          Back
        </button>

        {currentIndex < scenarios.length - 1 ? (
          <button
            className="border border-theme-primary text-theme-primary hover:bg-theme-primary-glow transition-colors px-6 py-2 rounded-lg disabled:opacity-50"
            onClick={handleNext}
            disabled={!selections[current?.id]}
          >
            Next
          </button>
        ) : (
          <button
            className="border border-theme-success text-theme-success hover:bg-theme-success-dim transition-colors px-6 py-2 rounded-lg disabled:opacity-50"
            onClick={handleCrystallize}
            disabled={!allSelected || loading}
          >
            {loading ? "Forging..." : "Forge Soul"}
          </button>
        )}
      </div>
    </div>
  );
}
