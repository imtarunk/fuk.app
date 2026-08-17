//! Push-to-talk binding.
//!
//! There are two ways to observe a held key, and which one is used depends on
//! the shape of the binding:
//!
//! * A **combo** such as `Alt+Space` goes through the global-shortcut plugin,
//!   which is `RegisterEventHotKey` on macOS. It reports press *and* release and
//!   needs no permission at all, so it works on a fresh install.
//! * A **bare key** such as `ControlRight` cannot be expressed as a Carbon
//!   hotkey, so it needs the raw key stream: an NSEvent monitor plus a
//!   listen-only HID tap on macOS, or `rdev` elsewhere.
//!
//! Everything funnels into one worker thread. Both event sources fire on the
//! main thread, and starting the microphone or showing a window there would
//! stall the run loop — which macOS punishes by silently killing the event tap.

use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::thread;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use rdev::Key;
#[cfg(not(target_os = "macos"))]
use rdev::{Event, EventType};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

use crate::AppState;

#[cfg(target_os = "macos")]
mod macos {
    pub use crate::hotkey_macos::*;
}

/// A key edge on its way to the worker thread.
pub enum Signal {
    Raw { key: Key, press: bool },
    Combo { id: u32, press: bool },
}

/// How the current binding is observed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mechanism {
    None,
    Combo,
    Raw,
}

pub enum Binding {
    Combo(Shortcut),
    Raw(Key),
}

/// Serialises arming. A rebind from Settings and the retry tick can otherwise
/// interleave their unregister/register pairs and leave nothing bound.
static ARMING: Mutex<()> = Mutex::new(());
static MECHANISM: Mutex<Mechanism> = Mutex::new(Mechanism::None);
static ACTIVE_COMBO: Mutex<Option<Shortcut>> = Mutex::new(None);
static SIGNAL_TX: Mutex<Option<SyncSender<Signal>>> = Mutex::new(None);
static HELD: AtomicBool = AtomicBool::new(false);
/// Swallows the key-up that follows a rebind, so confirming a binding in
/// Settings does not immediately start a recording. Expires on its own in case
/// that key-up never arrives — otherwise the hotkey would stay dead forever.
static SUPPRESS: Mutex<Option<(Key, Instant)>> = Mutex::new(None);
#[cfg(not(target_os = "macos"))]
static RDEV_RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Serialize)]
pub struct HotkeyCapturedPayload {
    pub hotkey: String,
}

pub fn is_listening() -> bool {
    *MECHANISM.lock() != Mechanism::None
}

/// True when the binding can only be observed through the raw key stream, which
/// on macOS is gated behind Accessibility.
pub fn needs_accessibility(hotkey: &str) -> bool {
    cfg!(target_os = "macos") && matches!(parse_binding(hotkey), Some(Binding::Raw(_)))
}

pub fn begin_capture(state: &AppState) {
    state.capturing_hotkey.store(true, Ordering::SeqCst);
}

pub fn cancel_capture(state: &AppState) {
    state.capturing_hotkey.store(false, Ordering::SeqCst);
}

pub fn parse_binding(name: &str) -> Option<Binding> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    // A lone modifier has no Carbon equivalent, so it can only be seen on the
    // raw stream. Everything else — combos and bare function keys alike — is
    // better off as a global shortcut, which needs no permission and stops the
    // key from also reaching whatever app is focused.
    if let Some(key) = lone_modifier(name) {
        return Some(Binding::Raw(key));
    }
    if let Ok(shortcut) = Shortcut::from_str(name) {
        return Some(Binding::Combo(shortcut));
    }
    parse_bare_key(name).map(Binding::Raw)
}

fn lone_modifier(name: &str) -> Option<Key> {
    Some(match name {
        "ControlLeft" => Key::ControlLeft,
        "ControlRight" => Key::ControlRight,
        "Alt" | "AltLeft" => Key::Alt,
        "AltGr" | "AltRight" => Key::AltGr,
        "ShiftLeft" => Key::ShiftLeft,
        "ShiftRight" => Key::ShiftRight,
        "MetaLeft" | "OSLeft" => Key::MetaLeft,
        "MetaRight" | "OSRight" => Key::MetaRight,
        "Function" | "Fn" => Key::Function,
        "CapsLock" => Key::CapsLock,
        _ => return None,
    })
}

/// Feeds the worker from the global-shortcut plugin. Runs on the main thread.
pub fn on_shortcut(_app: &AppHandle, shortcut: &Shortcut, event: ShortcutEvent) {
    let press = matches!(event.state, ShortcutState::Pressed);
    send(Signal::Combo {
        id: shortcut.id(),
        press,
    });
}

pub fn send(signal: Signal) {
    if let Some(tx) = SIGNAL_TX.lock().as_ref() {
        let _ = tx.try_send(signal);
    }
}

pub fn start(app: &AppHandle) {
    let (tx, rx) = mpsc::sync_channel::<Signal>(256);
    *SIGNAL_TX.lock() = Some(tx);

    let worker_app = app.clone();
    let _ = thread::Builder::new()
        .name("dictate-hotkey-worker".into())
        .spawn(move || {
            while let Ok(signal) = rx.recv() {
                match signal {
                    Signal::Raw { key, press } => on_raw(&worker_app, key, press),
                    Signal::Combo { id, press } => on_combo(&worker_app, id, press),
                }
            }
        });

    // Arming has to happen off the main thread: registering a global shortcut
    // blocks on a round trip *to* the main thread. The loop then re-arms
    // whenever the binding is not live, which covers Accessibility being
    // granted after launch and a tap the system decided to tear down.
    let app = app.clone();
    let _ = thread::Builder::new()
        .name("dictate-hotkey-arm".into())
        .spawn(move || loop {
            arm_if_idle(&app);
            thread::sleep(Duration::from_secs(3));
        });
}

/// Re-arm immediately instead of waiting for the retry tick. Never call this
/// from the main thread.
pub fn refresh(app: &AppHandle) {
    arm_if_idle(app);
}

fn arm_if_idle(app: &AppHandle) {
    let _arming = ARMING.lock();
    crate::focus::refresh_frontmost();
    if is_listening() {
        #[cfg(target_os = "macos")]
        macos::keepalive();
        return;
    }
    let hotkey = app.state::<AppState>().config.lock().hotkey.clone();
    if let Err(e) = arm_locked(app, &hotkey) {
        log::warn!("hotkey `{hotkey}` is not active: {e}");
    }
}

/// Persists a new binding and makes it live. Never call this from the main
/// thread — see [`arm`].
pub fn apply_hotkey(app: &AppHandle, hotkey: String) -> Result<String, String> {
    let binding =
        parse_binding(&hotkey).ok_or_else(|| format!("`{hotkey}` cannot be used as a hotkey"))?;

    let state = app.state::<AppState>();
    state.capturing_hotkey.store(false, Ordering::SeqCst);
    if let Binding::Raw(key) = binding {
        *SUPPRESS.lock() = Some((key, Instant::now() + Duration::from_secs(2)));
    }

    let armed = arm(app, &hotkey);

    {
        let mut cfg = state.config.lock();
        cfg.hotkey = hotkey.clone();
        crate::config::save(&cfg).map_err(|e| e.to_string())?;
        let _ = app.emit("config-updated", cfg.clone());
    }
    let _ = app.emit(
        "hotkey-captured",
        HotkeyCapturedPayload {
            hotkey: hotkey.clone(),
        },
    );

    armed?;
    Ok(hotkey)
}

/// Point the active mechanism at `hotkey`.
///
/// Must not run on the main thread: `GlobalShortcut::register` dispatches to the
/// main thread and blocks for the answer, so calling it from there deadlocks.
fn arm(app: &AppHandle, hotkey: &str) -> Result<(), String> {
    let _arming = ARMING.lock();
    arm_locked(app, hotkey)
}

fn arm_locked(app: &AppHandle, hotkey: &str) -> Result<(), String> {
    let binding =
        parse_binding(hotkey).ok_or_else(|| format!("`{hotkey}` cannot be used as a hotkey"))?;
    disarm(app);

    match binding {
        Binding::Combo(shortcut) => {
            app.global_shortcut()
                .register(shortcut)
                .map_err(|e| format!("another app already owns `{hotkey}` ({e})"))?;
            *ACTIVE_COMBO.lock() = Some(shortcut);
            *MECHANISM.lock() = Mechanism::Combo;
            log::info!("hotkey `{hotkey}` armed as a global shortcut");
            Ok(())
        }
        Binding::Raw(_) => {
            if !start_raw(app) {
                return Err(
                    "a single-key hotkey needs the raw key stream, which is not available"
                        .to_string(),
                );
            }
            *MECHANISM.lock() = Mechanism::Raw;
            log::info!("hotkey `{hotkey}` armed on the raw key stream");
            Ok(())
        }
    }
}

fn disarm(app: &AppHandle) {
    HELD.store(false, Ordering::SeqCst);
    *MECHANISM.lock() = Mechanism::None;
    let previous = ACTIVE_COMBO.lock().take();
    if let Some(shortcut) = previous {
        let _ = app.global_shortcut().unregister(shortcut);
    }
}

#[cfg(target_os = "macos")]
fn start_raw(app: &AppHandle) -> bool {
    if macos::is_installed() {
        return true;
    }
    // Do not pre-check AXIsProcessTrusted. That API stays false after a rebuild
    // even when System Settings still shows Fuk as enabled. Installing the
    // HID tap / NSEvent monitor is the real test.
    let (done_tx, done_rx) = mpsc::channel();
    if app
        .run_on_main_thread(move || {
            let installed = match macos::install() {
                Ok(()) => true,
                Err(e) => {
                    log::warn!("keyboard tap: {e}");
                    false
                }
            };
            let _ = done_tx.send(installed);
        })
        .is_err()
    {
        return false;
    }
    done_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap_or(false)
}

#[cfg(not(target_os = "macos"))]
fn start_raw(_app: &AppHandle) -> bool {
    if RDEV_RUNNING.load(Ordering::SeqCst) {
        return true;
    }
    let _ = thread::Builder::new()
        .name("dictate-hotkey-rdev".into())
        .spawn(move || {
            RDEV_RUNNING.store(true, Ordering::SeqCst);
            let callback = move |event: Event| {
                let (key, press) = match event.event_type {
                    EventType::KeyPress(k) => (k, true),
                    EventType::KeyRelease(k) => (k, false),
                    _ => return,
                };
                send(Signal::Raw { key, press });
            };
            if let Err(e) = rdev::listen(callback) {
                log::error!("hotkey listener failed: {e:?}");
            }
            RDEV_RUNNING.store(false, Ordering::SeqCst);
            *MECHANISM.lock() = Mechanism::None;
        });
    thread::sleep(Duration::from_millis(100));
    RDEV_RUNNING.load(Ordering::SeqCst)
}

fn on_combo(app: &AppHandle, id: u32, press: bool) {
    if *MECHANISM.lock() != Mechanism::Combo {
        return;
    }
    if ACTIVE_COMBO.lock().as_ref().map(Shortcut::id) != Some(id) {
        return;
    }
    edge(app, press);
}

fn on_raw(app: &AppHandle, key: Key, press: bool) {
    if *MECHANISM.lock() != Mechanism::Raw {
        return;
    }
    {
        let mut suppress = SUPPRESS.lock();
        if let Some((suppressed, deadline)) = *suppress {
            if Instant::now() >= deadline {
                *suppress = None;
            } else if suppressed == key {
                if !press {
                    *suppress = None;
                }
                return;
            }
        }
    }

    let hotkey = app.state::<AppState>().config.lock().hotkey.clone();
    let Some(Binding::Raw(target)) = parse_binding(&hotkey) else {
        return;
    };
    if key != target {
        return;
    }
    edge(app, press);
}

fn edge(app: &AppHandle, press: bool) {
    if app
        .state::<AppState>()
        .capturing_hotkey
        .load(Ordering::SeqCst)
    {
        return;
    }
    if press {
        if HELD.swap(true, Ordering::SeqCst) {
            return;
        }
        crate::pipeline::on_hotkey_down(app);
    } else if HELD.swap(false, Ordering::SeqCst) {
        crate::pipeline::on_hotkey_up(app);
    }
}

fn parse_bare_key(name: &str) -> Option<Key> {
    Some(match name {
        "ControlRight" => Key::ControlRight,
        "ControlLeft" => Key::ControlLeft,
        "Alt" | "AltLeft" => Key::Alt,
        "AltGr" | "AltRight" => Key::AltGr,
        "ShiftLeft" => Key::ShiftLeft,
        "ShiftRight" => Key::ShiftRight,
        "MetaLeft" | "OSLeft" => Key::MetaLeft,
        "MetaRight" | "OSRight" => Key::MetaRight,
        "Space" => Key::Space,
        "Tab" => Key::Tab,
        "CapsLock" => Key::CapsLock,
        "Escape" => Key::Escape,
        "Function" | "Fn" => Key::Function,
        "F1" => Key::F1,
        "F2" => Key::F2,
        "F3" => Key::F3,
        "F4" => Key::F4,
        "F5" => Key::F5,
        "F6" => Key::F6,
        "F7" => Key::F7,
        "F8" => Key::F8,
        "F9" => Key::F9,
        "F10" => Key::F10,
        "F11" => Key::F11,
        "F12" => Key::F12,
        "Return" | "Enter" => Key::Return,
        "Backspace" => Key::Backspace,
        "Period" | "Dot" => Key::Dot,
        "Comma" => Key::Comma,
        "Slash" => Key::Slash,
        "Semicolon" | "SemiColon" => Key::SemiColon,
        "Quote" => Key::Quote,
        "Backslash" | "BackSlash" => Key::BackSlash,
        "Backquote" | "BackQuote" => Key::BackQuote,
        "Minus" => Key::Minus,
        "Equal" => Key::Equal,
        "BracketLeft" | "LeftBracket" => Key::LeftBracket,
        "BracketRight" | "RightBracket" => Key::RightBracket,
        other => return parse_debug_key(other),
    })
}

fn parse_debug_key(name: &str) -> Option<Key> {
    for candidate in ALL_NAMED_KEYS {
        if format!("{candidate:?}") == name {
            return Some(*candidate);
        }
    }
    if let Some(rest) = name
        .strip_prefix("Unknown(")
        .and_then(|s| s.strip_suffix(')'))
    {
        if let Ok(code) = rest.parse::<u32>() {
            return Some(Key::Unknown(code));
        }
    }
    None
}

pub fn key_from_code(code: u16) -> Key {
    match code {
        0 => Key::KeyA,
        1 => Key::KeyS,
        2 => Key::KeyD,
        3 => Key::KeyF,
        4 => Key::KeyH,
        5 => Key::KeyG,
        6 => Key::KeyZ,
        7 => Key::KeyX,
        8 => Key::KeyC,
        9 => Key::KeyV,
        11 => Key::KeyB,
        12 => Key::KeyQ,
        13 => Key::KeyW,
        14 => Key::KeyE,
        15 => Key::KeyR,
        16 => Key::KeyY,
        17 => Key::KeyT,
        18 => Key::Num1,
        19 => Key::Num2,
        20 => Key::Num3,
        21 => Key::Num4,
        22 => Key::Num6,
        23 => Key::Num5,
        24 => Key::Equal,
        25 => Key::Num9,
        26 => Key::Num7,
        27 => Key::Minus,
        28 => Key::Num8,
        29 => Key::Num0,
        30 => Key::RightBracket,
        31 => Key::KeyO,
        32 => Key::KeyU,
        33 => Key::LeftBracket,
        34 => Key::KeyI,
        35 => Key::KeyP,
        36 => Key::Return,
        37 => Key::KeyL,
        38 => Key::KeyJ,
        39 => Key::Quote,
        40 => Key::KeyK,
        41 => Key::SemiColon,
        42 => Key::BackSlash,
        43 => Key::Comma,
        44 => Key::Slash,
        45 => Key::KeyN,
        46 => Key::KeyM,
        47 => Key::Dot,
        48 => Key::Tab,
        49 => Key::Space,
        50 => Key::BackQuote,
        51 => Key::Backspace,
        53 => Key::Escape,
        54 => Key::MetaRight,
        55 => Key::MetaLeft,
        56 => Key::ShiftLeft,
        57 => Key::CapsLock,
        58 => Key::Alt,
        59 => Key::ControlLeft,
        60 => Key::ShiftRight,
        61 => Key::AltGr,
        62 => Key::ControlRight,
        63 => Key::Function,
        96 => Key::F5,
        97 => Key::F6,
        98 => Key::F7,
        99 => Key::F3,
        100 => Key::F8,
        101 => Key::F9,
        103 => Key::F11,
        109 => Key::F10,
        111 => Key::F12,
        118 => Key::F4,
        120 => Key::F2,
        122 => Key::F1,
        123 => Key::LeftArrow,
        124 => Key::RightArrow,
        125 => Key::DownArrow,
        126 => Key::UpArrow,
        other => Key::Unknown(u32::from(other)),
    }
}

const ALL_NAMED_KEYS: &[Key] = &[
    Key::Alt,
    Key::AltGr,
    Key::Backspace,
    Key::CapsLock,
    Key::ControlLeft,
    Key::ControlRight,
    Key::Delete,
    Key::DownArrow,
    Key::End,
    Key::Escape,
    Key::F1,
    Key::F2,
    Key::F3,
    Key::F4,
    Key::F5,
    Key::F6,
    Key::F7,
    Key::F8,
    Key::F9,
    Key::F10,
    Key::F11,
    Key::F12,
    Key::Home,
    Key::LeftArrow,
    Key::MetaLeft,
    Key::MetaRight,
    Key::PageDown,
    Key::PageUp,
    Key::Return,
    Key::RightArrow,
    Key::ShiftLeft,
    Key::ShiftRight,
    Key::Space,
    Key::Tab,
    Key::UpArrow,
    Key::PrintScreen,
    Key::ScrollLock,
    Key::Pause,
    Key::NumLock,
    Key::Function,
    Key::KeyA,
    Key::KeyB,
    Key::KeyC,
    Key::KeyD,
    Key::KeyE,
    Key::KeyF,
    Key::KeyG,
    Key::KeyH,
    Key::KeyI,
    Key::KeyJ,
    Key::KeyK,
    Key::KeyL,
    Key::KeyM,
    Key::KeyN,
    Key::KeyO,
    Key::KeyP,
    Key::KeyQ,
    Key::KeyR,
    Key::KeyS,
    Key::KeyT,
    Key::KeyU,
    Key::KeyV,
    Key::KeyW,
    Key::KeyX,
    Key::KeyY,
    Key::KeyZ,
    Key::Insert,
    Key::Comma,
    Key::Dot,
    Key::Slash,
    Key::SemiColon,
    Key::Quote,
    Key::BackSlash,
    Key::IntlBackslash,
    Key::BackQuote,
    Key::Minus,
    Key::Equal,
    Key::LeftBracket,
    Key::RightBracket,
    Key::Num0,
    Key::Num1,
    Key::Num2,
    Key::Num3,
    Key::Num4,
    Key::Num5,
    Key::Num6,
    Key::Num7,
    Key::Num8,
    Key::Num9,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combos_take_the_permission_free_path() {
        assert!(matches!(
            parse_binding("Alt+Space"),
            Some(Binding::Combo(_))
        ));
        assert!(matches!(
            parse_binding("Control+Shift+KeyD"),
            Some(Binding::Combo(_))
        ));
        assert!(!needs_accessibility("Alt+Space"));
    }

    #[test]
    fn lone_modifiers_take_the_raw_path() {
        assert!(matches!(
            parse_binding("ControlRight"),
            Some(Binding::Raw(Key::ControlRight))
        ));
        assert!(matches!(
            parse_binding("Function"),
            Some(Binding::Raw(Key::Function))
        ));
        assert_eq!(
            needs_accessibility("ControlRight"),
            cfg!(target_os = "macos")
        );
    }

    #[test]
    fn a_bare_function_key_still_avoids_accessibility() {
        assert!(matches!(parse_binding("F8"), Some(Binding::Combo(_))));
        assert!(!needs_accessibility("F8"));
    }

    #[test]
    fn nonsense_is_rejected() {
        assert!(parse_binding("").is_none());
        assert!(parse_binding("Ctrl+NotAKey").is_none());
        assert!(parse_binding("NotAKey").is_none());
        assert!(!needs_accessibility("NotAKey"));
    }
}
