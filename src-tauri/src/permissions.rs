use std::sync::atomic::{AtomicU8, Ordering};

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::audio;
use crate::config;
use crate::AppState;

const UNKNOWN: u8 = 0;
const DENIED: u8 = 1;
const GRANTED: u8 = 2;

/// Enumerating input devices takes long enough to be felt at the start of a
/// recording, so a granted answer is remembered. A denied answer is not: the
/// user may grant Microphone in System Settings and press the hotkey without
/// coming back through Settings first.
static MICROPHONE: AtomicU8 = AtomicU8::new(UNKNOWN);

#[derive(Debug, Clone, Serialize)]
pub struct PermissionsStatus {
    pub microphone: bool,
    pub accessibility: bool,
    pub input_monitoring: bool,
    pub wayland: bool,
    pub hotkey_listening: bool,
    pub hotkey_needs_accessibility: bool,
}

pub fn check(app: &AppHandle) -> PermissionsStatus {
    status(app, audio::microphone_available(), accessibility_trusted(false))
}

/// Probe the microphone only. Must not prompt for Accessibility — that dialog
/// is what made Settings feel like it was asking again after the user had
/// already flipped the toggle.
pub fn request_microphone(app: &AppHandle) -> PermissionsStatus {
    status(app, audio::probe_microphone(), accessibility_trusted(false))
}

/// Re-arm the hotkey. Never call `AXIsProcessTrustedWithOptions(prompt)` or
/// `CGRequestListenEventAccess` from here — both pop a system dialog even when
/// the toggle in System Settings is already on. Recheck only reads status;
/// "Allow access" opens System Settings instead.
pub fn request_accessibility(app: &AppHandle) -> PermissionsStatus {
    crate::hotkey::refresh(app);
    status(
        app,
        audio::microphone_available(),
        accessibility_trusted(false),
    )
}

fn status(app: &AppHandle, microphone: bool, accessibility: bool) -> PermissionsStatus {
    MICROPHONE.store(if microphone { GRANTED } else { DENIED }, Ordering::Relaxed);
    let hotkey = app.state::<AppState>().config.lock().hotkey.clone();
    PermissionsStatus {
        microphone,
        accessibility,
        input_monitoring: listen_event_access(),
        wayland: config::is_wayland(),
        hotkey_listening: crate::hotkey::is_listening(),
        hotkey_needs_accessibility: crate::hotkey::needs_accessibility(&hotkey),
    }
}

pub fn microphone_cached() -> bool {
    if MICROPHONE.load(Ordering::Relaxed) == GRANTED {
        return true;
    }
    let available = audio::microphone_available();
    MICROPHONE.store(
        if available { GRANTED } else { DENIED },
        Ordering::Relaxed,
    );
    available
}

pub(crate) fn accessibility_trusted(prompt: bool) -> bool {
    let _ = prompt;
    #[cfg(target_os = "macos")]
    {
        crate::ax::api_available()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

fn listen_event_access() -> bool {
    #[cfg(target_os = "macos")]
    {
        unsafe { CGPreflightListenEventAccess() }
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

pub fn open_accessibility_settings(app: &AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use tauri_plugin_opener::OpenerExt;
        let opener = app.opener();
        let acc = "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
        let listen = "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent";
        opener
            .open_url(acc, None::<&str>)
            .map_err(|e| e.to_string())?;
        let _ = opener.open_url(listen, None::<&str>);
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Ok(())
    }
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightListenEventAccess() -> bool;
}
