# Build Prompt: Local Voice-to-Text Desktop App (Tauri + Rust)

## Goal
Build a cross-platform (macOS, Windows, Linux) desktop app similar to **Wispr Flow**, but fully local/open-source. The user holds a global hotkey, speaks, releases the hotkey, and the transcribed (optionally LLM-polished) text is inserted at the current cursor position in whatever app is focused. No accounts, no login, no cloud calls — everything runs on-device.

## Tech Stack
- **App shell:** Tauri v2 (Rust backend + Vite/React frontend)
- **Frontend:** React + TypeScript + Tailwind CSS for both the floating pill and the settings window
- **Audio capture:** `cpal`
- **Global hotkey / key hold detection:** `rdev` (listens to raw key-down/key-up events cross-platform, since we need push-to-talk, not just "hotkey triggered")
- **Speech-to-text:** `whisper-rs` (Rust bindings to whisper.cpp), using quantized ggml models (`tiny.en`, `base.en`, `small.en` — user-selectable)
- **LLM cleanup (optional "Polish" mode):** `llama-cpp-rs` embedded in-process, running a quantized 1B–2B instruct model (e.g. Llama-3.2-1B-Instruct-Q4_K_M or Qwen2.5-1.5B-Instruct-Q4_K_M, GGUF format)
- **Clipboard + text injection:** `arboard` (clipboard) + `enigo` (simulated paste keystroke)
- **Config storage:** local TOML file via `dirs::config_dir()`, no database, no accounts

## Core Flow
1. App launches into system tray (no main window shown by default). Tray icon reflects state: idle / recording / processing.
2. User holds the configured hotkey (default: **Right Ctrl**; user can rebind in settings; on macOS, optionally allow binding to Fn if `rdev` exposes it on that keyboard).
3. On key-down:
   - Show the floating pill UI (see below).
   - Start audio capture via `cpal` into an in-memory buffer.
4. On key-up:
   - Stop capture, trim leading/trailing silence.
   - Pill switches to "processing" state (spinner).
   - Run the buffer through whisper-rs → raw transcript.
   - If "Polish mode" is on, pass the transcript through the local LLM with a fixed system prompt (see below); otherwise apply lightweight rule-based cleanup (capitalize sentences, strip filler words like "um"/"uh", basic punctuation).
   - Copy final text to clipboard, simulate paste (Cmd/Ctrl+V) into the currently focused field.
   - Pill fades out.
5. If mic permission or (on macOS) Accessibility permission is missing, show a clear one-time prompt explaining why it's needed and how to grant it, rather than failing silently.

## Floating Pill UI
- A separate small Tauri window: frameless, transparent background, `always_on_top`, `skip_taskbar`, non-resizable, positioned bottom-center of the active screen, ~180x48px rounded pill.
- States:
  - **Recording:** pulsing mic icon + simple live waveform/amplitude bars driven by audio input level.
  - **Processing:** small spinner.
  - **Idle:** window hidden entirely (not just transparent) when not in use, to avoid any visual footprint.
- No interaction required with the pill itself — it's purely a status indicator.

## Settings Window (minimal, opened from tray menu)
- Hotkey rebind control.
- Mode toggle: **Fast** (Whisper only + rules) vs **Polish** (+ LLM cleanup).
- Whisper model selector: tiny.en / base.en / small.en (show approximate RAM/speed tradeoff).
- LLM model selector (only relevant in Polish mode): 1B vs 1.5B/2B option.
- Input device picker (if multiple mics).
- No login, no account, no telemetry.

## LLM Cleanup System Prompt (for Polish mode)
Use a fixed system prompt roughly like: "You clean up raw speech-to-text transcripts. Fix grammar, punctuation, and casing. Remove filler words and false starts. Do not add new information, do not change the meaning, do not answer questions in the text — only clean it up. Return only the cleaned text, nothing else."

## First-Run Experience
- On first launch, download the default Whisper model (base.en) and default LLM model (1B Q4 GGUF) from Hugging Face with a progress bar. Store in app data dir. After this, the app works fully offline.
- Request mic permission; on macOS, explain and link to the Accessibility permission screen (needed for simulated paste).

## Platform Notes to Handle Explicitly
- **Wayland (Linux):** simulated paste via `enigo` may be blocked by the compositor. Detect Wayland vs X11 at runtime; on Wayland, show a warning that text auto-insert may not work and offer "copy to clipboard only" as a fallback mode.
- **macOS:** requires both Microphone and Accessibility permissions; must handle the case where the app isn't yet in System Settings' Accessibility list.
- **Windows:** should work out of the box with `enigo`.

## Frontend Integration Notes (React + Tailwind + TypeScript)
- The pill and settings window are **separate Tauri windows**, so set up Vite as a multi-page app with two HTML entry points (`pill.html`, `settings.html`), each mounting its own React root.
- Use `@tauri-apps/api` (`invoke`, `listen`) for all Rust↔React communication — e.g. Rust emits `recording-started` / `audio-level` / `processing` / `done` events that the pill listens for via `listen()`, and React never talks to `cpal`/`whisper-rs` directly.
- Tailwind config should scan both entry points; keep the pill's Tailwind classes minimal since it renders on every keypress and needs to stay lightweight/instant.
- Type the Tauri event payloads and command return values in `shared/types.ts` so both windows and the Rust side agree on shapes (mirror these as `#[derive(Serialize)]` structs in Rust).

## Explicit Non-Goals (Phase 2, don't build now)
- Streaming/live partial transcripts while speaking.
- Custom vocabulary / user dictionary.
- Multi-language auto-detect (ship English-only first).
- Voice commands (e.g. "new paragraph", "delete last sentence").

## Deliverable Structure
Scaffold as a standard Tauri v2 project:
```
src-tauri/
  src/
    main.rs
    audio.rs         (cpal capture)
    hotkey.rs         (rdev listener, push-to-talk state machine)
    stt.rs            (whisper-rs wrapper)
    llm.rs            (llama-cpp-rs wrapper, polish-mode prompt)
    inject.rs         (clipboard + paste simulation, platform branches)
    config.rs         (TOML load/save)
    tray.rs
  tauri.conf.json
src/                    (React + TypeScript, Tailwind CSS)
  pill/                 (floating pill UI component + entry)
  settings/             (settings window UI component + entry)
  shared/               (shared types, Tauri command bindings, hooks)
tailwind.config.ts
vite.config.ts
tsconfig.json
```

Build this incrementally: (1) hotkey + audio capture + whisper transcript + clipboard paste working end-to-end in Fast mode first, (2) then add the pill UI, (3) then add Polish mode with the local LLM, (4) then settings window and first-run model download flow.