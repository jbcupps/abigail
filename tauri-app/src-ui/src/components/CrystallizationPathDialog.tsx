import { useState } from "react";
import type { CrystallizationIdentityDraft } from "./crystallizationPaths";

interface Props {
  onComplete: (draft: CrystallizationIdentityDraft) => void;
}

type Answers = {
  mentorName: string;
  name: string;
  mission: string;
  tone: string;
  boundary: string;
};

export default function CrystallizationPathDialog({ onComplete }: Props) {
  const [answers, setAnswers] = useState<Answers>({
    mentorName: "",
    name: "",
    mission: "",
    tone: "",
    boundary: "",
  });

  const canContinue =
    answers.name.trim().length > 0 &&
    answers.mission.trim().length > 0 &&
    answers.tone.trim().length > 0;

  const submit = () => {
    onComplete({
      name: answers.name.trim(),
      purpose: answers.mission.trim(),
      personality: `${answers.tone.trim()}. Boundaries: ${answers.boundary.trim() || "Constitution-first decisions."}`,
      primaryColor: "#00b894",
      avatarUrl: "",
    });
  };

  return (
    <div className="p-6 max-w-3xl">
      <h3 className="text-theme-primary-dim text-lg mb-2">Guided Dialog</h3>
      <p className="text-theme-text-dim text-sm mb-4">
        Progressive disclosure interview. Clear answers now become the soul baseline.
      </p>

      <div className="space-y-3">
        <input
          className="w-full bg-theme-input-bg border border-theme-border-dim rounded px-3 py-2"
          placeholder="Your name (mentor)"
          value={answers.mentorName}
          onChange={(e) => setAnswers((p) => ({ ...p, mentorName: e.target.value }))}
        />
        <input
          className="w-full bg-theme-input-bg border border-theme-border-dim rounded px-3 py-2"
          placeholder="Agent name"
          value={answers.name}
          onChange={(e) => setAnswers((p) => ({ ...p, name: e.target.value }))}
        />
        <textarea
          className="w-full bg-theme-input-bg border border-theme-border-dim rounded px-3 py-2"
          rows={2}
          placeholder="Primary mission"
          value={answers.mission}
          onChange={(e) => setAnswers((p) => ({ ...p, mission: e.target.value }))}
        />
        <input
          className="w-full bg-theme-input-bg border border-theme-border-dim rounded px-3 py-2"
          placeholder="Preferred tone (e.g., clear, direct, warm)"
          value={answers.tone}
          onChange={(e) => setAnswers((p) => ({ ...p, tone: e.target.value }))}
        />
        <textarea
          className="w-full bg-theme-input-bg border border-theme-border-dim rounded px-3 py-2"
          rows={2}
          placeholder="One hard boundary this entity should preserve"
          value={answers.boundary}
          onChange={(e) => setAnswers((p) => ({ ...p, boundary: e.target.value }))}
        />
      </div>

      <button
        disabled={!canContinue}
        className="mt-5 border border-theme-primary px-5 py-2 rounded hover:bg-theme-primary-glow disabled:opacity-50"
        onClick={submit}
      >
        Continue to Soul Preview
      </button>
    </div>
  );
}
