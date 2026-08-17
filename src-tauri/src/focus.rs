//! Remember which app the user was in when they started dictating, then put
//! that app back in front before we paste.
//!
//! Showing the pill used `makeKeyAndOrderFront`, which made Dictate the focused
//! app. Cmd+V then landed in our own webview (or nowhere) and the user had to
//! click back. The pill is now a non-activating overlay; this module is the
//! backup: snapshot the target on hotkey-down and restore it before insert.

use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Manager};

#[cfg(target_os = "macos")]
use parking_lot::Mutex;

#[cfg(target_os = "macos")]
use crate::ax::{self, AxElem};

static OUR_PID: OnceLock<i32> = OnceLock::new();
static LAST_FOREIGN_PID: AtomicI32 = AtomicI32::new(0);
static TARGET_PID: AtomicI32 = AtomicI32::new(0);
#[cfg(target_os = "macos")]
static FOCUSED: Mutex<Option<AxElem>> = Mutex::new(None);

pub fn init() {
    let _ = OUR_PID.set(std::process::id() as i32);
    refresh_frontmost();
}

fn our_pid() -> i32 {
    *OUR_PID.get_or_init(|| std::process::id() as i32)
}

fn is_self(pid: i32) -> bool {
    pid <= 0 || pid == our_pid()
}

/// Every key event from the global tap names the process it was heading for.
/// Keep the last non-Dictate one so we still know the target if the pill or
/// Settings briefly become frontmost.
pub fn note_event_pid(pid: i32) {
    if !is_self(pid) {
        LAST_FOREIGN_PID.store(pid, Ordering::SeqCst);
    }
}

pub fn refresh_frontmost() {
    #[cfg(target_os = "macos")]
    macos::note_frontmost();
}

/// Call on hotkey-down, before the pill appears.
pub fn capture_target() {
    refresh_frontmost();
    let front = macos_frontmost_pid();
    let pid = if is_self(front) {
        LAST_FOREIGN_PID.load(Ordering::SeqCst)
    } else {
        front
    };
    if !is_self(pid) {
        LAST_FOREIGN_PID.store(pid, Ordering::SeqCst);
    }
    TARGET_PID.store(pid, Ordering::SeqCst);
    #[cfg(target_os = "macos")]
    {
        let field = if is_self(pid) {
            None
        } else {
            ax::focused_element(pid)
        };
        let had_field = field.is_some();
        *FOCUSED.lock() = field;
        if had_field {
            log::info!("insert target pid {pid} (focused field captured)");
        } else if !is_self(pid) {
            log::info!("insert target pid {pid} (no focused field yet)");
        }
    }
    #[cfg(not(target_os = "macos"))]
    if !is_self(pid) {
        log::info!("insert target pid {pid}");
    }
}

pub fn captured_pid() -> Option<i32> {
    let pid = TARGET_PID.load(Ordering::SeqCst);
    if is_self(pid) {
        None
    } else {
        Some(pid)
    }
}

#[cfg(target_os = "macos")]
pub fn captured_field() -> Option<AxElem> {
    FOCUSED.lock().clone()
}

/// Hide Settings and bring the snapshotted app forward so its caret is live.
pub fn activate_captured(app: &AppHandle) {
    let pid = captured_pid();
    let app_main = app.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = app.run_on_main_thread(move || {
        if let Some(win) = app_main.get_webview_window("settings") {
            let _ = win.hide();
        }
        #[cfg(target_os = "macos")]
        if let Some(pid) = pid {
            macos::activate_pid(pid);
        }
        let _ = tx.send(());
    });
    let _ = rx.recv_timeout(Duration::from_millis(400));
    thread::sleep(Duration::from_millis(80));
}

fn macos_frontmost_pid() -> i32 {
    #[cfg(target_os = "macos")]
    {
        macos::frontmost_pid()
    }
    #[cfg(not(target_os = "macos"))]
    {
        0
    }
}

/// Show the overlay without making Dictate the key app.
pub fn show_overlay(win: &tauri::WebviewWindow) {
    let _ = win.set_focusable(false);
    let _ = win.set_ignore_cursor_events(true);
    #[cfg(target_os = "macos")]
    macos::order_front_overlay(win);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = win.show();
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationOptions, NSRunningApplication, NSStatusWindowLevel,
        NSWindow, NSWindowCollectionBehavior, NSWorkspace,
    };
    use super::{note_event_pid, our_pid};

    pub fn frontmost_pid() -> i32 {
        let Some(app) = NSWorkspace::sharedWorkspace().frontmostApplication() else {
            return 0;
        };
        app.processIdentifier() as i32
    }

    pub fn note_frontmost() {
        note_event_pid(frontmost_pid());
    }

    pub fn activate_pid(pid: i32) {
        if pid == our_pid() {
            return;
        }
        let Some(target) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
        else {
            log::warn!("insert target pid {pid} is no longer running");
            return;
        };
        if target.isTerminated() {
            return;
        }
        let Some(mtm) = MainThreadMarker::new() else {
            log::warn!("activate_pid must run on the main thread");
            return;
        };
        let us = NSApplication::sharedApplication(mtm);
        us.yieldActivationToApplication(&target);
        let _ = target.unhide();
        let _ = target.activateFromApplication_options(
            &NSRunningApplication::currentApplication(),
            NSApplicationActivationOptions::ActivateAllWindows,
        );
        let _ = target.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);
    }

    pub fn order_front_overlay(win: &tauri::WebviewWindow) {
        let Ok(ptr) = win.ns_window() else {
            let _ = win.show();
            return;
        };
        let ptr = ptr as *mut NSWindow;
        let Some(window) = (unsafe { Retained::retain(ptr) }) else {
            let _ = win.show();
            return;
        };
        window.setHidesOnDeactivate(false);
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::Transient
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        window.setLevel(NSStatusWindowLevel);
        window.orderFrontRegardless();
    }
}
