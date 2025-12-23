/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_SERVER_URL?: string;
  readonly VITE_CERT_HASH?: string;
  readonly VITE_IS_DEVELOPMENT?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
