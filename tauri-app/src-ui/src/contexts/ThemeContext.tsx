import { createContext, useContext, useState, useEffect, useCallback, useRef, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";

export type PersonaMode = "id" | "ego" | "neutral";

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

export function ThemeProvider({ initialMode = "neutral", children }: ThemeProviderProps) {
  const [mode, setMode] = useState<PersonaMode>(initialMode);
  const [agentName, setAgentName] = useState<string | null>(null);
  const mountedRef = useRef(true);

  // Apply theme class to document root
  useEffect(() => {
    const root = document.documentElement;
    root.classList.remove("theme-id", "theme-ego", "theme-neutral");
    root.classList.add(`theme-${mode}`);
  }, [mode]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const refreshAgentName = useCallback(async () => {
    try {
      const name = await invoke<string | null>("get_agent_name");
      if (!mountedRef.current) return;
      setAgentName(name);
    } catch (e) {
      // Ignore - agent name not yet set
      console.warn("[ThemeContext] refreshAgentName failed:", e);
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
