export type AppPhase =
  | "idle"
  | "starting"
  | "listening"
  | "transcribing"
  | "finalizing"
  | "done"
  | "error";
export type ActivationMode = "toggle" | "push-to-talk";

export type AudioDeviceSelection =
  | { mode: "automatic" }
  | { mode: "system_default" }
  | { mode: "specific"; device_id: string; device_name: string };

export interface AudioDeviceInfo {
  id: string;
  name: string;
  transport: string;
  isDefault: boolean;
  isBuiltIn: boolean;
}

export interface CustomVocabEntry {
  value: string;
  pronunciations?: string[];
  language?: string;
  intensity?: number;
}

export interface AppStatus {
  phase: AppPhase;
  title: string;
  detail: string;
}

export interface AppSettings {
  apiKey: string;
  languages: string[];
  activationMode: ActivationMode;
  audioDevice: string;
  audioDeviceSelection: AudioDeviceSelection;
  hotkey: string;
  codeSwitching: boolean;
  copyToClipboard: boolean;
  /** Type partial transcripts while speaking, corrected by each final. */
  liveTyping: boolean;
  endpointing: number;
  customVocabulary: CustomVocabEntry[];
}

export interface ApiBaseUrlSettings {
  /** Custom Gladia API base URL; `null` means use the default. */
  apiBaseUrl: string | null;
  /** The URL requests actually go to, given the current settings. */
  effectiveApiBaseUrl: string;
  defaultApiBaseUrl: string;
}

/** The live models the current API base URL offers (`list_live_models`). */
export interface LiveModels {
  models: string[];
  /** Whether the host listed them itself; otherwise a built-in fallback. */
  discovered: boolean;
}

export type LiveModelOptions =
  | { status: "loading" }
  | ({ status: "ready" } & LiveModels)
  | { status: "error"; message: string };

export interface BootstrapState {
  settings: AppSettings;
  apiKeySet: boolean;
  ready: boolean;
  status: AppStatus;
}

export interface TranscriptionHistoryEntry {
  id: string;
  text: string;
  created_at: string;
  /** API base URL the session ran on; null for entries saved before it was recorded. */
  api_base_url: string | null;
  /** Gladia dashboard page of the session; null when its API host has none. */
  open_url: string | null;
}

export interface TranscriptionHistoryPage {
  items: TranscriptionHistoryEntry[];
  total: number;
  page: number;
  page_size: number;
}
