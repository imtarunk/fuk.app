mod audio;
#[cfg(target_os = "macos")]
mod ax;
mod cleanup;
mod config;
mod download;
mod focus;
mod hardware;
mod hotkey;
#[cfg(target_os = "macos")]
mod hotkey_macos;
mod inject;
mod llm;
mod overlay;
mod permissions;
mod pipeline;
mod stt;
mod tray;

use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;
use tauri::{Emitter, Manager, WindowEvent};

use config::{AppConfig, AppUiState};
use llm::LlmEngine;
use stt::WhisperEngine;

pub struct AppState {
    pub config: Mutex<AppConfig>,
    pub capture: audio::CaptureController,
    pub whisper: Mutex<Option<WhisperEngine>>,
    pub llm: Mutex<Option<LlmEngine>>,
    pub capturing_hotkey: AtomicBool,
    pub recording: AtomicBool,
    pub processing: AtomicBool,
    pub downloading: AtomicBool,
    pub last_text: Mutex<String>,
}

impl AppState {
    fn new(config: AppConfig) -> Self {
        Self {
            config: Mutex::new(config),
            capture: audio::CaptureController::new(),
            whisper: Mutex::new(None),
            llm: Mutex::new(None),
            capturing_hotkey: AtomicBool::new(false),
            recording: AtomicBool::new(false),
            processing: AtomicBool::new(false),
            downloading: AtomicBool::new(false),
            last_text: Mutex::new(String::new()),
        }
    }
}

#[tauri::command]
fn get_config(state: tauri::State<AppState>) -> Result<AppConfig, String> {
    Ok(state.config.lock().clone())
}

#[tauri::command]
fn save_config(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    mut config: AppConfig,
) -> Result<AppConfig, String> {
    {
        let prev = state.config.lock();
        if config.overlay_x.is_none() {
            config.overlay_x = prev.overlay_x;
        }
        if config.overlay_y.is_none() {
            config.overlay_y = prev.overlay_y;
        }
    }
    config::apply_hardware_llm(&mut config);
    config::save(&config).map_err(|e| e.to_string())?;
    let (whisper_changed, llm_changed) = {
        let slot = state.config.lock();
        (
            slot.whisper_model != config.whisper_model,
            slot.llm_model != config.llm_model,
        )
    };
    if whisper_changed {
        *state.whisper.lock() = None;
    }
    if llm_changed {
        *state.llm.lock() = None;
    }
    *state.config.lock() = config.clone();
    let _ = app.emit("config-updated", config.clone());
    if download::pending_downloads(&config) {
        kickoff_model_download(&app);
    }
    Ok(config)
}

#[tauri::command]
fn list_input_devices() -> Result<Vec<audio::AudioDevice>, String> {
    audio::list_input_devices().map_err(|e| e.to_string())
}

#[tauri::command]
fn get_model_status(state: tauri::State<AppState>) -> Result<download::ModelStatus, String> {
    let config = state.config.lock().clone();
    Ok(download::get_model_status(&config))
}

#[tauri::command]
async fn start_model_download(app: tauri::AppHandle) -> Result<(), String> {
    run_model_download(app).await
}

pub(crate) fn kickoff_model_download(app: &tauri::AppHandle) {
    let cfg = app.state::<AppState>().config.lock().clone();
    if !download::pending_downloads(&cfg) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = run_model_download(app).await {
            log::warn!("background model download: {e}");
        }
    });
}

async fn run_model_download(app: tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    if state.downloading.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let config = state.config.lock().clone();
    drop(state);

    pipeline::emit_app_state(&app, AppUiState::Downloading);
    let result = download::start_model_download(&app, &config).await;

    let state = app.state::<AppState>();
    state.downloading.store(false, Ordering::SeqCst);

    match result {
        Ok(()) => {
            {
                let mut cfg = state.config.lock();
                cfg.first_run_complete = true;
                if let Err(e) = config::save(&cfg) {
                    log::warn!("save first_run_complete: {e}");
                }
                let _ = app.emit("config-updated", cfg.clone());
            }
            pipeline::emit_app_state(&app, AppUiState::Idle);
            preload_whisper(&app);
            preload_llm(&app);
            Ok(())
        }
        Err(e) => {
            pipeline::emit_app_state(&app, AppUiState::Idle);
            pipeline::emit_error(&app, format!("Download failed: {e}"));
            Err(e.to_string())
        }
    }
}

// Arming a hotkey blocks on the main thread, so anything that can re-arm has to
// be an async command; sync commands already run there.
#[tauri::command]
async fn check_permissions(
    app: tauri::AppHandle,
) -> Result<permissions::PermissionsStatus, String> {
    blocking(move || {
        hotkey::refresh(&app);
        permissions::check(&app)
    })
    .await
}

#[tauri::command]
async fn request_permissions(
    app: tauri::AppHandle,
) -> Result<permissions::PermissionsStatus, String> {
    blocking(move || permissions::request_microphone(&app)).await
}

#[tauri::command]
async fn request_accessibility(
    app: tauri::AppHandle,
) -> Result<permissions::PermissionsStatus, String> {
    blocking(move || permissions::request_accessibility(&app)).await
}

async fn blocking<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn open_accessibility_settings(app: tauri::AppHandle) -> Result<(), String> {
    permissions::open_accessibility_settings(&app)
}

#[tauri::command]
fn begin_hotkey_capture(state: tauri::State<AppState>) -> Result<(), String> {
    hotkey::begin_capture(&*state);
    Ok(())
}

#[tauri::command]
fn cancel_hotkey_capture(state: tauri::State<AppState>) -> Result<(), String> {
    hotkey::cancel_capture(&*state);
    Ok(())
}

#[tauri::command]
async fn set_hotkey(app: tauri::AppHandle, hotkey: String) -> Result<String, String> {
    blocking(move || hotkey::apply_hotkey(&app, hotkey)).await?
}

#[tauri::command]
fn retry_last_insert(app: tauri::AppHandle, state: tauri::State<AppState>) -> Result<(), String> {
    let text = state.last_text.lock().clone();
    if text.is_empty() {
        return Err("Nothing to insert".into());
    }
    let mode = state.config.lock().insert_mode;
    inject::retry_insert(&app, &text, mode).map_err(|e| e.to_string())?;
    let _ = app.emit("done", pipeline::DonePayload { text });
    Ok(())
}

fn preload_llm(app: &tauri::AppHandle) {
    let app = app.clone();
    std::thread::Builder::new()
        .name("dictate-llm-preload".into())
        .spawn(move || {
            let state = app.state::<AppState>();
            let kind = state.config.lock().llm_model;
            if !kind.path().is_file() {
                return;
            }
            let already = state
                .llm
                .lock()
                .as_ref()
                .map(|e| e.kind == kind)
                .unwrap_or(false);
            if already {
                return;
            }
            match LlmEngine::load(kind) {
                Ok(engine) => {
                    *state.llm.lock() = Some(engine);
                    log::info!("polish model {} loaded", kind.as_id());
                }
                Err(e) => log::warn!("polish preload failed: {e}"),
            }
        })
        .ok();
}

fn preload_whisper(app: &tauri::AppHandle) {
    let app = app.clone();
    std::thread::Builder::new()
        .name("dictate-whisper-preload".into())
        .spawn(move || {
            let state = app.state::<AppState>();
            let model = state.config.lock().whisper_model;
            if !model.path().is_file() {
                return;
            }
            let already = state
                .whisper
                .lock()
                .as_ref()
                .map(|e| e.model == model)
                .unwrap_or(false);
            if already {
                return;
            }
            match WhisperEngine::load(model) {
                Ok(engine) => {
                    // The first inference pays for compiling the Metal
                    // pipelines. Spend it now rather than on the user's first
                    // sentence.
                    engine.warm_up();
                    *state.whisper.lock() = Some(engine);
                    log::info!("whisper model {} loaded and warm", model.as_id());
                }
                Err(e) => log::warn!("whisper preload failed: {e}"),
            }
        })
        .ok();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("dictate_lib=info,dictate=info"),
    )
    .try_init();
    let config = config::load();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(hotkey::on_shortcut)
                .build(),
        )
        .manage(AppState::new(config))
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            list_input_devices,
            get_model_status,
            start_model_download,
            check_permissions,
            request_permissions,
            request_accessibility,
            open_accessibility_settings,
            begin_hotkey_capture,
            cancel_hotkey_capture,
            set_hotkey,
            retry_last_insert,
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            {
                let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }

            focus::init();

            if let Some(settings) = app.get_webview_window("settings") {
                let _ = settings.hide();
            }
            overlay::show_idle(app.handle());

            if let Err(e) = tray::install(app.handle()) {
                log::error!("tray setup failed: {e}");
            }

            hotkey::start(app.handle());

            let cfg = app.state::<AppState>().config.lock().clone();
            if !cfg.first_run_complete {
                tray::show_settings(app.handle());
            }
            kickoff_model_download(app.handle());
            if !download::whisper_missing(&cfg) {
                preload_whisper(app.handle());
            }
            if cfg.llm_model.path().is_file() {
                preload_llm(app.handle());
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            match event {
                WindowEvent::CloseRequested { api, .. } if window.label() == "settings" => {
                    api.prevent_close();
                    let _ = window.hide();
                }
                WindowEvent::Moved(pos) if window.label() == "pill" => {
                    overlay::on_moved(window.app_handle(), *pos);
                }
                _ => {}
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
