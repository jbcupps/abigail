import { fireEvent, render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import SetupChat from "./SetupChat";
import type { SetupStatus } from "../lib/daemonClient";

const mocks = vi.hoisted(() => ({
  history: vi.fn(),
  send: vi.fn(),
  status: vi.fn(),
}));
vi.mock("../lib/daemonClient", () => ({
  setupHistory: mocks.history,
  sendSetupMessage: mocks.send,
  getSetupStatus: mocks.status,
}));

it("keeps conversation and unsent text when the model changes or a turn fails", async () => {
  const local: SetupStatus = {
    phase: "ready",
    message: "Ready",
    model: "local-test",
    active_provider: "local",
    active_model: "local-test",
  };
  mocks.history.mockResolvedValue({
    messages: [{ role: "assistant", content: "Welcome back to our setup." }],
  });
  mocks.send.mockRejectedValue(
    new Error("Provider is offline. Retry or continue locally."),
  );
  mocks.status.mockResolvedValue(local);
  const props = { setup: local, onConnect: vi.fn(), onStatus: vi.fn() };
  const view = render(<SetupChat {...props} />);
  await screen.findByText("Welcome back to our setup.");
  fireEvent.change(screen.getByLabelText("Message Abigail"), {
    target: { value: "Explain API billing" },
  });
  view.rerender(
    <SetupChat
      {...props}
      setup={{
        ...local,
        active_provider: "openai",
        active_model: "test-model",
      }}
    />,
  );
  expect(screen.getByText("Welcome back to our setup.")).toBeInTheDocument();
  expect(screen.getByLabelText("Message Abigail")).toHaveValue(
    "Explain API billing",
  );
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  await screen.findByRole("alert");
  expect(screen.getByLabelText("Message Abigail")).toHaveValue(
    "Explain API billing",
  );
  expect(mocks.history).toHaveBeenCalledOnce();
});
