import { detectRuntimeMode } from "../runtimeMode";

// Open an Entity's chat window. This drives the native `open_entity` command in
// the Hive shell, which starts the entity's daemon on demand and launches the
// Entity Runtime app pointed at it. Only meaningful in the packaged desktop app.
export async function openEntity(entityId: string): Promise<void> {
  if (detectRuntimeMode() !== "native") {
    throw new Error("Opening an Entity window is only available in the desktop app.");
  }
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("open_entity", { entityId });
}
