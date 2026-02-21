import { useMemo, useState } from "react";
import type { CrystallizationIdentityDraft } from "./crystallizationPaths";

interface Props {
  onComplete: (draft: CrystallizationIdentityDraft) => void;
}

type Choice = { label: string; trait: string };
type Question = { prompt: string; choices: Choice[] };

const QUESTIONS: Question[] = [
  {
    prompt: "When speed and accuracy conflict, which do you prioritize first?",
    choices: [
      { label: "Accuracy and verification", trait: "careful" },
      { label: "Speed with explicit caveats", trait: "decisive" },
    ],
  },
  {
    prompt: "How should the entity handle uncertain user intent?",
    choices: [
      { label: "Ask one sharp clarifying question", trait: "precise" },
      { label: "Offer two safe interpretations", trait: "adaptive" },
    ],
  },
  {
    prompt: "What matters most during conflict?",
    choices: [
      { label: "Constitutional boundaries", trait: "principled" },
      { label: "Pragmatic progress", trait: "practical" },
    ],
  },
];

export default function CrystallizationPathPsychQuestions({ onComplete }: Props) {
  const [name, setName] = useState("");
  const [answers, setAnswers] = useState<number[]>(Array(QUESTIONS.length).fill(-1));
  const done = useMemo(() => answers.every((a) => a >= 0), [answers]);

  const complete = () => {
    const traits = answers
      .map((idx, qIdx) => QUESTIONS[qIdx].choices[idx]?.trait)
      .filter(Boolean)
      .join(", ");
    onComplete({
      name: name.trim() || "Abigail",
      purpose: "Provide ethical, useful assistance with transparent reasoning.",
      personality: `Psych profile baseline: ${traits}.`,
      primaryColor: "#14b8a6",
      avatarUrl: "",
    });
  };

  return (
    <div className="p-6 max-w-3xl">
      <h3 className="text-theme-primary-dim text-lg mb-2">Psych and Moral Questions</h3>
      <p className="text-theme-text-dim text-sm mb-4">
        Choose responses that best reflect your mentorship model and risk posture.
      </p>

      <input
        className="w-full bg-theme-input-bg border border-theme-border-dim rounded px-3 py-2 mb-4"
        placeholder="Agent name"
        value={name}
        onChange={(e) => setName(e.target.value)}
      />

      <div className="space-y-4">
        {QUESTIONS.map((q, qIdx) => (
          <div key={q.prompt} className="border border-theme-border-dim rounded p-3">
            <div className="text-sm text-theme-text mb-2">{q.prompt}</div>
            <div className="space-y-2">
              {q.choices.map((choice, cIdx) => (
                <label key={choice.label} className="flex items-center gap-2 text-sm text-theme-text-dim">
                  <input
                    type="radio"
                    name={`q-${qIdx}`}
                    checked={answers[qIdx] === cIdx}
                    onChange={() => {
                      const next = [...answers];
                      next[qIdx] = cIdx;
                      setAnswers(next);
                    }}
                  />
                  {choice.label}
                </label>
              ))}
            </div>
          </div>
        ))}
      </div>

      <button
        disabled={!done}
        className="mt-5 border border-theme-primary px-5 py-2 rounded hover:bg-theme-primary-glow disabled:opacity-50"
        onClick={complete}
      >
        Continue to Soul Preview
      </button>
    </div>
  );
}
