/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_ENABLE_EXPERIMENTAL_UI?: string;
  readonly VITE_CHAT_TRANSPORT?: string;
  readonly VITE_ENTITY_DAEMON_URL?: string;
  readonly VITE_HIVE_DAEMON_URL?: string;
  readonly VITE_HIVE_CONTROL_TOKEN?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
