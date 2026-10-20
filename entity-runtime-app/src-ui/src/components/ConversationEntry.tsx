import type { EntityProfile, Temperament } from "../presence/profile";

// Editable conversation starters, never hidden personality instructions.
const FIRST_TASKS: Record<Temperament, Array<{ title: string; draft: string }>> = {
  gentle: [
    { title: "Make room for an idea", draft: "Help me untangle an idea. Ask me one small question to get started." },
    { title: "Find one small next step", draft: "Help me make a manageable plan. First, ask me what I want to do." },
    { title: "Make something together", draft: "Let's create something small together. Ask me what I feel like making." },
  ],
  direct: [
    { title: "Turn a goal into a plan", draft: "Help me turn a goal into three practical next steps. Ask me the goal and deadline first." },
    { title: "Compare my options", draft: "Help me compare my options. Ask me what I'm deciding and what matters most." },
    { title: "Sharpen a draft", draft: "Help me make a draft clearer. Ask me to paste it and tell you who it's for." },
  ],
  curious: [
    { title: "Find an unexpected angle", draft: "Help me find an unexpected angle on an idea. Ask me what I'm exploring first." },
    { title: "Try a tiny experiment", draft: "Help me design a small experiment for something I want to learn. Ask me what I'm curious about." },
    { title: "Build a strange little world", draft: "Let's invent a strange little world together. Ask me for one detail to begin with." },
  ],
};

export default function ConversationEntry({ profile, onChoose, hasDraft }: {
  profile?: EntityProfile;
  onChoose: (draft: string) => void;
  hasDraft: boolean;
}) {
  return <section className="runtime-first-task" aria-labelledby="first-task-title">
    <span className="runtime-eyebrow">FIRST MISSION / OPTIONAL</span>
    <h3 id="first-task-title">Where shall we begin?</h3>
    <p>Pick a spark to edit, or write your own message below.</p>
    <div className="runtime-starters">
      {FIRST_TASKS[profile?.temperament || "gentle"].map(task =>
        <button type="button" key={task.title} disabled={hasDraft} onClick={() => onChoose(task.draft)}>
          {task.title} <span aria-hidden="true">↗</span>
        </button>)}
    </div>
    {hasDraft && <p className="runtime-input-hint">Your draft is ready to shape. Clear it to choose another spark.</p>}
  </section>;
}
