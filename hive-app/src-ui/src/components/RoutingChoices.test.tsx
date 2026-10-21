import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import RoutingChoices from "./RoutingChoices";
import { chooseRoutingModels, getRoutingChoices, type ModelConnection } from "../lib/daemonClient";
vi.mock("../lib/daemonClient", () => ({ chooseRoutingModels: vi.fn(), getRoutingChoices: vi.fn() }));
afterEach(() => vi.resetAllMocks());
const cloud: ModelConnection = { provider: "openai", model: "fixture-cloud", local_url: null };
const local: ModelConnection = { provider: "local", model: "fixture-local", local_url: "http://127.0.0.1:1234" };
const choices = { policy: { connections: [cloud, local], allowed: [cloud] }, readiness: [{ connection: cloud, ready: true }, { connection: local, ready: true }] };
describe("Allowed model choices", () => {
  it("does not approve discovered connections until a choice is explicitly saved", async () => {
    vi.mocked(getRoutingChoices).mockResolvedValue(choices);
    vi.mocked(chooseRoutingModels).mockResolvedValue({ ...choices, policy: { ...choices.policy, allowed: [local, cloud] } });
    const saved = vi.fn();
    render(<RoutingChoices onSaved={saved} busy={false} />);
    const checkbox = await screen.findByRole("checkbox", { name: /Local server/ });
    expect(checkbox).not.toBeChecked(); expect(chooseRoutingModels).not.toHaveBeenCalled();
    fireEvent.click(checkbox);
    fireEvent.click(screen.getByRole("button", { name: "Try first" }));
    expect(chooseRoutingModels).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Save allowed models" }));
    await waitFor(() => expect(chooseRoutingModels).toHaveBeenCalledWith(choices.policy, [local, cloud]));
    expect(await screen.findByText(/Saved. Abigail can use only/)).toBeInTheDocument(); expect(saved).toHaveBeenCalledOnce();
  });
  it("keeps an unavailable approved model visible and allows stopping all model use", async () => {
    vi.mocked(getRoutingChoices).mockResolvedValue({ ...choices, readiness: choices.readiness.map(item => ({ ...item, ready: false })) });
    vi.mocked(chooseRoutingModels).mockResolvedValue({ ...choices, policy: { ...choices.policy, allowed: [] } });
    render(<RoutingChoices onSaved={vi.fn()} busy={false} />);
    const approved = await screen.findByRole("checkbox", { name: /OpenAI/ });
    expect(approved).toBeChecked(); expect(approved).toBeEnabled();
    expect(screen.getByRole("checkbox", { name: /Local server/ })).toBeDisabled();
    fireEvent.click(approved); fireEvent.click(screen.getByRole("button", { name: "Save allowed models" }));
    expect(await screen.findByText(/Conversations will wait/)).toBeInTheDocument();
    expect(chooseRoutingModels).toHaveBeenCalledWith(choices.policy, []);
  });
  it("preserves choices after a stale save and offers refresh without claiming success", async () => {
    vi.mocked(getRoutingChoices).mockResolvedValue(choices);
    vi.mocked(chooseRoutingModels).mockRejectedValue(new Error("Model choices changed. Refresh Settings."));
    const saved = vi.fn(); render(<RoutingChoices onSaved={saved} busy={false} />);
    await screen.findByRole("checkbox", { name: /OpenAI/ });
    fireEvent.click(screen.getByRole("button", { name: "Save allowed models" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Model choices changed");
    expect(saved).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Refresh choices" }));
    await waitFor(() => expect(getRoutingChoices).toHaveBeenCalledTimes(2));
  });
});
