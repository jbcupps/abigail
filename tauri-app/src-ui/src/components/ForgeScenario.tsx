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
        <h2 className="text-2xl font-bold text-white">Soul Forged</h2>
        <div className="bg-gray-800 rounded-lg p-6 w-full text-center">
          <p className="text-gray-400 text-sm mb-2">Archetype</p>
          <h3 className="text-xl font-bold text-blue-400 mb-4">{soulOutput.archetype}</h3>

          <div className="grid grid-cols-2 gap-3 mb-4">
            <div className="bg-gray-700 rounded p-2">
              <p className="text-xs text-gray-400">Deontology</p>
              <p className="text-white font-mono">{(soulOutput.weights.deontology * 100).toFixed(0)}%</p>
            </div>
            <div className="bg-gray-700 rounded p-2">
              <p className="text-xs text-gray-400">Teleology</p>
              <p className="text-white font-mono">{(soulOutput.weights.teleology * 100).toFixed(0)}%</p>
            </div>
            <div className="bg-gray-700 rounded p-2">
              <p className="text-xs text-gray-400">Areteology</p>
              <p className="text-white font-mono">{(soulOutput.weights.areteology * 100).toFixed(0)}%</p>
            </div>
            <div className="bg-gray-700 rounded p-2">
              <p className="text-xs text-gray-400">Welfare</p>
              <p className="text-white font-mono">{(soulOutput.weights.welfare * 100).toFixed(0)}%</p>
            </div>
          </div>

          <pre className="text-green-400 text-xs font-mono whitespace-pre mb-4">{soulOutput.sigil}</pre>

          <p className="text-xs text-gray-500 font-mono break-all">
            Hash: {soulOutput.soul_hash}
          </p>
        </div>

        {testResult && (
          <div className="bg-gray-800 border border-gray-700 rounded-lg p-4 w-full">
            <p className="text-xs text-gray-400 mb-1">Skill Test Result</p>
            <pre className="text-green-400 text-sm font-mono whitespace-pre-wrap">{testResult}</pre>
          </div>
        )}

        <div className="flex gap-3">
          <button
            className="border border-gray-600 hover:border-blue-500 text-gray-300 hover:text-white px-6 py-3 rounded-lg transition-colors disabled:opacity-50"
            onClick={handleTestAgent}
            disabled={testing}
          >
            {testing ? "Testing..." : "Test Agent"}
          </button>
          <button
            className="bg-blue-600 hover:bg-blue-700 text-white px-8 py-3 rounded-lg transition-opacity"
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
        <p className="text-gray-400 animate-pulse">Loading scenarios...</p>
      </div>
    );
  }

  return (
    <div className="flex flex-col items-center gap-6 p-8 max-w-2xl mx-auto">
      <h2 className="text-2xl font-bold text-white">Soul Forge</h2>
      <p className="text-gray-400 text-center text-sm">
        Scenario {currentIndex + 1} of {scenarios.length}
      </p>

      {current && (
        <div className="bg-gray-800 rounded-lg p-6 w-full">
          <h3 className="text-lg font-semibold text-white mb-2">{current.title}</h3>
          <p className="text-gray-400 text-sm mb-4">{current.description}</p>

          <div className="space-y-2">
            {current.choices.map((choice) => (
              <button
                key={choice.id}
                className={`w-full text-left p-3 rounded-lg border-2 transition-all ${
                  selections[current.id] === choice.id
                    ? "border-blue-500 bg-blue-900/20"
                    : "border-gray-700 bg-gray-700 hover:border-gray-600"
                }`}
                onClick={() => handleChoice(current.id, choice.id)}
              >
                <span className="text-white text-sm font-medium">{choice.label}</span>
                <p className="text-gray-400 text-xs mt-1">{choice.description}</p>
              </button>
            ))}
          </div>
        </div>
      )}

      <div className="flex gap-3">
        <button
          className="px-4 py-2 text-gray-400 hover:text-white disabled:opacity-30"
          onClick={handleBack}
          disabled={currentIndex === 0}
        >
          Back
        </button>

        {currentIndex < scenarios.length - 1 ? (
          <button
            className="bg-blue-600 hover:bg-blue-700 text-white px-6 py-2 rounded-lg disabled:opacity-50"
            onClick={handleNext}
            disabled={!selections[current?.id]}
          >
            Next
          </button>
        ) : (
          <button
            className="bg-green-600 hover:bg-green-700 text-white px-6 py-2 rounded-lg disabled:opacity-50"
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
