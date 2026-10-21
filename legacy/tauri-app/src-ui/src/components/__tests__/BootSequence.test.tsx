import "../../test/tauri-mock";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import BootSequence from "../BootSequence";

const mockInvoke = invoke as unknown as Mock;

function mockBootInvoke(identityStatus: "Clean" | "Broken" | "Complete") {
  mockInvoke.mockImplementation((cmd: string) => {
    switch (cmd) {
      case "init_soul":
        return Promise.resolve(null);
      case "check_interrupted_birth":
        return Promise.resolve({ was_interrupted: false, stage: null });
      case "check_identity_status":
        return Promise.resolve(identityStatus);
      case "start_birth":
        return Promise.resolve(null);
      case "generate_identity":
        return Promise.resolve({
          private_key_base64: "test-private-key",
          public_key_path: "C:/tmp/external_pubkey.bin",
          newly_generated: true,
        });
      case "inspect_identity_integrity":
        return Promise.resolve({
          status: "repairable",
          summary: "Constitutional verification failed for soul.md.",
          details: ["Signature mismatch detected for soul.md"],
        });
      case "save_recovery_key":
        return Promise.resolve("C:/tmp/RECOVERY_BUNDLE.abigail-recovery");
      case "save_recovery_key_plaintext":
        return Promise.resolve("C:/tmp/RECOVERY_KEY.txt");
      default:
        return Promise.resolve(null);
    }
  });
}

describe("BootSequence", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("reaches key presentation on clean identity", async () => {
    mockBootInvoke("Clean");
    const onComplete = vi.fn();

    render(<BootSequence onComplete={onComplete} />);

    expect(await screen.findByText(/your constitutional signing key/i)).toBeInTheDocument();
    expect(onComplete).not.toHaveBeenCalled();
  });

  it("shows repair flow on broken identity", async () => {
    mockBootInvoke("Broken");
    const onComplete = vi.fn();

    render(<BootSequence onComplete={onComplete} />);

    expect(
      await screen.findByRole("heading", { name: /identity verification failed/i })
    ).toBeInTheDocument();
    expect(
      screen.getByText(/constitutional verification failed for soul\.md/i)
    ).toBeInTheDocument();
    expect(onComplete).not.toHaveBeenCalled();
  });

  it("completes immediately when identity is already complete", async () => {
    mockBootInvoke("Complete");
    const onComplete = vi.fn();

    render(<BootSequence onComplete={onComplete} />);

    await waitFor(() => {
      expect(onComplete).toHaveBeenCalledTimes(1);
    });
  });

  it("moves from key presentation into genesis without forcing crystallization stage", async () => {
    mockBootInvoke("Clean");
    const onComplete = vi.fn();

    render(<BootSequence onComplete={onComplete} />);

    const checkbox = await screen.findByRole("checkbox");
    fireEvent.click(checkbox);
    fireEvent.click(screen.getByRole("button", { name: /^continue$/i }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("advance_past_darkness");
      expect(mockInvoke).toHaveBeenCalledWith("advance_to_connectivity");
    });
    expect(mockInvoke).not.toHaveBeenCalledWith("advance_to_crystallization");
    expect(onComplete).not.toHaveBeenCalled();
  });

  it("shows both encrypted and explicit plaintext recovery export actions", async () => {
    mockBootInvoke("Clean");
    const onComplete = vi.fn();

    render(<BootSequence onComplete={onComplete} />);

    expect(
      await screen.findByRole("button", { name: /save encrypted recovery bundle/i })
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /save plaintext key \(advanced\)/i })
    ).toBeInTheDocument();
    expect(onComplete).not.toHaveBeenCalled();
  });
});

