use anyhow::Result;

use crate::config::{is_wayland, InsertMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertResult {
    Pasted,
    Copied,
}

pub fn insert_text(app: &tauri::AppHandle, text: &str, mode: InsertMode) -> Result<InsertResult> {
    if text.is_empty() {
        return Ok(InsertResult::Copied);
    }
    if mode == InsertMode::ClipboardOnly || is_wayland() {
        set_clipboard(app, text)?;
        return Ok(InsertResult::Copied);
    }
    paste_at_caret(app, text)
}

pub fn retry_insert(app: &tauri::AppHandle, text: &str, mode: InsertMode) -> Result<InsertResult> {
    insert_text(app, text, mode)
}

fn set_clipboard(app: &tauri::AppHandle, text: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        macos::set_pasteboard(app, text)?;
        // NSPasteboard is async with the paste server; Cmd+V too soon pastes
        // the previous contents.
        std::thread::sleep(std::time::Duration::from_millis(90));
        return Ok(());
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        use anyhow::Context;
        use arboard::Clipboard;
        let mut clipboard = Clipboard::new().context("open clipboard")?;
        clipboard.set_text(text).context("set clipboard")?;
        Ok(())
    }
}

fn paste_at_caret(app: &tauri::AppHandle, text: &str) -> Result<InsertResult> {
    crate::focus::stop_caret_watch();
    set_clipboard(app, text)?;

    // Stay hidden until the target app has handled Cmd+V. Bringing the
    // overlay back immediately steals key-window status and the paste dies.
    crate::overlay::hide_for_insert(app);
    crate::focus::hide_our_windows(app);
    crate::focus::activate_captured(app);

    #[cfg(target_os = "macos")]
    {
        if macos::insert_at_live_caret(text) {
            return Ok(InsertResult::Pasted);
        }
        crate::focus::activate_captured(app);
        if let Some(caret) = crate::focus::captured_caret() {
            crate::ax::prepare_insert(&caret);
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        crate::focus::activate_captured(app);
    }
    if let Err(e) = simulate_paste(crate::focus::captured_pid()) {
        log::warn!("paste simulation skipped: {e}");
        #[cfg(target_os = "macos")]
        if !crate::ax::api_available() {
            notify_accessibility_needed(app);
        }
        return Ok(InsertResult::Copied);
    }
    // Let the frontmost app consume Cmd+V before anything of ours reappears.
    std::thread::sleep(std::time::Duration::from_millis(220));
    Ok(InsertResult::Pasted)
}

#[cfg(target_os = "macos")]
fn notify_accessibility_needed(app: &tauri::AppHandle) {
    use tauri::Emitter;
    log::warn!("accessibility not trusted; transcript copied to clipboard only");
    let _ = app.emit(
        "permission-needed",
        crate::pipeline::PermissionNeededPayload {
            kind: "accessibility",
            message: "Text was copied to the clipboard. Enable Accessibility for Fuk \
                      (System Settings → Privacy & Security → Accessibility) so it can \
                      type at the cursor. After reinstalling a build, toggle Fuk off \
                      and on again there."
                .to_string(),
        },
    );
    crate::tray::show_settings_on_main(app);
}

fn simulate_paste(target_pid: Option<i32>) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        macos::cmd_v(target_pid)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = target_pid;
        enigo_paste()
    }
}

#[cfg(not(target_os = "macos"))]
fn enigo_paste() -> Result<()> {
    use anyhow::anyhow;
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};

    let settings = Settings {
        open_prompt_to_get_permissions: false,
        ..Settings::default()
    };
    let mut enigo = Enigo::new(&settings).map_err(|e| anyhow!("enigo: {e}"))?;

    for stuck in [Key::Alt, Key::Shift, Key::Control, Key::Meta] {
        let _ = enigo.key(stuck, Direction::Release);
    }

    enigo
        .key(Key::Control, Direction::Press)
        .map_err(|e| anyhow!("press modifier: {e}"))?;
    enigo
        .key(Key::Unicode('v'), Direction::Click)
        .map_err(|e| anyhow!("press v: {e}"))?;
    enigo
        .key(Key::Control, Direction::Release)
        .map_err(|e| anyhow!("release modifier: {e}"))?;
    Ok(())
}

#[cfg(target_os = "macos")]
mod macos {
    use std::thread;
    use std::time::Duration;

    use anyhow::{anyhow, Result};
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, KeyCode};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    const KEY_V: u16 = 0x09;

    pub fn insert_at_live_caret(text: &str) -> bool {
        // Prefer the field we tracked while the user was actually typing —
        // after the overlay appears, the live system focus is often Fuk or a
        // non-editable web view.
        if let Some(caret) = crate::focus::captured_caret() {
            match crate::ax::insert_caret(&caret, text) {
                Ok(()) => {
                    log::info!(
                        "inserted at tracked caret pid {} range {}+{}",
                        caret.pid,
                        caret.location,
                        caret.length
                    );
                    return true;
                }
                Err(e) => log::info!("tracked caret AX insert: {e}"),
            }
        }
        if let Some(pid) = crate::focus::captured_pid() {
            match crate::ax::insert_into_app(pid, text) {
                Ok(()) => {
                    log::info!("inserted at live caret in pid {pid}");
                    return true;
                }
                Err(e) => log::info!("live focused-field insert: {e}"),
            }
        }
        if let Some(caret) = crate::ax::system_caret() {
            if caret.pid != std::process::id() as i32 {
                match crate::ax::insert_caret(&caret, text) {
                    Ok(()) => {
                        log::info!(
                            "inserted at system caret pid {} range {}+{}",
                            caret.pid,
                            caret.location,
                            caret.length
                        );
                        return true;
                    }
                    Err(e) => log::info!("system caret insert: {e}"),
                }
            }
        }
        false
    }

    pub fn cmd_v(_target_pid: Option<i32>) -> Result<()> {
        // Always HID into the frontmost app. post_to_pid is ignored by
        // Electron/Chrome, which is most of the fields people dictate into.
        let source = event_source()?;
        let cmd = CGEventFlags::CGEventFlagCommand;

        for extra in [
            KeyCode::CONTROL,
            KeyCode::SHIFT,
            KeyCode::OPTION,
            KeyCode::COMMAND,
        ] {
            let up = key(source.clone(), extra, false, CGEventFlags::CGEventFlagNull)?;
            up.post(CGEventTapLocation::HID);
        }
        thread::sleep(Duration::from_millis(20));

        let cmd_down = key(source.clone(), KeyCode::COMMAND, true, cmd)?;
        let v_down = key(source.clone(), KEY_V, true, cmd)?;
        let v_up = key(source.clone(), KEY_V, false, cmd)?;
        let cmd_up = key(source, KeyCode::COMMAND, false, CGEventFlags::CGEventFlagNull)?;

        for event in [&cmd_down, &v_down, &v_up, &cmd_up] {
            event.post(CGEventTapLocation::HID);
            thread::sleep(Duration::from_millis(12));
        }
        Ok(())
    }

    fn event_source() -> Result<CGEventSource> {
        // CombinedSessionState is what Enigo uses for synthesized keypresses.
        // HIDSystemState inherits leftover hardware modifiers (Control from
        // push-to-talk) and the paste never looks like Cmd+V.
        CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
            .or_else(|()| CGEventSource::new(CGEventSourceStateID::HIDSystemState))
            .or_else(|()| CGEventSource::new(CGEventSourceStateID::Private))
            .map_err(|()| anyhow!("could not create a keyboard event source"))
    }

    fn key(source: CGEventSource, code: u16, down: bool, flags: CGEventFlags) -> Result<CGEvent> {
        let event = CGEvent::new_keyboard_event(source, code, down)
            .map_err(|()| anyhow!("could not build a keyboard event"))?;
        event.set_flags(flags);
        Ok(event)
    }

    pub fn set_pasteboard(app: &tauri::AppHandle, text: &str) -> Result<()> {
        use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
        use objc2_foundation::NSString;

        let text = text.to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        let _ = app.run_on_main_thread(move || {
            let pb = NSPasteboard::generalPasteboard();
            pb.clearContents();
            let ok = pb.setString_forType(
                &NSString::from_str(&text),
                unsafe { NSPasteboardTypeString },
            );
            let _ = tx.send(ok);
        });
        match rx.recv_timeout(Duration::from_millis(400)) {
            Ok(true) => Ok(()),
            Ok(false) => Err(anyhow!("NSPasteboard rejected the transcript")),
            Err(_) => Err(anyhow!("timed out writing the pasteboard")),
        }
    }
}
