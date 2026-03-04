import { createContext, useContext, useState, useEffect, useCallback, useRef, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";

const VALID_THEMES = ["modern", "phosphor", "classic"] as const;
export type ThemeId = (typeof VALID_THEMES)[number];

interface EntityTheme {
  primary_color: string | null;
  avatar_url: string | null;
  theme_id: string | null;
}

interface ThemeContextValue {
  themeId: ThemeId;
  setThemeId: (id: ThemeId) => Promise<void>;
  agentName: string | null;
  primaryColor: string | null;
  avatarUrl: string | null;
  refreshAgentName: () => Promise<void>;
  refreshTheme: () => Promise<void>;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

interface ThemeProviderProps {
  children: ReactNode;
}

function isValidTheme(id: string | null | undefined): id is ThemeId {
  return typeof id === "string" && (VALID_THEMES as readonly string[]).includes(id);
}

function applyThemeClass(themeId: ThemeId) {
  const root = document.documentElement;
  for (const t of VALID_THEMES) {
    root.classList.remove(`theme-${t}`);
  }
  root.classList.add(`theme-${themeId}`);
}

function applyAccentOverride(primaryColor: string | null) {
  const root = document.documentElement;
  if (primaryColor) {
    root.style.setProperty("--color-primary", primaryColor);
    if (primaryColor.startsWith("#") && primaryColor.length === 7) {
      root.style.setProperty("--color-primary-dim", primaryColor + "cc");
      root.style.setProperty("--color-primary-glow", primaryColor + "26");
    }
  } else {
    root.style.removeProperty("--color-primary");
    root.style.removeProperty("--color-primary-dim");
    root.style.removeProperty("--color-primary-glow");
  }
}

export function ThemeProvider({ children }: ThemeProviderProps) {
  const [themeId, setThemeIdState] = useState<ThemeId>("modern");
  const [agentName, setAgentName] = useState<string | null>(null);
  const [primaryColor, setPrimaryColor] = useState<string | null>(null);
  const [avatarUrl, setAvatarUrl] = useState<string | null>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    applyThemeClass(themeId);
  }, [themeId]);

  useEffect(() => {
    applyAccentOverride(primaryColor);
  }, [primaryColor]);

  useEffect(() => {
    mountedRef.current = true;
    return () => { mountedRef.current = false; };
  }, []);

  const setThemeId = useCallback(async (id: ThemeId) => {
    setThemeIdState(id);
    try {
      await invoke("set_entity_theme_id", { themeId: id });
    } catch (e) {
      console.warn("[ThemeContext] set_entity_theme_id failed:", e);
    }
  }, []);

  const refreshTheme = useCallback(async () => {
    try {
      const theme = await invoke<EntityTheme>("get_entity_theme");
      if (!mountedRef.current) return;
      setPrimaryColor(theme.primary_color);
      setAvatarUrl(theme.avatar_url);
      if (isValidTheme(theme.theme_id)) {
        setThemeIdState(theme.theme_id);
      }
    } catch {
      try {
        const hiveTheme = await invoke<string>("get_hive_theme");
        if (!mountedRef.current) return;
        if (isValidTheme(hiveTheme)) {
          setThemeIdState(hiveTheme);
        }
      } catch (e2) {
        console.warn("[ThemeContext] get_hive_theme fallback failed:", e2);
      }
    }
  }, []);

  const refreshAgentName = useCallback(async () => {
    try {
      const name = await invoke<string | null>("get_agent_name");
      if (!mountedRef.current) return;
      setAgentName(name);
      await refreshTheme();
    } catch (e) {
      console.warn("[ThemeContext] refreshAgentName failed:", e);
    }
  }, [refreshTheme]);

  return (
    <ThemeContext.Provider value={{
      themeId,
      setThemeId,
      agentName,
      primaryColor,
      avatarUrl,
      refreshAgentName,
      refreshTheme,
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
