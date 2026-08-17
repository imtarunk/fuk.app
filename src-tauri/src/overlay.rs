//! Always-on floating overlay: a tiny idle ball that expands into the speak
//! control while dictating, then collapses back without hiding.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

use crate::config;
use crate::AppState;

pub const IDLE_LOGICAL: f64 = 36.0;
pub const ACTIVE_LOGICAL: f64 = 96.0;

static EXPANDED: AtomicBool = AtomicBool::new(false);
static SAVE_GEN: AtomicU64 = AtomicU64::new(0);

pub fn show_idle(app: &AppHandle) {
    let app = app.clone();
    let app_main = app.clone();
    let _ = app.run_on_main_thread(move || show_idle_inner(&app_main));
}

pub fn expand(app: &AppHandle) {
    let app = app.clone();
    let app_main = app.clone();
    let _ = app.run_on_main_thread(move || expand_inner(&app_main));
}

pub fn collapse(app: &AppHandle) {
    let app = app.clone();
    let app_main = app.clone();
    let _ = app.run_on_main_thread(move || collapse_inner(&app_main));
}

pub fn raise(app: &AppHandle) {
    let app = app.clone();
    let app_main = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(win) = app_main.get_webview_window("pill") {
            present(&win, EXPANDED.load(Ordering::SeqCst));
        }
    });
}

/// Hide the overlay so the user's app can become key before we type.
pub fn hide_for_insert(app: &AppHandle) {
    let app_main = app.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = app.run_on_main_thread(move || {
        if let Some(win) = app_main.get_webview_window("pill") {
            let _ = win.hide();
        }
        let _ = tx.send(());
    });
    let _ = rx.recv_timeout(Duration::from_millis(200));
}

pub fn on_moved(app: &AppHandle, pos: PhysicalPosition<i32>) {
    if EXPANDED.load(Ordering::SeqCst) {
        return;
    }
    let gen = SAVE_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(350));
        if SAVE_GEN.load(Ordering::SeqCst) != gen {
            return;
        }
        persist_position(&app, pos);
    });
}

fn show_idle_inner(app: &AppHandle) {
    let Some(win) = app.get_webview_window("pill") else {
        return;
    };
    EXPANDED.store(false, Ordering::SeqCst);
    let scale = scale_of(&win);
    let size = idle_size(scale);
    let _ = win.set_size(tauri::Size::Physical(size));
    let pos = saved_or_default(app, &win, scale, size);
    let _ = win.set_position(tauri::Position::Physical(pos));
    present(&win, false);
}

fn expand_inner(app: &AppHandle) {
    let Some(win) = app.get_webview_window("pill") else {
        return;
    };
    let scale = scale_of(&win);
    let idle = idle_size(scale);
    let active = active_size(scale);
    let current = win.outer_position().unwrap_or_else(|_| default_pos(&win, scale, idle));
    let already = EXPANDED.swap(true, Ordering::SeqCst);
    let pos = if already {
        win.outer_position().unwrap_or(current)
    } else {
        grow_origin(current, scale)
    };
    let _ = win.set_size(tauri::Size::Physical(active));
    let _ = win.set_position(tauri::Position::Physical(pos));
    present(&win, true);
}

fn collapse_inner(app: &AppHandle) {
    let Some(win) = app.get_webview_window("pill") else {
        return;
    };
    let scale = scale_of(&win);
    let idle = idle_size(scale);
    let current = win.outer_position().ok();
    let was_expanded = EXPANDED.swap(false, Ordering::SeqCst);
    let pos = if was_expanded {
        current.map(|p| shrink_origin(p, scale)).unwrap_or_else(|| {
            saved_or_default(app, &win, scale, idle)
        })
    } else {
        current.unwrap_or_else(|| saved_or_default(app, &win, scale, idle))
    };
    let _ = win.set_size(tauri::Size::Physical(idle));
    let _ = win.set_position(tauri::Position::Physical(pos));
    persist_position(app, pos);
    present(&win, false);
}

fn present(win: &WebviewWindow, click_through: bool) {
    crate::focus::show_overlay(win, click_through);
}

fn persist_position(app: &AppHandle, pos: PhysicalPosition<i32>) {
    let state = app.state::<AppState>();
    let mut cfg = state.config.lock();
    if cfg.overlay_x == Some(pos.x) && cfg.overlay_y == Some(pos.y) {
        return;
    }
    cfg.overlay_x = Some(pos.x);
    cfg.overlay_y = Some(pos.y);
    if let Err(e) = config::save(&cfg) {
        log::warn!("save overlay position: {e}");
    }
}

fn saved_or_default(
    app: &AppHandle,
    win: &WebviewWindow,
    scale: f64,
    size: PhysicalSize<u32>,
) -> PhysicalPosition<i32> {
    let cfg = app.state::<AppState>().config.lock().clone();
    if let (Some(x), Some(y)) = (cfg.overlay_x, cfg.overlay_y) {
        return clamp_to_monitor(win, PhysicalPosition::new(x, y), size);
    }
    default_pos(win, scale, size)
}

fn default_pos(win: &WebviewWindow, scale: f64, size: PhysicalSize<u32>) -> PhysicalPosition<i32> {
    let monitor = win
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| win.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return PhysicalPosition::new(24, 24);
    };
    let mpos = monitor.position();
    let msize = monitor.size();
    let margin = (24.0 * scale) as i32;
    let x = mpos.x + (msize.width as i32 - size.width as i32) / 2;
    let y = mpos.y + msize.height as i32 - size.height as i32 - margin;
    PhysicalPosition::new(x, y)
}

fn clamp_to_monitor(
    win: &WebviewWindow,
    pos: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
) -> PhysicalPosition<i32> {
    let monitor = win
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| win.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return pos;
    };
    let mpos = monitor.position();
    let msize = monitor.size();
    let max_x = mpos.x + msize.width as i32 - size.width as i32;
    let max_y = mpos.y + msize.height as i32 - size.height as i32;
    PhysicalPosition::new(pos.x.clamp(mpos.x, max_x.max(mpos.x)), pos.y.clamp(mpos.y, max_y.max(mpos.y)))
}

fn scale_of(win: &WebviewWindow) -> f64 {
    win.current_monitor()
        .ok()
        .flatten()
        .or_else(|| win.primary_monitor().ok().flatten())
        .map(|m| m.scale_factor())
        .unwrap_or(1.0)
}

fn idle_size(scale: f64) -> PhysicalSize<u32> {
    PhysicalSize::new((IDLE_LOGICAL * scale) as u32, (IDLE_LOGICAL * scale) as u32)
}

fn active_size(scale: f64) -> PhysicalSize<u32> {
    PhysicalSize::new((ACTIVE_LOGICAL * scale) as u32, (ACTIVE_LOGICAL * scale) as u32)
}

fn inset(scale: f64) -> i32 {
    ((ACTIVE_LOGICAL - IDLE_LOGICAL) * 0.5 * scale).round() as i32
}

pub fn grow_origin(idle_pos: PhysicalPosition<i32>, scale: f64) -> PhysicalPosition<i32> {
    let d = inset(scale);
    PhysicalPosition::new(idle_pos.x - d, idle_pos.y - d)
}

pub fn shrink_origin(active_pos: PhysicalPosition<i32>, scale: f64) -> PhysicalPosition<i32> {
    let d = inset(scale);
    PhysicalPosition::new(active_pos.x + d, active_pos.y + d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_keeps_center() {
        let idle = PhysicalPosition::new(100, 200);
        let grown = grow_origin(idle, 1.0);
        assert_eq!(grown, PhysicalPosition::new(70, 170));
        assert_eq!(shrink_origin(grown, 1.0), idle);
    }
}
