import { createContext, useContext, useState, useEffect, useCallback, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";

export type PersonaMode = "id" | "ego";

interface ThemeContextValue {
  mode: PersonaMode;
  setMode: (mode: PersonaMode) => void;
  agentName: string | null;
  refreshAgentName: () => Promise<void>;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

interface ThemeProviderProps {
  initialMode?: PersonaMode;
  children: ReactNode;
}

export function ThemeProvider({ initialMode = "id", children }: ThemeProviderProps) {
  const [mode, setMode] = useState<PersonaMode>(initialMode);
  const [agentName, setAgentName] = useState<string | null>(null);

  // Apply theme class to document root
  useEffect(() => {
    const root = document.documentElement;
    root.classList.remove("theme-id", "theme-ego");
    root.classList.add(`theme-${mode}`);
  }, [mode]);

  const refreshAgentName = useCallback(async () => {
    try {
      const name = await invoke<string | null>("get_agent_name");
      setAgentName(name);
    } catch {
      // Ignore - agent name not yet set
    }
  }, []);

  return (
    <ThemeContext.Provider value={{ mode, setMode, agentName, refreshAgentName }}>
      {children}
    </ThemeContext.Provider>
  );
}

export function useTheme(): ThemeContextValue {
  const ctx = useContext(ThemeContext);
  if (!ctx) throw new Error("useTheme must be used within ThemeProvider");
  return ctx;
}
