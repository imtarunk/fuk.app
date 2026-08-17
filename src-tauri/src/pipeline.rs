use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition};

use crate::audio;
use crate::cleanup;
use crate::config::{AppMode, AppUiState};
use crate::download;
use crate::inject;
use crate::llm::LlmEngine;
use crate::permissions;
use crate::stt::WhisperEngine;
use crate::tray::{self, TooltipState};
use crate::AppState;

const MIN_SAMPLES_16K: usize = 16_000 / 4; // ~0.25s

#[derive(Clone, Serialize)]
pub struct AppStatePayload {
    pub state: AppUiState,
}

#[derive(Clone, Serialize)]
pub struct AudioLevelPayload {
    pub level: f32,
}

#[derive(Clone, Serialize)]
pub struct DonePayload {
    pub text: String,
}

#[derive(Clone, Serialize)]
pub struct ErrorPayload {
    pub message: String,
}

#[derive(Clone, Serialize)]
pub struct PermissionNeededPayload {
    pub kind: &'static str,
    pub message: String,
}

pub fn emit_app_state(app: &AppHandle, state: AppUiState) {
    let _ = app.emit("app-state", AppStatePayload { state });
    let tip = match state {
        AppUiState::Recording => TooltipState::Recording,
        AppUiState::Processing => TooltipState::Processing,
        AppUiState::Idle | AppUiState::Downloading => TooltipState::Idle,
    };
    tray::set_tooltip_state(app, tip);
}

pub fn emit_error(app: &AppHandle, message: impl Into<String>) {
    let message = message.into();
    log::error!("{message}");
    let _ = app.emit("error", ErrorPayload { message });
}

pub fn on_hotkey_down(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.processing.load(Ordering::SeqCst) || state.recording.load(Ordering::SeqCst) {
        return;
    }
    if state.capturing_hotkey.load(Ordering::SeqCst) {
        return;
    }

    crate::focus::capture_target();

    if !permissions::microphone_cached() {
        let _ = app.emit(
            "permission-needed",
            PermissionNeededPayload {
                kind: "microphone",
                message: "Microphone access is required to dictate. Enable it in Settings.".to_string(),
            },
        );
        tray::show_settings_on_main(app);
        return;
    }

    let config = state.config.lock().clone();
    if !download::models_ready(&config) {
        emit_error(
            app,
            "Required models are missing. Open Settings to download them.",
        );
        tray::show_settings_on_main(app);
        return;
    }

    // Opening the input stream costs tens of milliseconds, and the pill is the
    // only signal that the key registered, so put it on screen first.
    show_pill(app);
    let _ = app.emit("recording-started", ());
    emit_app_state(app, AppUiState::Recording);

    let app_for_level = app.clone();
    let on_level: Arc<dyn Fn(f32) + Send + Sync> = Arc::new(move |level: f32| {
        let _ = app_for_level.emit("audio-level", AudioLevelPayload { level });
    });

    if let Err(e) = state.capture.start(config.input_device.as_deref(), on_level) {
        hide_pill(app);
        emit_app_state(app, AppUiState::Idle);
        emit_error(app, format!("Could not start microphone: {e}"));
        return;
    }

    state.recording.store(true, Ordering::SeqCst);
}

pub fn on_hotkey_up(app: &AppHandle) {
    let state = app.state::<AppState>();
    if !state.recording.load(Ordering::SeqCst) {
        return;
    }
    state.recording.store(false, Ordering::SeqCst);
    if state.processing.swap(true, Ordering::SeqCst) {
        return;
    }

    let app = app.clone();
    if let Err(e) = thread::Builder::new()
        .name("dictate-pipeline".into())
        .spawn(move || {
            run_pipeline(&app);
            let state = app.state::<AppState>();
            state.processing.store(false, Ordering::SeqCst);
        })
    {
        log::error!("failed to spawn pipeline thread: {e}");
        state.processing.store(false, Ordering::SeqCst);
    }
}

fn run_pipeline(app: &AppHandle) {
    let state = app.state::<AppState>();
    let config = state.config.lock().clone();
    let (samples, sample_rate) = state.capture.stop();

    let pcm = audio::resample_to_16k(&samples, sample_rate);
    let pcm = audio::trim_silence(&pcm);
    if pcm.len() < MIN_SAMPLES_16K {
        emit_error(app, "Didn't catch that");
        hide_pill(app);
        emit_app_state(app, AppUiState::Idle);
        return;
    }

    let _ = app.emit("processing", ());
    emit_app_state(app, AppUiState::Processing);
    show_pill(app);

    let whisper_model = config.whisper_model;
    {
        let mut slot = state.whisper.lock();
        let reload = slot.as_ref().map(|e| e.model != whisper_model).unwrap_or(true);
        if reload {
            match WhisperEngine::load(whisper_model) {
                Ok(eng) => *slot = Some(eng),
                Err(e) => {
                    emit_error(app, format!("Failed to load Whisper: {e}"));
                    hide_pill(app);
                    emit_app_state(app, AppUiState::Idle);
                    return;
                }
            }
        }
    }

    let raw = {
        let slot = state.whisper.lock();
        match slot.as_ref() {
            Some(eng) => match eng.transcribe(&pcm) {
                Ok(t) => t,
                Err(e) => {
                    emit_error(app, format!("Transcription failed: {e}"));
                    hide_pill(app);
                    emit_app_state(app, AppUiState::Idle);
                    return;
                }
            },
            None => {
                emit_error(app, "Whisper engine is not loaded");
                hide_pill(app);
                emit_app_state(app, AppUiState::Idle);
                return;
            }
        }
    };

    let text = match config.mode {
        AppMode::Fast => cleanup::cleanup_fast(&raw),
        AppMode::Polish => {
            let llm_kind = config.llm_model;
            {
                let mut slot = state.llm.lock();
                let reload = slot.as_ref().map(|e| e.kind != llm_kind).unwrap_or(true);
                if reload {
                    match LlmEngine::load(llm_kind) {
                        Ok(eng) => *slot = Some(eng),
                        Err(e) => {
                            emit_error(app, format!("Failed to load polish model: {e}"));
                        }
                    }
                }
            }
            let polished = {
                let slot = state.llm.lock();
                slot.as_ref().and_then(|eng| match eng.polish(&raw) {
                    Ok(t) => Some(t),
                    Err(e) => {
                        emit_error(app, format!("Polish failed, using fast cleanup: {e}"));
                        None
                    }
                })
            };
            polished.unwrap_or_else(|| cleanup::cleanup_fast(&raw))
        }
    };

    *state.last_text.lock() = text.clone();
    if let Err(e) = inject::insert_text(app, &text, config.insert_mode) {
        emit_error(app, format!("Could not insert text: {e}"));
    }

    let _ = app.emit("done", DonePayload { text });
    emit_app_state(app, AppUiState::Idle);
    // Long enough for the pill to flash its confirmation and play its exit.
    let app_hide = app.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(420));
        hide_pill(&app_hide);
    });
}

pub fn show_pill(app: &AppHandle) {
    let app = app.clone();
    let app_main = app.clone();
    let _ = app.run_on_main_thread(move || show_pill_inner(&app_main));
}

fn show_pill_inner(app: &AppHandle) {
    let Some(win) = app.get_webview_window("pill") else {
        return;
    };
    let monitor = win
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| win.primary_monitor().ok().flatten());
    if let Some(monitor) = monitor {
        let scale = monitor.scale_factor();
        let mpos = monitor.position();
        let msize = monitor.size();
        let pill = win
            .outer_size()
            .unwrap_or(tauri::PhysicalSize::new(
                (216.0 * scale) as u32,
                (56.0 * scale) as u32,
            ));
        let x = mpos.x + (msize.width as i32 - pill.width as i32) / 2;
        let y = mpos.y + msize.height as i32 - pill.height as i32 - (36.0 * scale) as i32;
        let _ = win.set_position(PhysicalPosition::new(x, y));
    }
    crate::focus::show_overlay(&win);
}

pub fn hide_pill(app: &AppHandle) {
    let app = app.clone();
    let app_main = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(win) = app_main.get_webview_window("pill") {
            let _ = win.hide();
        }
    });
}
