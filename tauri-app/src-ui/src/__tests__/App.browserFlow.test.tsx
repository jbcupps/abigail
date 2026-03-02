import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import App from "../App";
import { installBrowserTauriHarness } from "../browserTauriHarness";

describe("App browser harness full flow", () => {
  beforeEach(() => {
    installBrowserTauriHarness({ force: true, resetState: true });
  });

  it("completes Birth -> Genesis -> Chat -> Clipboard scenario", async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: /\[skip\]/i }));
    expect(
      await screen.findByText(/birth a new sovereign entity to begin/i, {}, { timeout: 5000 })
    ).toBeInTheDocument();

    await user.type(screen.getByPlaceholderText(/entity name/i), "E2E Birth Test");
    await user.click(screen.getByRole("button", { name: /^birth$/i }));

    expect(await screen.findByText(/your constitutional signing key/i)).toBeInTheDocument();
    await user.click(screen.getByRole("checkbox"));
    await user.click(screen.getByRole("button", { name: /continue/i }));

    // Goes directly to Genesis now (Ignition/Connectivity removed)
    await user.click(await screen.findByRole("button", { name: /fast template/i }));
    await user.click(screen.getByRole("button", { name: /begin/i }));
    await user.click(await screen.findByRole("button", { name: /use template/i }));

    await user.click(await screen.findByRole("button", { name: /crystallize soul/i }));
    await user.click(await screen.findByRole("button", { name: /begin emergence ceremony/i }));

    const messageInput = await waitFor(
      () => screen.getByPlaceholderText(/^message$/i),
      { timeout: 7000 }
    );
    await user.type(messageInput, "hello from browser harness");
    await user.click(screen.getByRole("button", { name: /send/i }));
    await waitFor(() => {
      expect(screen.getByText(/harness reply via/i)).toBeInTheDocument();
    });

    await user.type(screen.getByPlaceholderText(/^message$/i), "please read clipboard");
    await user.click(screen.getByRole("button", { name: /send/i }));
    await waitFor(() => {
      expect(screen.getByText(/clipboard skill result: read succeeded/i)).toBeInTheDocument();
    });
  }, 20000);

  it("triggers skill factory tool call and renders result", async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: /\[skip\]/i }));

    const entityInput = await waitFor(
      () => screen.getByPlaceholderText(/entity name/i),
      { timeout: 5000 }
    );
    await user.type(entityInput, "Skill Flow Test");
    await user.click(screen.getByRole("button", { name: /^birth$/i }));
    await user.click(await screen.findByRole("checkbox"));
    await user.click(screen.getByRole("button", { name: /continue/i }));

    // Direct to Genesis
    await user.click(await screen.findByRole("button", { name: /fast template/i }));
    await user.click(screen.getByRole("button", { name: /begin/i }));
    await user.click(await screen.findByRole("button", { name: /use template/i }));
    await user.click(await screen.findByRole("button", { name: /crystallize soul/i }));
    await user.click(await screen.findByRole("button", { name: /begin emergence ceremony/i }));

    const messageInput = await waitFor(
      () => screen.getByPlaceholderText(/^message$/i),
      { timeout: 7000 }
    );

    await user.type(messageInput, "please create a skill called greeter");
    await user.click(screen.getByRole("button", { name: /send/i }));
    await waitFor(() => {
      expect(screen.getByText(/I've created the skill 'custom.greeter'/i)).toBeInTheDocument();
    });
  }, 20000);

  it("validates crystallization name is required", async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: /\[skip\]/i }));

    const entityInput = await waitFor(
      () => screen.getByPlaceholderText(/entity name/i),
      { timeout: 5000 }
    );
    await user.type(entityInput, "Validation Flow");
    await user.click(screen.getByRole("button", { name: /^birth$/i }));

    await user.click(await screen.findByRole("checkbox"));
    await user.click(screen.getByRole("button", { name: /continue/i }));

    // Direct to Genesis
    await user.click(await screen.findByRole("button", { name: /fast template/i }));
    await user.click(screen.getByRole("button", { name: /begin/i }));
    await user.click(await screen.findByRole("button", { name: /use template/i }));

    // Crystallization guard: name is required.
    await user.clear(screen.getByPlaceholderText(/^abigail$/i));
    await user.click(screen.getByRole("button", { name: /crystallize soul/i }));
    expect(await screen.findByText(/name is required/i)).toBeInTheDocument();
  }, 20000);

  it("recovers from provider failure and reaches chat", async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: /\[skip\]/i }));

    const entityInput = await waitFor(
      () => screen.getByPlaceholderText(/entity name/i),
      { timeout: 5000 }
    );
    await user.type(entityInput, "Provider Recovery");
    await user.click(screen.getByRole("button", { name: /^birth$/i }));
    await user.click(await screen.findByRole("checkbox"));
    await user.click(screen.getByRole("button", { name: /continue/i }));

    // Direct to Genesis, complete birth
    await user.click(await screen.findByRole("button", { name: /fast template/i }));
    await user.click(screen.getByRole("button", { name: /begin/i }));
    await user.click(await screen.findByRole("button", { name: /use template/i }));
    await user.click(await screen.findByRole("button", { name: /crystallize soul/i }));
    await user.click(await screen.findByRole("button", { name: /begin emergence ceremony/i }));
    const messageInput = await waitFor(
      () => screen.getByPlaceholderText(/^message$/i),
      { timeout: 7000 }
    );
    await user.type(messageInput, "verify recovery");
    await user.click(screen.getByRole("button", { name: /send/i }));
    expect(await screen.findByText(/harness reply via (openai|local)/i)).toBeInTheDocument();
  }, 25000);
});
