import { useEffect, useState } from "react";
import type { EditableField, EntityDefinition, FieldUpdate, Personality } from "../lib/genesisClient";

const TEXT_FIELDS: { field: EditableField; label: string; hint?: string }[] = [
  { field: "name", label: "Name" },
  { field: "identity", label: "Identity", hint: "Who is this Entity?" },
  { field: "purpose", label: "Purpose", hint: "How will you spend time together?" },
  { field: "appearance", label: "Appearance", hint: "Age, presentation, species, clothing, pose, expression, environment, and palette." },
  { field: "visual_style", label: "Artistic direction" },
  { field: "voice_profile", label: "Voice & presence", hint: "Tone, cadence, expressiveness, and preferred voice qualities." },
];
const LIST_FIELDS: { field: EditableField; label: string; hint?: string }[] = [
  { field: "values", label: "Values" }, { field: "boundaries", label: "Boundaries" },
  { field: "lore", label: "Fictional biography & lore" },
  { field: "fictional_memories", label: "Created memories", hint: "Invented narrative memories, separate from actual conversation history." },
  { field: "relationships", label: "Relationships", hint: "Friends, mentors, rivals, and shared fictional histories." },
  { field: "capabilities", label: "Interests & capabilities", hint: "Descriptive traits only. These do not authorize tools or connected accounts." },
];
const TRAITS: { field: keyof Personality; label: string }[] = [
  { field: "empathy", label: "Empathy" }, { field: "curiosity", label: "Curiosity" },
  { field: "assertiveness", label: "Assertiveness" }, { field: "humor", label: "Humor" },
  { field: "creativity", label: "Creativity" }, { field: "analytical_reasoning", label: "Analytical reasoning" },
  { field: "emotional_expressiveness", label: "Emotional expression" }, { field: "independence", label: "Independence" },
  { field: "sociability", label: "Sociability" },
];
// Preserve trailing newlines while typing; normalize only the proposed save.
const lines = (value: string) => value.split("\n");
const cleanLines = (value: string[]) => value.map((line) => line.trim()).filter(Boolean);
function savedValue(field: EditableField, value: unknown): unknown {
  if (Array.isArray(value)) return cleanLines(value as string[]);
  if (field === "personality") {
    const p = value as Personality;
    return { ...p, fears: cleanLines(p.fears), aspirations: cleanLines(p.aspirations), response_patterns: cleanLines(p.response_patterns) };
  }
  return value;
}

export default function GenesisDefinition({ definition, busy, onSave, onDraftChanges }: {
  definition: EntityDefinition; busy: boolean; onSave: (updates: FieldUpdate[]) => Promise<void>;
  onDraftChanges: (updates: FieldUpdate[]) => void;
}) {
  const [draft, setDraft] = useState<EntityDefinition>(() => structuredClone(definition));
  const [dirty, setDirty] = useState<Set<EditableField>>(() => new Set());
  const [saved, setSaved] = useState(false);
  useEffect(() => {
    if (dirty.size === 0) setDraft(definition);
    else if ([...dirty].every((field) => JSON.stringify(savedValue(field, draft[field])) === JSON.stringify(definition[field]))) {
      setDirty(new Set());
      onDraftChanges([]);
    }
  }, [definition, dirty, draft, onDraftChanges]);
  const update = (field: EditableField, value: unknown) => {
    setDraft((previous) => ({ ...previous, [field]: value }));
    setDirty((previous) => new Set([...previous, field]));
    const changed = new Set([...dirty, field]);
    onDraftChanges([...changed].map((key) => ({ field: key, value: savedValue(key, key === field ? value : draft[key]) })));
    setSaved(false);
  };
  const personality = (field: keyof Personality, value: unknown) => update("personality", { ...draft.personality, [field]: value });
  const submit = async () => {
    await onSave([...dirty].map((field) => ({ field, value: savedValue(field, draft[field]) })));
    setDirty(new Set());
    onDraftChanges([]);
    setSaved(true);
  };

  return <form className="genesis-definition" onSubmit={(event) => { event.preventDefault(); void submit().catch(() => {}); }}>
    <p className="genesis-muted">These are your confirmed choices. Edit any detail directly, or develop it through conversation.</p>
    {TEXT_FIELDS.map(({ field, label, hint }) => <label key={field}>{label}
      {hint && <small>{hint}</small>}
      {field === "name" ? <input maxLength={80} value={draft.name} onChange={(event) => update(field, event.target.value)} /> :
        <textarea rows={2} maxLength={12000} value={String(draft[field])} onChange={(event) => update(field, event.target.value)} />}
    </label>)}
    <details open><summary>Personality</summary>
      <label>Personality description<textarea rows={3} value={draft.personality.description} onChange={(event) => personality("description", event.target.value)} /></label>
      <div className="genesis-traits">{TRAITS.map(({ field, label }) => <label key={field}>{label}<input type="number" min={0} max={100} placeholder="Unspecified" value={typeof draft.personality[field] === "number" ? draft.personality[field] as number : ""} onChange={(event) => personality(field, event.target.value === "" ? null : Number(event.target.value))} /></label>)}</div>
      <label>Communication style<textarea rows={2} value={draft.personality.communication_style} onChange={(event) => personality("communication_style", event.target.value)} /></label>
      {(["fears", "aspirations", "response_patterns"] as const).map((field) => <label key={field}>{field === "response_patterns" ? "Typical response patterns" : field === "fears" ? "Fears (fictional)" : "Aspirations"}<small>One per line</small><textarea rows={2} value={draft.personality[field].join("\n")} onChange={(event) => personality(field, lines(event.target.value))} /></label>)}
    </details>
    {LIST_FIELDS.map(({ field, label, hint }) => <label key={field}>{label}<small>{hint ? `${hint} ` : ""}One per line.</small><textarea rows={2} value={(draft[field] as string[]).join("\n")} onChange={(event) => update(field, lines(event.target.value))} /></label>)}
    <div className="genesis-actions"><button className="genesis-primary" disabled={busy || dirty.size === 0} type="submit">Save definition</button>{dirty.size > 0 && <span className="genesis-muted">{dirty.size} unsaved {dirty.size === 1 ? "field" : "fields"}</span>}{saved && <span role="status">Saved to Hive.</span>}</div>
  </form>;
}
