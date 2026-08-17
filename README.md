# Dictate

Local, open-source push-to-talk voice-to-text for macOS, Windows, and Linux. Hold a hotkey, speak, release, and the transcript is pasted into whatever app is focused. No accounts and no cloud calls — Whisper and an optional on-device LLM run on your machine.

## Requirements

- Node.js 18+
- Rust (stable)
- CMake (needed to compile `whisper.cpp` and `llama.cpp`)
- On macOS: Xcode Command Line Tools

## Run

```bash
npm install
npm run tauri dev
```

The app lives in the system tray (no main window). Open **Settings…** from the tray icon on first launch.

## First run

1. Grant **Microphone** access (the app prompts).
2. On macOS, grant **Accessibility** so Dictate can paste into other apps (`System Settings → Privacy & Security → Accessibility`).
3. Download the default models from Settings (Whisper `base.en` and Llama 3.2 1B GGUF, from Hugging Face). After that, everything works offline.

## Usage

- Default hotkey: **⌥ Space** on macOS, **Right Ctrl** elsewhere. Rebind in Settings.
- Hold the hotkey to record, release to transcribe and insert.
- **Fast** mode: Whisper + light cleanup.
- **Polish** mode: also runs the local LLM to fix grammar and filler words.
- If simulated paste is blocked (some Wayland compositors), switch **Insert mode** to clipboard-only.

### Choosing a hotkey

Bindings come in two shapes, and the shape decides what the app needs from the OS.

| Shape | Example | Needs Accessibility | How it is observed |
| --- | --- | --- | --- |
| Combo, or a bare function key | `⌥ Space`, `⌃⇧ D`, `F8` | No | `RegisterEventHotKey` (macOS) via a global shortcut |
| Lone modifier | Right Ctrl, Fn, Caps Lock | Yes, on macOS | A `CGEventTap` on the raw key stream |

Combos are the default because they work the moment the app is installed. A lone modifier feels nicer to hold, but macOS will not show those keys to an app until Accessibility is granted, and that grant is tied to the exact binary — so after rebuilding from source you have to remove and re-add Dictate in `System Settings → Privacy & Security → Accessibility`.

Fn/Globe cannot be bound at all: macOS routes it to its own handler before any app sees it. A stored Fn binding is migrated to the default on launch.

## Icons

`assets/icon-source.png` is the artwork. To regenerate every icon from it:

```bash
python3 -m venv .venv-icons && .venv-icons/bin/pip install Pillow
.venv-icons/bin/python scripts/make-icons.py
npx tauri icon assets/icon-app.png
```

The script does the two things `tauri icon` cannot: it composites the artwork onto an Apple-style rounded square (macOS does not round app icons for you), and it renders a black-plus-alpha template image for the menu bar, which the system recolours for the light or dark bar.

## Build

```bash
npm run tauri build
```

Packaged builds land under `src-tauri/target/release/bundle/`. A macOS build is also at:

- `release/Dictate.app`
- `release/Dictate_0.1.0_aarch64.dmg`

Models are stored in the OS app-data directory (`~/Library/Application Support/Dictate/models` on macOS). Config is TOML at the OS config directory (`~/Library/Application Support/Dictate/config.toml` on macOS).
