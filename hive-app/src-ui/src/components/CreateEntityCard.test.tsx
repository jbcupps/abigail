import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import CreateEntityCard from "./CreateEntityCard";

const api = vi.hoisted(() => ({ createEntity: vi.fn(), completeEntitySetup: vi.fn() }));
vi.mock("../lib/daemonClient", () => api);

describe("Entity creation", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    api.createEntity.mockResolvedValue({ id: "ada" });
    api.completeEntitySetup.mockResolvedValue(undefined);
  });

  it("finishes identity setup with the family's purpose before marking creation complete", async () => {
    const completed = vi.fn();
    render(<CreateEntityCard onCreated={completed} />);
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: " Ada " } });
    fireEvent.change(screen.getByLabelText("Purpose (optional)"), { target: { value: "Help with homework" } });
    fireEvent.keyDown(screen.getByLabelText("Name"), { key: "Enter" });
    fireEvent.keyDown(screen.getByLabelText("Name"), { key: "Enter" });
    await waitFor(() => expect(completed).toHaveBeenCalledOnce());
    expect(api.createEntity).toHaveBeenCalledExactlyOnceWith("Ada");
    expect(api.completeEntitySetup).toHaveBeenCalledExactlyOnceWith("ada", "Help with homework");
  });

  it("retries setup without duplicating a saved Entity", async () => {
    api.completeEntitySetup.mockRejectedValueOnce(new Error("Setup interrupted. Retry."));
    render(<CreateEntityCard onCreated={vi.fn()} />);
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Ada" } });
    fireEvent.click(screen.getByRole("button", { name: "Create Entity" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Setup interrupted");
    fireEvent.click(screen.getByRole("button", { name: "Finish creating" }));
    await waitFor(() => expect(api.completeEntitySetup).toHaveBeenCalledTimes(2));
    expect(api.createEntity).toHaveBeenCalledOnce();
  });
});
