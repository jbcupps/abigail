import { createContext, useContext, useState, useEffect, useCallback, useRef, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";

export type PersonaMode = "id" | "ego" | "neutral";

interface EntityTheme {
  primary_color: string | null;
  avatar_url: string | null;
}

interface ThemeContextValue {
  mode: PersonaMode;
  setMode: (mode: PersonaMode) => void;
  agentName: string | null;
  primaryColor: string | null;
  avatarUrl: string | null;
  refreshAgentName: () => Promise<void>;
  refreshTheme: () => Promise<void>;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

interface ThemeProviderProps {
  initialMode?: PersonaMode;
  children: ReactNode;
}

export function ThemeProvider({ initialMode = "neutral", children }: ThemeProviderProps) {
  const [mode, setMode] = useState<PersonaMode>(initialMode);
  const [agentName, setAgentName] = useState<string | null>(null);
  const [primaryColor, setPrimaryColor] = useState<string | null>(null);
  const [avatarUrl, setAvatarUrl] = useState<string | null>(null);
  const mountedRef = useRef(true);

  // Apply theme class and CSS variables to document root
  useEffect(() => {
    const root = document.documentElement;
    root.classList.remove("theme-id", "theme-ego", "theme-neutral");
    root.classList.add(`theme-${mode}`);

    if (primaryColor) {
      root.style.setProperty("--theme-primary", primaryColor);
      // Generate a dimmer version for borders/glows if it's hex
      if (primaryColor.startsWith("#") && primaryColor.length === 7) {
        root.style.setProperty("--theme-primary-dim", primaryColor + "cc");
        root.style.setProperty("--theme-primary-faint", primaryColor + "33");
      }
    } else {
      root.style.removeProperty("--theme-primary");
      root.style.removeProperty("--theme-primary-dim");
      root.style.removeProperty("--theme-primary-faint");
    }
  }, [mode, primaryColor]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const refreshTheme = useCallback(async () => {
    try {
      const theme = await invoke<EntityTheme>("get_entity_theme");
      if (!mountedRef.current) return;
      setPrimaryColor(theme.primary_color);
      setAvatarUrl(theme.avatar_url);
    } catch (e) {
      console.warn("[ThemeContext] refreshTheme failed:", e);
    }
  }, []);

  const refreshAgentName = useCallback(async () => {
    try {
      const name = await invoke<string | null>("get_agent_name");
      if (!mountedRef.current) return;
      setAgentName(name);
      // When name is refreshed, also refresh theme as they usually change together (e.g. on load)
      await refreshTheme();
    } catch (e) {
      // Ignore - agent name not yet set
      console.warn("[ThemeContext] refreshAgentName failed:", e);
    }
  }, [refreshTheme]);

  return (
    <ThemeContext.Provider value={{ 
      mode, 
      setMode, 
      agentName, 
      primaryColor, 
      avatarUrl, 
      refreshAgentName, 
      refreshTheme 
    }}>
      {children}
    </ThemeContext.Provider>
  );
}

export function useTheme(): ThemeContextValue {
  const ctx = useContext(ThemeContext);
  if (!ctx) throw new Error("useTheme must be used within ThemeProvider");
  return ctx;
}
