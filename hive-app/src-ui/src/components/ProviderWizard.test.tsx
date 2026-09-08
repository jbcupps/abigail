import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import ProviderWizard from "./ProviderWizard";

const mocks = vi.hoisted(() => ({ activate: vi.fn(), discover: vi.fn() }));
vi.mock("../lib/daemonClient", () => ({
  activateSetupModel: mocks.activate,
  discoverModels: mocks.discover,
}));
beforeEach(() => vi.resetAllMocks());

it("requires an explicit model and waits for a real validation result before handoff", async () => {
  let finish!: () => void;
  mocks.discover.mockResolvedValue([
    { model_id: "claude-test", display_name: "Test model" },
  ]);
  mocks.activate.mockImplementation(
    () =>
      new Promise<void>((resolve) => {
        finish = resolve;
      }),
  );
  const complete = vi.fn();
  render(<ProviderWizard onClose={vi.fn()} onComplete={complete} />);
  fireEvent.change(screen.getByLabelText("API key"), {
    target: { value: "test-key" },
  });
  expect(screen.getByLabelText("API key")).toHaveAttribute("type", "password");
  fireEvent.click(
    screen.getByRole("button", { name: "List available models" }),
  );
  await screen.findByRole("option", { name: "Test model" });
  expect(
    screen.getByRole("button", { name: "Test and connect" }),
  ).toBeDisabled();
  fireEvent.change(screen.getByLabelText("Available models"), {
    target: { value: "claude-test" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Test and connect" }));
  expect(mocks.activate).toHaveBeenCalledWith(
    "anthropic",
    "claude-test",
    "test-key",
  );
  expect(complete).not.toHaveBeenCalled();
  expect(screen.getByRole("button", { name: "Close" })).toBeDisabled();
  finish();
  await screen.findByText(/I’m still Abigail/);
  expect(complete).toHaveBeenCalledOnce();
  expect(screen.queryByLabelText("API key")).not.toBeInTheDocument();
});

it("keeps a failed connection editable and does not activate it", async () => {
  mocks.activate.mockRejectedValue(
    new Error("Model access was denied. Choose another model."),
  );
  const complete = vi.fn();
  render(<ProviderWizard onClose={vi.fn()} onComplete={complete} />);
  fireEvent.change(screen.getByLabelText("API key"), {
    target: { value: "test-key" },
  });
  fireEvent.change(screen.getByLabelText("Model ID"), {
    target: { value: "denied-model" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Test and connect" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Model access was denied",
  );
  expect(complete).not.toHaveBeenCalled();
  expect(screen.getByLabelText("Model ID")).toHaveValue("denied-model");
  fireEvent.change(screen.getByLabelText("Provider"), {
    target: { value: "openai" },
  });
  expect(screen.getByLabelText("API key")).toHaveValue("");
  await waitFor(() =>
    expect(screen.queryByRole("alert")).not.toBeInTheDocument(),
  );
});
