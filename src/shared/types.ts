export type AppMode = "fast" | "polish";
export type InsertMode = "paste" | "clipboard_only";
export type WhisperModelId = "tiny.en" | "base.en" | "small.en";
export type LlmModelId = "llama-3.2-1b" | "qwen2.5-1.5b";
export type AppUiState = "idle" | "recording" | "processing" | "downloading";
export type PermissionKind = "microphone" | "accessibility";

export interface AppConfig {
  hotkey: string;
  mode: AppMode;
  whisper_model: WhisperModelId;
  llm_model: LlmModelId;
  input_device: string | null;
  insert_mode: InsertMode;
  first_run_complete: boolean;
  settings_version: number;
}

export interface AudioDevice {
  id: string;
  name: string;
  is_default: boolean;
}

export interface ModelFileStatus {
  id: string;
  filename: string;
  present: boolean;
  size_bytes: number | null;
  required: boolean;
}

export interface ModelStatus {
  whisper: ModelFileStatus[];
  llm: ModelFileStatus[];
  ready_for_fast: boolean;
  ready_for_polish: boolean;
}

export interface PermissionsStatus {
  microphone: boolean;
  accessibility: boolean;
  input_monitoring: boolean;
  wayland: boolean;
  hotkey_listening: boolean;
  /** Bare-key bindings need the raw key stream, which macOS gates behind Accessibility. */
  hotkey_needs_accessibility: boolean;
}

export interface AudioLevelPayload {
  level: number;
}

export interface AppStatePayload {
  state: AppUiState;
}

export interface DonePayload {
  text: string;
}

export interface ErrorPayload {
  message: string;
}

export interface DownloadProgressPayload {
  model: string;
  downloaded: number;
  total: number;
}

export interface PermissionNeededPayload {
  kind: PermissionKind;
  message: string;
}

export interface HotkeyCapturedPayload {
  hotkey: string;
}

export const WHISPER_MODELS: {
  id: WhisperModelId;
  label: string;
  hint: string;
}[] = [
  { id: "tiny.en", label: "tiny.en", hint: "~75 MB · fastest, less accurate" },
  { id: "base.en", label: "base.en", hint: "~142 MB · default, good balance" },
  { id: "small.en", label: "small.en", hint: "~466 MB · slower, more accurate" },
];

export const LLM_MODELS: {
  id: LlmModelId;
  label: string;
  hint: string;
}[] = [
  { id: "llama-3.2-1b", label: "Llama 3.2 1B", hint: "~800 MB · default polish model" },
  { id: "qwen2.5-1.5b", label: "Qwen2.5 1.5B", hint: "~1.1 GB · slightly stronger cleanup" },
];

export const DEFAULT_CONFIG: AppConfig = {
  hotkey: "Alt+Space",
  mode: "fast",
  whisper_model: "base.en",
  llm_model: "llama-3.2-1b",
  input_device: null,
  insert_mode: "paste",
  first_run_complete: false,
  settings_version: 1,
};
