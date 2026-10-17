import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import GenesisDefinition from "./GenesisDefinition";
import type { EntityDefinition } from "../lib/genesisClient";

const definition = {
  name: "Nova", identity: "", purpose: "Learn together", appearance: "", visual_style: "", voice_profile: "",
  personality: { description: "", fears: [], aspirations: [], communication_style: "", response_patterns: [] },
  values: [], boundaries: [], lore: [], fictional_memories: [], relationships: [], capabilities: [],
} as unknown as EntityDefinition;
afterEach(cleanup);

describe("Genesis definition editing", () => {
  it("preserves newlines during typing and normalizes list items only for saving", async () => {
    const save = vi.fn().mockResolvedValue(undefined);
    render(<GenesisDefinition definition={definition} busy={false} onSave={save} onDraftChanges={vi.fn()} />);
    const values = screen.getByRole("textbox", { name: /Values/ });
    fireEvent.change(values, { target: { value: "Curiosity\n" } });
    expect(values).toHaveValue("Curiosity\n");
    fireEvent.change(values, { target: { value: "Curiosity\n  Kindness \n" } });
    fireEvent.click(screen.getByRole("button", { name: "Save definition" }));
    await waitFor(() => expect(save).toHaveBeenCalledWith([{ field: "values", value: ["Curiosity", "Kindness"] }]));
  });

  it("clears editor dirty state when awakening saved the same pending changes", async () => {
    const props = { busy: false, onSave: vi.fn(), onDraftChanges: vi.fn() };
    const view = render(<GenesisDefinition definition={definition} {...props} />);
    fireEvent.change(screen.getByRole("textbox", { name: "Name" }), { target: { value: "Aster" } });
    expect(screen.getByRole("button", { name: "Save definition" })).toBeEnabled();
    view.rerender(<GenesisDefinition definition={{ ...definition, name: "Aster" }} {...props} />);
    await waitFor(() => expect(screen.getByRole("button", { name: "Save definition" })).toBeDisabled());
    expect(screen.getByRole("textbox", { name: "Name" })).toHaveValue("Aster");
  });
});
