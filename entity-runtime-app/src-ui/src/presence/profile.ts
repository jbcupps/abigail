export type VisualIdentity = "aether" | "ember" | "lumen";
export type Temperament = "gentle" | "direct" | "curious";
export interface EntityProfile {
  name: string;
  purpose: string;
  temperament: Temperament;
  boundaries: string;
  identity: VisualIdentity;
}

export const identities: { id: VisualIdentity; name: string; description: string }[] = [
  { id: "aether", name: "Aether", description: "Iridescent · spacious · blue" },
  { id: "ember", name: "Ember", description: "Solar · grounded · amber" },
  { id: "lumen", name: "Lumen", description: "Auroral · fluid · mint" },
];
export const temperaments: { id: Temperament; name: string; sample: string }[] = [
  { id: "gentle", name: "Gentle", sample: "Let's make some room. What's the one thing that would make today easier?" },
  { id: "direct", name: "Direct", sample: "Let's find the next step. What needs to happen, and by when?" },
  { id: "curious", name: "Curious", sample: "What if we approached this differently? Tell me what you've tried." },
];
export const defaultProfile = (name = ""): EntityProfile => ({ name, purpose: "", temperament: "gentle", boundaries: "Ask before taking action outside our conversation.", identity: "aether" });

export function isEntityProfile(value: unknown): value is EntityProfile {
  if (!value || typeof value !== "object") return false;
  const p = value as Record<string, unknown>;
  return typeof p.name === "string" && !!p.name.trim() && Array.from(p.name).length <= 80 && !/[\u0000-\u001f\u007f]/.test(p.name)
    && typeof p.purpose === "string" && Array.from(p.purpose).length <= 2000
    && typeof p.boundaries === "string" && Array.from(p.boundaries).length <= 2000
    && identities.some(item => item.id === p.identity) && temperaments.some(item => item.id === p.temperament);
}

// An allowlist, not a prompt interpreter. Never copy arbitrary Markdown into an
// instruction, tool permission, network setting, or executable HTML surface.
export function extractProfile(markdown: string): Partial<EntityProfile> {
  if (markdown.length > 32768) throw new Error("Choose a Markdown file smaller than 32 KB.");
  const proposed: Partial<EntityProfile> = {};
  const fields = ["name", "purpose", "temperament", "boundaries", "identity"] as const;
  let section: typeof fields[number] | undefined;
  const acceptField = (key: typeof fields[number], rawValue: string, quoted = false) => {
    const value = quoted ? rawValue : rawValue.trim();
    if (key === "identity") {
      if (identities.some(item => item.id === value.toLowerCase())) proposed.identity = value.toLowerCase() as VisualIdentity;
    } else if (key === "temperament") {
      if (temperaments.some(item => item.id === value.toLowerCase())) proposed.temperament = value.toLowerCase() as Temperament;
    } else if (key === "name") {
      const name = value.trim();
      if (proposed.name === undefined && name && !/[\u0000-\u001f\u007f]/.test(name)) {
        proposed.name = Array.from(name).slice(0, 64).join("");
      }
    } else if (value || quoted) {
      proposed[key] = ((proposed[key] === undefined ? "" : proposed[key] + "\n") + value).slice(0, 2000);
    }
  };
  for (const line of markdown.split(/\r?\n/)) {
    // Quoted section content is literal, including headings, field names and
    // blank lines. This is the format used by our own Markdown export.
    const quoted = section && line.match(/^> ?(.*)$/);
    if (quoted && section) { acceptField(section, quoted[1], true); continue; }
    const heading = line.match(/^#{1,3}\s+(name|purpose|temperament|boundaries|identity)\s*$/i);
    if (heading) { section = heading[1].toLowerCase() as typeof section; continue; }
    if (/^#/.test(line)) { section = undefined; continue; }
    if (section) { acceptField(section, line); continue; }
    const field = line.match(/^\s*(?:[-*]\s+)?(name|purpose|temperament|boundaries|identity)\s*:\s*(.+)$/i);
    if (field) acceptField(field[1].toLowerCase() as typeof fields[number], field[2]);
  }
  if (Object.keys(proposed).length === 0) throw new Error("No supported fields found. Use Name:, Purpose:, Temperament:, Boundaries:, or Identity:.");
  return proposed;
}

export function profileMarkdown(profile: EntityProfile): string {
  const quote = (value: string) => value.split(/\r?\n/).map(line => `> ${line}`).join("\n");
  return `# Entity brief\n\nName: ${profile.name}\nIdentity: ${profile.identity}\nTemperament: ${profile.temperament}\n\n## Purpose\n${quote(profile.purpose)}\n\n## Boundaries\n${quote(profile.boundaries)}\n\n## Scope\nThis is a visual identity and design brief. It does not grant permissions or change runtime instructions.\n`;
}
