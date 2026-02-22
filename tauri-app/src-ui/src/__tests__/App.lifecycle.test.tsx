import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, beforeEach, expect } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import App from "../App";
import { installBrowserTauriHarness } from "../browserTauriHarness";

interface Snapshot {
  state: {
    providers: string[];
  };
}

function getTestKey(envName: string, fallback: string): string {
  const value = (
    (globalThis as { process?: { env?: Record<string, string | undefined> } }).process?.env?.[
      envName
    ] || ""
  ).trim();
  return value.length > 0 ? value : fallback;
}

async function advanceFromConnectivity(user: ReturnType<typeof userEvent.setup>) {
  const continueBtn =
    screen.queryByRole("button", { name: /continue to crystallization/i }) ??
    screen.queryByRole("button", { name: /establish linkage/i });
  if (!continueBtn) {
    throw new Error("Could not find connectivity advance button");
  }
  await user.click(continueBtn);
}

describe("App full lifecycle flow", () => {
  beforeEach(() => {
    installBrowserTauriHarness({ force: true, resetState: true });
  });

  it("runs full lifecycle: birth -> chat -> skill -> eject -> reload identity", async () => {
    const user = userEvent.setup();
    render(<App />);

    // Birth first identity to chat-ready.
    await user.click(await screen.findByRole("button", { name: /\[skip\]/i }));
    await user.type(screen.getByPlaceholderText(/entity name/i), "Lifecycle Prime");
    await user.click(screen.getByRole("button", { name: /^birth$/i }));

    await user.click(await screen.findByRole("checkbox"));
    await user.click(screen.getByRole("button", { name: /continue/i }));
    await user.click(await screen.findByRole("button", { name: /continue without model/i }));

    await user.click(screen.getByRole("button", { name: /^openai$/i }));
    const dialog = await screen.findByRole("dialog");
    await user.type(within(dialog).getByLabelText(/openai api key/i), "sk-test-openai-lifecycle");
    await user.click(within(dialog).getByRole("button", { name: /save & validate/i }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });

    await advanceFromConnectivity(user);
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

  it("creates a test entity, completes birth, and connects 2+ providers including search", async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: /\[skip\]/i }));
    await user.type(screen.getByPlaceholderText(/entity name/i), "Provider Validation Entity");
    await user.click(screen.getByRole("button", { name: /^birth$/i }));

    await user.click(await screen.findByRole("checkbox"));
    await user.click(screen.getByRole("button", { name: /continue/i }));
    await user.click(await screen.findByRole("button", { name: /continue without model/i }));
    expect(await screen.findByText(/connectivity command center/i)).toBeInTheDocument();

    const saveProvider = async (providerName: RegExp, keyLabel: RegExp, keyValue: string) => {
      await user.click(screen.getByRole("button", { name: providerName }));
      const dialog = await screen.findByRole("dialog");
      await user.type(within(dialog).getByLabelText(keyLabel), keyValue);
      await user.click(within(dialog).getByRole("button", { name: /save & validate/i }));
      await waitFor(() => {
        expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
      });
    };

    await saveProvider(
      /^openai$/i,
      /openai api key/i,
      getTestKey("OPENAI_API_KEY", "sk-test-openai-provider-validation")
    );
    await saveProvider(
      /^perplexity$/i,
      /perplexity api key/i,
      getTestKey("PERPLEXITY_API_KEY", "pplx-test-provider-validation")
    );

    const snapshot = await invoke<Snapshot>("harness_debug_snapshot");
    const providers = snapshot.state.providers;

    const nonCliProviders = providers.filter((provider) => !provider.endsWith("-cli"));
    expect(new Set(nonCliProviders).size).toBeGreaterThanOrEqual(2);
    expect(providers).toContain("perplexity");

    await advanceFromConnectivity(user);
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

