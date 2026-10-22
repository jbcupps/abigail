import "../../test/tauri-mock";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import ForgeScenario from "../ForgeScenario";

const mockInvoke = invoke as unknown as Mock;

describe("ForgeScenario public display boundary", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    mockInvoke.mockImplementation((command: string) => {
      if (command === "get_forge_scenarios") {
        return Promise.resolve([
          {
            id: "care",
            title: "A family member needs help",
            description: "Choose how Nova should respond.",
            choices: [
              {
                id: "protect",
                label: "Protect the family",
                description: "Offer calm, practical support.",
              },
            ],
          },
        ]);
      }
      if (command === "crystallize_forge") {
        return Promise.resolve({
          archetype: "The Guardian",
          soul_hash: "soul-hash",
          sigil: "<guardian>",
        });
      }
      return Promise.resolve(null);
    });
  });

  it("uses the ordered, weight-free backend DTO for display and completion", async () => {
    const onComplete = vi.fn();
    render(<ForgeScenario onComplete={onComplete} />);

    fireEvent.click(await screen.findByRole("button", { name: /protect the family/i }));
    fireEvent.click(screen.getByRole("button", { name: /forge soul/i }));
    expect(await screen.findByRole("heading", { name: /soul forged/i })).toBeInTheDocument();
    expect(mockInvoke).toHaveBeenCalledWith("crystallize_forge", {
      choices: ["protect"],
    });

    for (const hiddenAxis of ["Deontology", "Teleology", "Areteology", "Welfare"]) {
      expect(screen.queryByText(hiddenAxis)).not.toBeInTheDocument();
    }
    expect(screen.queryByText("91%")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /accept soul/i }));
    await waitFor(() => {
      expect(onComplete).toHaveBeenCalledWith({
        archetype: "The Guardian",
        soul_hash: "soul-hash",
        sigil: "<guardian>",
      });
    });
    expect(onComplete.mock.calls[0][0]).not.toHaveProperty("weights");
  });
});
