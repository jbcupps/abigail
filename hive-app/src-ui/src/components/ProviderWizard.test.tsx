import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import ProviderWizard from "./ProviderWizard";

vi.mock("../lib/connection", () => ({ resolveHiveUrl: async () => "http://127.0.0.1:43141" }));
const json = (data: unknown) => new Response(JSON.stringify({ ok: true, data }), { headers: { "Content-Type": "application/json" } });

afterEach(() => vi.unstubAllGlobals());

describe("Model connection", () => {
  it("keeps keyboard focus on the discovery summary when its options are collapsed", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => json({ providers: [
      { provider: "codex-cli", on_path: true, is_official: true, is_authenticated: true, authentication: true },
    ] })));
    render(<ProviderWizard onClose={vi.fn()} onComplete={vi.fn()} />);
    const summary = await screen.findByText(/1 connection tools found/);
    fireEvent.click(summary);
    expect(summary.parentElement).not.toHaveAttribute("open");
    screen.getByRole("button", { name: "Close" }).focus();
    fireEvent.keyDown(document, { key: "Tab", shiftKey: true });
    expect(summary).toHaveFocus();
  });

  it("shows incomplete discovery as a retryable error while keeping other routes available", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => json({ presence_only: true })));
    render(<ProviderWizard onClose={vi.fn()} onComplete={vi.fn()} />);
    expect(await screen.findByRole("alert")).toHaveTextContent("incomplete installed-tool status");
    expect(screen.getByRole("button", { name: /Use a local model/ })).toBeEnabled();
    expect(screen.getByRole("button", { name: /Paste an API key/ })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Check again" })).toBeEnabled();
  });

  it("does not announce a connection when Hive confirms a different selection", async () => {
    const complete = vi.fn();
    vi.stubGlobal("fetch", vi.fn(async (url: string) => url.endsWith("/detect")
      ? json({ providers: [{ provider: "codex-cli", on_path: true, is_official: true, is_authenticated: true }] })
      : json({ provider: "grok-cli", model: null })));
    render(<ProviderWizard onClose={vi.fn()} onComplete={complete} />);
    fireEvent.click(await screen.findByRole("button", { name: "Use ChatGPT" }));
    fireEvent.click(screen.getByRole("button", { name: "Check and connect" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("did not confirm");
    expect(complete).not.toHaveBeenCalled();
  });

  it("requires an explicit connection check after choosing a signed-in CLI", async () => {
    const completed = vi.fn();
    const requests = vi.fn(async (url: string, init?: RequestInit) => {
      if (url.endsWith("/detect")) return json({ providers: [
        { provider: "claude-cli", on_path: true, is_official: true, is_authenticated: true, authentication: true },
      ] });
      expect(url).toMatch(/\/v1\/providers\/hive-default$/);
      expect(JSON.parse(init?.body as string)).toEqual({ provider: "claude-cli" });
      return json({ provider: "claude-cli", model: null });
    });
    vi.stubGlobal("fetch", requests);
    render(<ProviderWizard onClose={vi.fn()} onComplete={completed} />);
    fireEvent.click(await screen.findByRole("button", { name: "Use Claude" }));
    expect(requests).toHaveBeenCalledOnce();
    expect(screen.getByText(/account's usage allowance/)).toBeInTheDocument();
    expect(completed).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Check and connect" }));
    expect(await screen.findByText(/Claude Code is connected\./)).toBeInTheDocument();
    expect(completed).toHaveBeenCalledOnce();
  });

  it.each([["codex-cli", "Codex (ChatGPT)"], ["grok-cli", "Grok Build"]])("waits for the real %s account check and preserves a failed connection for retry", async (provider, label) => {
    const completed = vi.fn();
    let resolveProbe: (response: Response) => void = () => undefined;
    const requests = vi.fn(async (url: string, init?: RequestInit) => {
      if (url.endsWith("/detect")) return json({ providers: [
        { provider, on_path: true, is_official: true, is_authenticated: false, auth_hint: `Check your ${label} sign-in.` },
      ] });
      expect(url).toMatch(/\/v1\/providers\/hive-default$/);
      expect(JSON.parse(init?.body as string)).toEqual({ provider });
      return new Promise<Response>((resolve) => { resolveProbe = resolve; });
    });
    vi.stubGlobal("fetch", requests);
    render(<ProviderWizard onClose={vi.fn()} onComplete={completed} />);
    fireEvent.click(await screen.findByRole("button", { name: `Use ${provider === "codex-cli" ? "ChatGPT" : "Grok"}` }));
    expect(requests).toHaveBeenCalledOnce(); // Selecting a tool does not send inference.
    const card = screen.getByRole("button", { name: "Check and connect" });
    fireEvent.click(card);
    expect(await screen.findByRole("status")).toHaveTextContent(`Checking your ${label} connection`);
    expect(card).toBeDisabled();
    expect(screen.getByRole("button", { name: "Back" })).toBeDisabled();
    expect(completed).not.toHaveBeenCalled();
    await waitFor(() => expect(requests).toHaveBeenCalledTimes(2));
    resolveProbe(new Response(JSON.stringify({ ok: false, error: `${label} connection check failed. Check its account sign-in and try again.` }), { headers: { "Content-Type": "application/json" } }));
    expect(await screen.findByRole("alert")).toHaveTextContent(`${label} connection check failed`);
    expect(completed).not.toHaveBeenCalled();
    expect(card).toBeEnabled();
    fireEvent.click(card);
    await waitFor(() => expect(requests).toHaveBeenCalledTimes(3));
    resolveProbe(json({ provider, model: null }));
    expect(await screen.findByText(text => text.startsWith(`${label} is connected.`))).toBeInTheDocument();
    expect(completed).toHaveBeenCalledOnce();
  });

  it("checks the running local server before reporting success", async () => {
    const completed = vi.fn();
    const requests = vi.fn(async (url: string, init?: RequestInit) => {
      if (url.endsWith("/detect")) return json({ providers: [] });
      expect(url).toMatch(/\/v1\/providers\/local$/);
      expect(JSON.parse(init?.body as string)).toEqual({ base_url: "http://127.0.0.1:1234/v1" });
      return json({ base_url: "http://127.0.0.1:1234/v1", model: "local-test-model" });
    });
    vi.stubGlobal("fetch", requests);
    render(<ProviderWizard onClose={vi.fn()} onComplete={completed} />);
    expect(screen.queryByText(/A local model already works/)).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Use a local model/ }));
    fireEvent.click(screen.getByRole("button", { name: "LM Studio" }));
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    expect(await screen.findByText(/Local model local-test-model is connected/)).toBeInTheDocument();
    expect(completed).toHaveBeenCalledOnce();
  });

  it("validates the cloud key before storing it and saves the selected model", async () => {
    const paths: string[] = [];
    const completed = vi.fn();
    vi.stubGlobal("fetch", vi.fn(async (url: string, init?: RequestInit) => {
      paths.push(new URL(url).pathname);
      if (url.endsWith("/detect")) return json({ providers: [] });
      if (url.endsWith("/models")) return json({ provider: "openai", models: [{ model_id: "test-model", display_name: "Test model" }] });
      if (url.endsWith("/secrets")) return json("stored");
      expect(JSON.parse(init?.body as string)).toEqual({ provider: "openai", model: "test-model" });
      return json({ provider: "openai", model: "test-model" });
    }));
    render(<ProviderWizard onClose={vi.fn()} onComplete={completed} />);
    fireEvent.click(screen.getByRole("button", { name: /Paste an API key/ }));
    fireEvent.change(screen.getByLabelText("Paste your provider API key"), { target: { value: "sk-test-placeholder" } });
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByRole("combobox", { name: "Model" })).toHaveValue("test-model");
    expect(paths.indexOf("/v1/providers/models")).toBeLessThan(paths.indexOf("/v1/secrets"));
    fireEvent.click(screen.getByRole("button", { name: "Check and allow model" }));
    await waitFor(() => expect(completed).toHaveBeenCalledOnce());
    expect(screen.getByText(/Messages sent with this model are processed by that provider/)).toBeInTheDocument();
  });
});
