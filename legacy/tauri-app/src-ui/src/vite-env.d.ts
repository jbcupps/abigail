/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_ENABLE_EXPERIMENTAL_UI?: string;
  readonly VITE_CHAT_TRANSPORT?: string;
  readonly VITE_ENTITY_DAEMON_URL?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
