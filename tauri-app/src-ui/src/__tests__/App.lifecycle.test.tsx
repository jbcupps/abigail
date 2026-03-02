import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, beforeEach, expect } from "vitest";
import App from "../App";
import { installBrowserTauriHarness } from "../browserTauriHarness";

describe("App full lifecycle flow", () => {
  beforeEach(() => {
    installBrowserTauriHarness({ force: true, resetState: true });
  });

  it("runs full lifecycle: birth -> chat -> skill -> eject -> reload identity", async () => {
    const user = userEvent.setup();
    render(<App />);

    // Splash → may briefly show model_loading → management
    await user.click(await screen.findByRole("button", { name: /\[skip\]/i }));

    // Wait for management screen (entity name input)
    const entityInput = await waitFor(
      () => screen.getByPlaceholderText(/entity name/i),
      { timeout: 5000 }
    );
    await user.type(entityInput, "Lifecycle Prime");
    await user.click(screen.getByRole("button", { name: /^birth$/i }));

    // Key presentation
    await user.click(await screen.findByRole("checkbox"));
    await user.click(screen.getByRole("button", { name: /continue/i }));

    // Now goes directly to Genesis (Ignition/Connectivity removed)
    await user.click(await screen.findByRole("button", { name: /fast template/i }));
    await user.click(screen.getByRole("button", { name: /begin/i }));
    await user.click(await screen.findByRole("button", { name: /use template/i }));
    await user.click(await screen.findByRole("button", { name: /crystallize soul/i }));
    await user.click(await screen.findByRole("button", { name: /begin emergence ceremony/i }));

    const input = await waitFor(() => screen.getByPlaceholderText(/^message$/i), { timeout: 7000 });
    await user.type(input, "hello lifecycle");
    await user.click(screen.getByRole("button", { name: /send/i }));
    await waitFor(() => {
      expect(screen.getByText(/harness reply via/i)).toBeInTheDocument();
    });

    await user.type(screen.getByPlaceholderText(/^message$/i), "please read clipboard");
    await user.click(screen.getByRole("button", { name: /send/i }));
    await waitFor(() => {
      expect(screen.getByText(/clipboard skill result: read succeeded/i)).toBeInTheDocument();
    });

    // Eject back to management.
    await user.click(screen.getByTitle(/open the forge/i));
    await user.click(await screen.findByRole("button", { name: /\[eject\]/i }));
    expect(await screen.findByText(/soul registry/i)).toBeInTheDocument();

    // Reload same identity and validate chat re-entry.
    await user.click(screen.getByText("Lifecycle Prime"));
    const resumedInput = await waitFor(() => screen.getByPlaceholderText(/^message$/i), { timeout: 7000 });
    await user.type(resumedInput, "resumed lifecycle check");
    await user.click(screen.getByRole("button", { name: /send/i }));
    await waitFor(() => {
      expect(screen.getByText(/harness reply via .*resumed lifecycle check/i)).toBeInTheDocument();
    });
  }, 30000);

  it("creates a test entity, completes birth, and reaches chat", async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: /\[skip\]/i }));

    const entityInput = await waitFor(
      () => screen.getByPlaceholderText(/entity name/i),
      { timeout: 5000 }
    );
    await user.type(entityInput, "Provider Validation Entity");
    await user.click(screen.getByRole("button", { name: /^birth$/i }));

    await user.click(await screen.findByRole("checkbox"));
    await user.click(screen.getByRole("button", { name: /continue/i }));

    // Goes directly to Genesis now (no Connectivity)
    await user.click(await screen.findByRole("button", { name: /fast template/i }));
    await user.click(screen.getByRole("button", { name: /begin/i }));
    await user.click(await screen.findByRole("button", { name: /use template/i }));
    await user.click(await screen.findByRole("button", { name: /crystallize soul/i }));
    await user.click(await screen.findByRole("button", { name: /begin emergence ceremony/i }));

    const messageInput = await waitFor(() => screen.getByPlaceholderText(/^message$/i), {
      timeout: 7000,
    });
    expect(messageInput).toBeInTheDocument();
  }, 30000);
});
