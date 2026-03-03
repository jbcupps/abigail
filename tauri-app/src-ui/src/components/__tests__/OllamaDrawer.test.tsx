import "../../test/tauri-mock";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import OllamaDrawer from "../OllamaDrawer";

const mockInvoke = invoke as unknown as Mock;

describe("OllamaDrawer", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "detect_ollama") return Promise.resolve({ status: "running", path: "/usr/bin/ollama" });
      if (cmd === "list_recommended_models") return Promise.resolve([
        { name: "llama3.2:3b", label: "Small", size_bytes: 2_000_000_000, description: "Fast small model", recommended: true },
      ]);
      if (cmd === "get_config_snapshot") return Promise.resolve({ bundled_model: "llama3.2:3b" });
      if (cmd === "list_ollama_models") return Promise.resolve([
        { name: "llama3.2:3b", size: 2_000_000_000, modified_at: "2025-01-01" },
      ]);
      return Promise.resolve(null);
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("is hidden when not open", () => {
    render(<OllamaDrawer open={false} onClose={() => {}} />);
    const drawer = screen.getByTestId("ollama-drawer");
    expect(drawer.className).toContain("-translate-x-full");
  });

  it("is visible when open", () => {
    render(<OllamaDrawer open={true} onClose={() => {}} />);
    const drawer = screen.getByTestId("ollama-drawer");
    expect(drawer.className).toContain("translate-x-0");
  });

  it("calls detect_ollama and list_recommended_models on open", async () => {
    render(<OllamaDrawer open={true} onClose={() => {}} />);

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("detect_ollama");
      expect(mockInvoke).toHaveBeenCalledWith("list_recommended_models");
      expect(mockInvoke).toHaveBeenCalledWith("get_config_snapshot");
    });
  });

  it("shows installed models when ollama is running", async () => {
    render(<OllamaDrawer open={true} onClose={() => {}} />);

    await waitFor(() => {
      expect(screen.getByText("Running")).toBeInTheDocument();
      expect(screen.getByText("llama3.2:3b")).toBeInTheDocument();
    });
  });

  it("fires onClose when backdrop is clicked", async () => {
    const onClose = vi.fn();
    const user = userEvent.setup();
    render(<OllamaDrawer open={true} onClose={onClose} />);

    const backdrop = screen.getByTestId("ollama-drawer-backdrop");
    await user.click(backdrop);
    expect(onClose).toHaveBeenCalled();
  });

  it("fires onClose when close button is clicked", async () => {
    const onClose = vi.fn();
    const user = userEvent.setup();
    render(<OllamaDrawer open={true} onClose={onClose} />);

    const closeBtn = screen.getByLabelText("Close drawer");
    await user.click(closeBtn);
    expect(onClose).toHaveBeenCalled();
  });

  it("shows install button when ollama is not found", async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "detect_ollama") return Promise.resolve({ status: "not_found", path: null });
      if (cmd === "list_recommended_models") return Promise.resolve([]);
      if (cmd === "get_config_snapshot") return Promise.resolve({});
      return Promise.resolve(null);
    });

    render(<OllamaDrawer open={true} onClose={() => {}} />);

    await waitFor(() => {
      expect(screen.getByText("Not Found")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Install Ollama" })).toBeInTheDocument();
    });
  });

  it("shows start button when ollama is installed but not running", async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "detect_ollama") return Promise.resolve({ status: "installed", path: "/usr/bin/ollama" });
      if (cmd === "list_recommended_models") return Promise.resolve([]);
      if (cmd === "get_config_snapshot") return Promise.resolve({});
      return Promise.resolve(null);
    });

    render(<OllamaDrawer open={true} onClose={() => {}} />);

    await waitFor(() => {
      expect(screen.getByText("Installed")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Start Ollama" })).toBeInTheDocument();
    });
  });
});
