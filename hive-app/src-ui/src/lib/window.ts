import { detectRuntimeMode } from "../runtimeMode";

// Reveal the OS window (which starts hidden via tauri.conf `visible:false`) once
// the app has painted its first frame, so the user never sees a white flash —
// the window appears already showing the splash. No-op in the browser/dev path.
export async function showAppWindow(): Promise<void> {
  if (detectRuntimeMode() !== "native") return;
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().show();
  } catch {
    // Browser/dev runtime, or the show permission isn't granted — ignore.
  }
}
