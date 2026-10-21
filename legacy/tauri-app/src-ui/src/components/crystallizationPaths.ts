export interface CrystallizationIdentityDraft {
  name: string;
  purpose: string;
  personality: string;
  primaryColor: string;
  avatarUrl: string;
}

export interface CrystallizationPathDefinition {
  id: string;
  name: string;
  description: string;
  estimatedTime: string;
}

export const CRYSTALLIZATION_PATHS: CrystallizationPathDefinition[] = [
  {
    id: "fast_template",
    name: "Fast Template",
    description: "Pick a proven starter profile and fine-tune quickly.",
    estimatedTime: "2-3 min",
  },
  {
    id: "guided_dialog",
    name: "Guided Dialog",
    description: "Answer progressive mentor questions to shape identity.",
    estimatedTime: "4-6 min",
  },
  {
    id: "image_archetype",
    name: "Image Archetypes",
    description: "Choose between visual archetypes to reveal tone and intent.",
    estimatedTime: "5-7 min",
  },
  {
    id: "psych_moral",
    name: "Psych and Moral",
    description: "Choose responses to scenario prompts and moral trade-offs.",
    estimatedTime: "5-7 min",
  },
  {
    id: "editable_template",
    name: "Editable Template",
    description: "Start from a base constitution profile and edit directly.",
    estimatedTime: "3-5 min",
  },
];
