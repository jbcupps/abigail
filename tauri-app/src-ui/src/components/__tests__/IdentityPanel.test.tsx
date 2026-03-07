import "../../test/tauri-mock";
import { render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import IdentityPanel from "../IdentityPanel";

vi.mock("../../contexts/ThemeContext", () => ({
  useTheme: () => ({
    themeId: "modern",
    setThemeId: vi.fn(),
    agentName: "Abigail",
    primaryColor: "#00ffcc",
    avatarUrl: null,
    refreshAgentName: vi.fn(),
    refreshTheme: vi.fn(),
  }),
}));

const mockInvoke = invoke as unknown as Mock;

describe("IdentityPanel", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    mockInvoke.mockImplementation((cmd: string) => {
      switch (cmd) {
        case "get_router_status":
          return Promise.resolve({
            id_provider: "local_http",
            id_url: "http://127.0.0.1:11434",
            ego_configured: true,
            routing_mode: "selector",
          });
        case "get_stored_providers":
          return Promise.resolve([]);
        case "list_skills_vault_entries":
          return Promise.resolve([
            {
              secret_name: "github_token",
              skill_names: ["GitHub API"],
              description: "Access token for GitHub API calls.",
              is_set: false,
            },
            {
              secret_name: "github_owner",
              skill_names: ["GitHub API"],
              description: "Default GitHub owner or organization.",
              is_set: true,
            },
            {
              secret_name: "jira_api_token",
              skill_names: ["Jira"],
              description: "Token for Jira API access.",
              is_set: true,
            },
            {
              secret_name: "shared_search_key",
              skill_names: ["Perplexity Search", "Web Search"],
              description: "Shared search provider credential.",
              is_set: false,
            },
            {
              secret_name: "legacy_secret",
              skill_names: [],
              description: "Imported from an older manifest.",
              is_set: false,
            },
          ]);
        default:
          return Promise.resolve(null);
      }
    });
  });

  it("groups skill secrets by skill and breaks out shared entries", async () => {
    render(<IdentityPanel initialTab="keys" embedded />);

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("list_skills_vault_entries");
    });

    expect(await screen.findByRole("heading", { name: "GitHub API" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Jira" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Shared Across Skills" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Unassigned" })).toBeInTheDocument();

    expect(screen.getByText("github_token")).toBeInTheDocument();
    expect(screen.getByText("jira_api_token")).toBeInTheDocument();
    expect(screen.getByText("shared_search_key")).toBeInTheDocument();
    expect(screen.getByText("Perplexity Search, Web Search")).toBeInTheDocument();
    expect(screen.getByText("legacy_secret")).toBeInTheDocument();
  });
});
