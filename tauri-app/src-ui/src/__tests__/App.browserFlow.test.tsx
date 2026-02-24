import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it } from "vitest";
import App from "../App";
import { installBrowserTauriHarness } from "../browserTauriHarness";

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

describe("App browser harness full flow", () => {
  beforeEach(() => {
    installBrowserTauriHarness({ force: true, resetState: true });
  });

  it("completes Birth -> Providers -> Chat -> Clipboard scenario", async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: /\[skip\]/i }));
    expect(await screen.findByText(/birth a new sovereign entity to begin/i)).toBeInTheDocument();

    await user.type(screen.getByPlaceholderText(/entity name/i), "E2E Birth Test");
    await user.click(screen.getByRole("button", { name: /^birth$/i }));

    expect(await screen.findByText(/your constitutional signing key/i)).toBeInTheDocument();
    await user.click(screen.getByRole("checkbox"));
    await user.click(screen.getByRole("button", { name: /continue/i }));

    expect(await screen.findByText(/ignition: connect your local mind/i)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /continue without model/i }));

    expect(await screen.findByText(/connectivity command center/i)).toBeInTheDocument();

    const saveProvider = async (providerLabel: RegExp, ariaLabel: RegExp, key: string) => {
      await user.click(screen.getByRole("button", { name: providerLabel }));
      const dialog = await screen.findByRole("dialog");
      await user.type(within(dialog).getByLabelText(ariaLabel), key);
      await user.click(within(dialog).getByRole("button", { name: /save & validate/i }));
      await waitFor(() => {
        expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
      });
    };

    await saveProvider(
      /^openai$/i,
      /openai api key/i,
      getTestKey("OPENAI_API_KEY", "sk-test-openai-1234567890")
    );
    await saveProvider(
      /^google$/i,
      /google \(gemini\) api key/i,
      getTestKey("GOOGLE_API_KEY", "AIza-test-google-1234567890")
    );
    await saveProvider(
      /^xai$/i,
      /x\.ai \(grok\) api key/i,
      getTestKey("XAI_API_KEY", "xai-test-grok-1234567890")
    );

    await advanceFromConnectivity(user);

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
    await user.type(screen.getByPlaceholderText(/entity name/i), "Skill Flow Test");
    await user.click(screen.getByRole("button", { name: /^birth$/i }));
    await user.click(await screen.findByRole("checkbox"));
    await user.click(screen.getByRole("button", { name: /continue/i }));
    await user.click(await screen.findByRole("button", { name: /continue without model/i }));

    const saveProvider = async (providerLabel: RegExp, ariaLabel: RegExp, key: string) => {
      await user.click(screen.getByRole("button", { name: providerLabel }));
      const dialog = await screen.findByRole("dialog");
      await user.type(within(dialog).getByLabelText(ariaLabel), key);
      await user.click(within(dialog).getByRole("button", { name: /save & validate/i }));
      await waitFor(() => {
        expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
      });
    };

    await saveProvider(
      /^openai$/i,
      /openai api key/i,
      getTestKey("OPENAI_API_KEY", "sk-test-skill-flow")
    );

    await advanceFromConnectivity(user);
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

  it("enforces connectivity provider requirement and crystallization name validation", async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: /\[skip\]/i }));
    await user.type(screen.getByPlaceholderText(/entity name/i), "Validation Flow");
    await user.click(screen.getByRole("button", { name: /^birth$/i }));

    await user.click(await screen.findByRole("checkbox"));
    await user.click(screen.getByRole("button", { name: /continue/i }));
    await user.click(await screen.findByRole("button", { name: /continue without model/i }));

    // Birth connectivity guard: cannot proceed without at least one provider.
    await user.click(screen.getByRole("button", { name: /continue to crystallization/i }));
    expect(
      await screen.findByText(/at least one provider must be configured before crystallization can begin/i)
    ).toBeInTheDocument();

    // Add one provider and advance to crystallization.
    await user.click(screen.getByRole("button", { name: /^openai$/i }));
    const dialog = await screen.findByRole("dialog");
    await user.type(
      within(dialog).getByLabelText(/openai api key/i),
      getTestKey("OPENAI_API_KEY", "sk-test-openai-validation")
    );
    await user.click(within(dialog).getByRole("button", { name: /save & validate/i }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });

    await advanceFromConnectivity(user);
    await user.click(await screen.findByRole("button", { name: /fast template/i }));
    await user.click(screen.getByRole("button", { name: /begin/i }));
    await user.click(await screen.findByRole("button", { name: /use template/i }));

    // Crystallization guard: name is required.
    await user.clear(screen.getByPlaceholderText(/^abigail$/i));
    await user.click(screen.getByRole("button", { name: /crystallize soul/i }));
    expect(await screen.findByText(/name is required/i)).toBeInTheDocument();
  }, 20000);

  it("surfaces provider failure then recovers with working provider", async () => {
    await invoke("harness_debug_set_provider_validation", {
      provider: "anthropic",
      error: "Anthropic API error (404 Not Found): not_found_error - model: claude-sonnet-4-6",
    });

    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: /\[skip\]/i }));
    await user.type(screen.getByPlaceholderText(/entity name/i), "Provider Recovery");
    await user.click(screen.getByRole("button", { name: /^birth$/i }));
    await user.click(await screen.findByRole("checkbox"));
    await user.click(screen.getByRole("button", { name: /continue/i }));
    await user.click(await screen.findByRole("button", { name: /continue without model/i }));

    await user.click(screen.getByRole("button", { name: /^anthropic$/i }));
    const anthropicDialog = await screen.findByRole("dialog");
    await user.type(
      within(anthropicDialog).getByLabelText(/anthropic api key/i),
      getTestKey("ANTHROPIC_API_KEY", "sk-ant-test-provider-failure")
    );
    await user.click(within(anthropicDialog).getByRole("button", { name: /save & validate/i }));
    expect(await within(anthropicDialog).findByText(/anthropic api error/i)).toBeInTheDocument();
    await user.click(within(anthropicDialog).getByRole("button", { name: /cancel/i }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });

    await user.click(screen.getByRole("button", { name: /^openai$/i }));
    const openAiDialog = await screen.findByRole("dialog");
    await user.type(
      within(openAiDialog).getByLabelText(/openai api key/i),
      getTestKey("OPENAI_API_KEY", "sk-test-openai-provider-recovery")
    );
    await user.click(within(openAiDialog).getByRole("button", { name: /save & validate/i }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
    expect(screen.queryByText(/anthropic api error/i)).not.toBeInTheDocument();

    await advanceFromConnectivity(user);
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
    expect(await screen.findByText(/harness reply via openai/i)).toBeInTheDocument();
  }, 25000);
});

