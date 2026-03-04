import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTheme, type ThemeId } from "../contexts/ThemeContext";

interface ThemeDrawerProps {
  onClose: () => void;
}

interface ThemeInfo {
  id: string;
  name: string;
  description: string;
  mode: string;
}

const THEME_PREVIEWS: Record<string, { bg: string; fg: string; accent: string; surface: string; border: string }> = {
  modern: { bg: "#0f1419", fg: "#e2e8f0", accent: "#6366f1", surface: "#1a2028", border: "#334155" },
  phosphor: { bg: "#0a0a0a", fg: "#33ff33", accent: "#33ff33", surface: "#111111", border: "#1a5c1a" },
  classic: { bg: "#c0c0c0", fg: "#000000", accent: "#000080", surface: "#d4d0c8", border: "#808080" },
};

function ThemePreviewCard({ theme, active, onClick }: { theme: ThemeInfo; active: boolean; onClick: () => void }) {
  const p = THEME_PREVIEWS[theme.id] ?? THEME_PREVIEWS.modern;

  return (
    <button
      onClick={onClick}
      className={`w-full text-left rounded-theme-md border-2 transition-all overflow-hidden ${
        active
          ? "border-theme-primary shadow-theme-elevated"
          : "border-theme-border-dim hover:border-theme-border"
      }`}
    >
      {/* Mini preview */}
      <div
        className="px-3 py-2.5 space-y-1.5"
        style={{ backgroundColor: p.bg }}
      >
        <div className="flex items-center gap-2 mb-1">
          <div className="w-2 h-2 rounded-full" style={{ backgroundColor: p.accent }} />
          <div className="text-[10px] font-bold tracking-wide" style={{ color: p.fg }}>
            {theme.name}
          </div>
          <div
            className="ml-auto text-[8px] uppercase tracking-widest px-1.5 py-0.5 rounded"
            style={{ backgroundColor: p.surface, color: p.accent, border: `1px solid ${p.border}` }}
          >
            {theme.mode}
          </div>
        </div>
        <div className="space-y-1">
          <div className="h-1.5 rounded-full w-3/4" style={{ backgroundColor: p.accent, opacity: 0.4 }} />
          <div className="flex gap-1">
            <div className="h-6 flex-1 rounded-sm" style={{ backgroundColor: p.surface, border: `1px solid ${p.border}` }} />
            <div className="h-6 flex-1 rounded-sm" style={{ backgroundColor: p.surface, border: `1px solid ${p.border}` }} />
          </div>
          <div className="h-1 rounded-full w-1/2" style={{ backgroundColor: p.fg, opacity: 0.2 }} />
        </div>
      </div>

      {/* Label */}
      <div className="px-3 py-2 bg-theme-bg-elevated border-t border-theme-border-dim">
        <div className="flex items-center justify-between">
          <span className="text-xs text-theme-text font-primary">{theme.description}</span>
          {active && (
            <span className="text-[10px] text-theme-primary font-bold uppercase tracking-wider">Active</span>
          )}
        </div>
      </div>
    </button>
  );
}

export default function ThemeDrawer({ onClose }: ThemeDrawerProps) {
  const { setThemeId } = useTheme();
  const [themes, setThemes] = useState<ThemeInfo[]>([]);
  const [hiveDefault, setHiveDefault] = useState<string>("modern");

  useEffect(() => {
    (async () => {
      try {
        const [list, hive] = await Promise.all([
          invoke<ThemeInfo[]>("list_available_themes"),
          invoke<string>("get_hive_theme"),
        ]);
        setThemes(list);
        setHiveDefault(hive);
      } catch (e) {
        console.warn("[ThemeDrawer] failed to load themes:", e);
      }
    })();
  }, []);

  const handleSelect = async (id: string) => {
    setHiveDefault(id);
    try {
      await invoke("set_hive_theme", { themeId: id });
      await setThemeId(id as ThemeId);
    } catch (e) {
      console.warn("[ThemeDrawer] set_hive_theme failed:", e);
    }
  };

  return (
    <>
      <div
        className="fixed inset-0 bg-theme-overlay z-40 transition-opacity"
        onClick={onClose}
      />

      <div className="fixed top-0 left-0 h-full w-[420px] max-w-[90vw] bg-theme-bg border-r border-theme-border z-50 flex flex-col">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-theme-border shrink-0">
          <div>
            <h2 className="text-sm font-bold text-theme-text-bright font-primary uppercase tracking-widest">Theme</h2>
            <p className="text-xs text-theme-text-dim mt-0.5">Hive default for new entities</p>
          </div>
          <button
            onClick={onClose}
            className="text-theme-text-dim hover:text-theme-text text-xl leading-none px-1"
          >
            &times;
          </button>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto p-4 space-y-3">
          {themes.map((t) => (
            <ThemePreviewCard
              key={t.id}
              theme={t}
              active={hiveDefault === t.id}
              onClick={() => handleSelect(t.id)}
            />
          ))}

          {themes.length === 0 && (
            <div className="text-xs text-theme-text-dim text-center py-8">Loading themes...</div>
          )}
        </div>

        {/* Footer */}
        <div className="px-4 py-3 border-t border-theme-border-dim text-[10px] text-theme-text-dim text-center">
          New entities inherit the hive theme at creation
        </div>
      </div>
    </>
  );
}
