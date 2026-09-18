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
  useScreenContextVocabulary: boolean;
  useScreenContextOcr: boolean;
  endpointing: number;
  customVocabulary: CustomVocabEntry[];
}

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
}

export interface TranscriptionHistoryPage {
  items: TranscriptionHistoryEntry[];
  total: number;
  page: number;
  page_size: number;
}
