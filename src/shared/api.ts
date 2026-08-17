import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppConfig,
  AudioDevice,
  AudioLevelPayload,
  AppStatePayload,
  DonePayload,
  DownloadProgressPayload,
  ErrorPayload,
  HotkeyCapturedPayload,
  ModelStatus,
  PermissionNeededPayload,
  PermissionsStatus,
} from "./types";

export const api = {
  getConfig: () => invoke<AppConfig>("get_config"),
  saveConfig: (config: AppConfig) => invoke<AppConfig>("save_config", { config }),
  listInputDevices: () => invoke<AudioDevice[]>("list_input_devices"),
  getModelStatus: () => invoke<ModelStatus>("get_model_status"),
  startModelDownload: () => invoke<void>("start_model_download"),
  checkPermissions: () => invoke<PermissionsStatus>("check_permissions"),
  requestPermissions: () => invoke<PermissionsStatus>("request_permissions"),
  requestAccessibility: () => invoke<PermissionsStatus>("request_accessibility"),
  openAccessibilitySettings: () => invoke<void>("open_accessibility_settings"),
  beginHotkeyCapture: () => invoke<void>("begin_hotkey_capture"),
  cancelHotkeyCapture: () => invoke<void>("cancel_hotkey_capture"),
  setHotkey: (hotkey: string) => invoke<string>("set_hotkey", { hotkey }),
  retryLastInsert: () => invoke<void>("retry_last_insert"),
};

export const events = {
  appState: (cb: (p: AppStatePayload) => void) =>
    listen<AppStatePayload>("app-state", (e) => cb(e.payload)),
  audioLevel: (cb: (p: AudioLevelPayload) => void) =>
    listen<AudioLevelPayload>("audio-level", (e) => cb(e.payload)),
  recordingStarted: (cb: () => void) =>
    listen("recording-started", () => cb()),
  processing: (cb: () => void) => listen("processing", () => cb()),
  done: (cb: (p: DonePayload) => void) =>
    listen<DonePayload>("done", (e) => cb(e.payload)),
  error: (cb: (p: ErrorPayload) => void) =>
    listen<ErrorPayload>("error", (e) => cb(e.payload)),
  downloadProgress: (cb: (p: DownloadProgressPayload) => void) =>
    listen<DownloadProgressPayload>("download-progress", (e) => cb(e.payload)),
  permissionNeeded: (cb: (p: PermissionNeededPayload) => void) =>
    listen<PermissionNeededPayload>("permission-needed", (e) => cb(e.payload)),
  configUpdated: (cb: (p: AppConfig) => void) =>
    listen<AppConfig>("config-updated", (e) => cb(e.payload)),
  hotkeyCaptured: (cb: (p: HotkeyCapturedPayload) => void) =>
    listen<HotkeyCapturedPayload>("hotkey-captured", (e) => cb(e.payload)),
};

export type { UnlistenFn };
