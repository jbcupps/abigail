import "../test/tauri-mock";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, type Mock } from "vitest";
import App from "../App";

const mockInvoke = invoke as unknown as Mock;

describe("App state transitions", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("transitions from splash to management when no identities exist", async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      switch (cmd) {
        case "get_active_agent":
          return Promise.resolve(null);
        case "get_identities":
          return Promise.resolve([]);
        case "check_existing_identity":
          return Promise.resolve(null);
        default:
          return Promise.resolve(null);
      }
    });

    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: /\[skip\]/i }));

    await waitFor(() => {
      expect(screen.getByText(/create your first agent to begin/i)).toBeInTheDocument();
    });
  });

  it("routes to identity conflict when legacy identity is detected", async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      switch (cmd) {
        case "get_active_agent":
          return Promise.resolve(null);
        case "get_identities":
          return Promise.resolve([]);
        case "check_existing_identity":
          return Promise.resolve({
            name: "Legacy Abigail",
            birth_date: "2026-01-01",
            data_path: "C:/tmp/legacy",
            has_memories: true,
            has_signatures: true,
          });
        default:
          return Promise.resolve(null);
      }
    });

    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: /\[skip\]/i }));

    await waitFor(() => {
      expect(screen.getByText(/consciousness detected/i)).toBeInTheDocument();
    });
  });
});

