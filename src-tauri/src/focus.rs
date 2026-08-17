//! Remember which app and text caret the user was in, and keep that snapshot
//! fresh while they dictate so insert always hits the latest cursor.
//!
//! Showing the pill used `makeKeyAndOrderFront`, which made Fuk the focused
//! app. Cmd+V then landed in our own webview (or nowhere) and the user had to
//! click back. The pill is now a non-activating overlay; this module snapshots
//! the target on hotkey-down, follows caret moves during recording, and
//! restores the latest app only if Accessibility insert needs it.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Manager};

#[cfg(target_os = "macos")]
use parking_lot::Mutex;

#[cfg(target_os = "macos")]
use crate::ax::{self, Caret};

static OUR_PID: OnceLock<i32> = OnceLock::new();
static LAST_FOREIGN_PID: AtomicI32 = AtomicI32::new(0);
static TARGET_PID: AtomicI32 = AtomicI32::new(0);
static WATCHING: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "macos")]
static CARET: Mutex<Option<Caret>> = Mutex::new(None);

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
/// Keep the last non-Fuk one so we still know the target if the pill or
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
    refresh_caret();
}

/// Follow the frontmost app's focused field and caret until insert.
pub fn start_caret_watch() {
    refresh_caret();
    if WATCHING.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = thread::Builder::new()
        .name("fuk-caret".into())
        .spawn(|| {
            while WATCHING.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(80));
                if WATCHING.load(Ordering::SeqCst) {
                    refresh_caret();
                }
            }
        });
}

pub fn stop_caret_watch() {
    WATCHING.store(false, Ordering::SeqCst);
}

/// Re-read the live insertion point. The system-wide Accessibility element is
/// asked first — it names the focused field in *any* application — so a click
/// into another app's field during dictation wins.
pub fn refresh_caret() {
    refresh_frontmost();

    #[cfg(target_os = "macos")]
    if let Some(next) = ax::system_caret() {
        if !is_self(next.pid) {
            LAST_FOREIGN_PID.store(next.pid, Ordering::SeqCst);
            TARGET_PID.store(next.pid, Ordering::SeqCst);
            let mut slot = CARET.lock();
            let replace = match slot.as_ref() {
                None => true,
                Some(prev) if prev.pid != next.pid => true,
                Some(_) if ax::is_editable(&next.element) => true,
                Some(prev) => !ax::is_editable(&prev.element),
            };
            if replace {
                if slot.as_ref().is_none_or(|prev| {
                    prev.pid != next.pid || !prev.element.ptr_eq(&next.element)
                }) {
                    log::info!(
                        "caret pid {} range {}+{}",
                        next.pid,
                        next.location,
                        next.length
                    );
                }
                *slot = Some(next);
            }
            return;
        }
    }

    // System-wide element unavailable (no Accessibility, or focus is on a
    // widget it cannot see). Fall back to the frontmost app's focused field.
    let front = macos_frontmost_pid();
    let pid = if !is_self(front) {
        front
    } else {
        let stored = TARGET_PID.load(Ordering::SeqCst);
        if !is_self(stored) {
            stored
        } else {
            LAST_FOREIGN_PID.load(Ordering::SeqCst)
        }
    };
    if is_self(pid) {
        return;
    }
    LAST_FOREIGN_PID.store(pid, Ordering::SeqCst);
    TARGET_PID.store(pid, Ordering::SeqCst);

    #[cfg(target_os = "macos")]
    if let Some(next) = ax::snapshot_caret(pid) {
        *CARET.lock() = Some(next);
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
pub fn captured_caret() -> Option<Caret> {
    CARET.lock().clone()
}

/// Hide Settings without yanking the user's caret back to an older app.
pub fn hide_our_windows(app: &AppHandle) {
    let app_main = app.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = app.run_on_main_thread(move || {
        if let Some(win) = app_main.get_webview_window("settings") {
            let _ = win.hide();
        }
        let _ = tx.send(());
    });
    let _ = rx.recv_timeout(Duration::from_millis(200));
}

/// Bring the latest target app forward so its caret is live. Only used when
/// Accessibility insert could not write into the background app.
pub fn activate_captured(app: &AppHandle) {
    let pid = captured_pid();
    hide_our_windows(app);
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = app.run_on_main_thread(move || {
        #[cfg(target_os = "macos")]
        if let Some(pid) = pid {
            macos::activate_pid(pid);
        }
        let _ = tx.send(());
    });
    let _ = rx.recv_timeout(Duration::from_millis(400));
    thread::sleep(Duration::from_millis(80));
    let _ = wait_until_frontmost(pid, Duration::from_millis(350));
    refresh_caret();
}

/// True when `pid` is the frontmost app, or when `pid` is None.
pub fn wait_until_frontmost(pid: Option<i32>, timeout: Duration) -> bool {
    let Some(pid) = pid else {
        return true;
    };
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if macos_frontmost_pid() == pid {
            return true;
        }
        thread::sleep(Duration::from_millis(16));
    }
    macos_frontmost_pid() == pid
}

pub fn frontmost_pid() -> i32 {
    macos_frontmost_pid()
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

/// Show the overlay without making Fuk the key app.
pub fn show_overlay(win: &tauri::WebviewWindow, click_through: bool) {
    let _ = win.set_focusable(false);
    let _ = win.set_ignore_cursor_events(click_through);
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
        NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask, NSWorkspace,
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
        window.setStyleMask(window.styleMask() | NSWindowStyleMask::NonactivatingPanel);
        window.setLevel(NSStatusWindowLevel);
        window.orderFrontRegardless();
    }
}
