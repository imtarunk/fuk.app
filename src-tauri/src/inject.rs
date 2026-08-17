use anyhow::{Context, Result};
use arboard::Clipboard;

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
        set_clipboard(text)?;
        return Ok(InsertResult::Copied);
    }
    paste_at_caret(app, text)
}

pub fn retry_insert(app: &tauri::AppHandle, text: &str, mode: InsertMode) -> Result<InsertResult> {
    insert_text(app, text, mode)
}

fn set_clipboard(text: &str) -> Result<()> {
    let mut clipboard = Clipboard::new().context("open clipboard")?;
    clipboard.set_text(text).context("set clipboard")?;
    Ok(())
}

fn paste_at_caret(app: &tauri::AppHandle, text: &str) -> Result<InsertResult> {
    crate::focus::activate_captured(app);

    #[cfg(target_os = "macos")]
    {
        if macos::insert_at_tracked_caret(text) {
            return Ok(InsertResult::Pasted);
        }
    }

    // Keyboard paste needs the text on the pasteboard. Leave it there: if the
    // synthetic Cmd+V is ignored, the user can still paste by hand.
    set_clipboard(text)?;
    if let Err(e) = simulate_paste(crate::focus::captured_pid()) {
        log::warn!("paste simulation skipped: {e}");
        return Ok(InsertResult::Copied);
    }
    Ok(InsertResult::Pasted)
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

    pub fn insert_at_tracked_caret(text: &str) -> bool {
        if let Some(field) = crate::focus::captured_field() {
            match crate::ax::insert_at_caret(&field, text) {
                Ok(()) => {
                    log::info!("inserted at the captured caret");
                    return true;
                }
                Err(e) => log::debug!("captured caret insert: {e}"),
            }
        }
        if let Some(pid) = crate::focus::captured_pid() {
            match crate::ax::insert_into_app(pid, text) {
                Ok(()) => {
                    log::info!("inserted at pid {pid} focused field");
                    return true;
                }
                Err(e) => log::debug!("focused field insert: {e}"),
            }
        }
        false
    }

    pub fn cmd_v(target_pid: Option<i32>) -> Result<()> {
        let source = event_source()?;
        let cmd = CGEventFlags::CGEventFlagCommand;

        // A leftover push-to-talk modifier turns Cmd+V into a different shortcut.
        for extra in [
            KeyCode::CONTROL,
            KeyCode::SHIFT,
            KeyCode::OPTION,
            KeyCode::COMMAND,
        ] {
            let up = key(source.clone(), extra, false, CGEventFlags::CGEventFlagNull)?;
            post_opt(target_pid, &up);
        }
        thread::sleep(Duration::from_millis(12));

        let cmd_down = key(source.clone(), KeyCode::COMMAND, true, cmd)?;
        let v_down = key(source.clone(), KEY_V, true, cmd)?;
        let v_up = key(source.clone(), KEY_V, false, cmd)?;
        let cmd_up = key(source, KeyCode::COMMAND, false, CGEventFlags::CGEventFlagNull)?;

        for event in [&cmd_down, &v_down, &v_up, &cmd_up] {
            post_opt(target_pid, event);
            thread::sleep(Duration::from_millis(8));
        }
        Ok(())
    }

    fn event_source() -> Result<CGEventSource> {
        CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .or_else(|()| CGEventSource::new(CGEventSourceStateID::CombinedSessionState))
            .or_else(|()| CGEventSource::new(CGEventSourceStateID::Private))
            .map_err(|()| anyhow!("could not create a keyboard event source"))
    }

    fn key(source: CGEventSource, code: u16, down: bool, flags: CGEventFlags) -> Result<CGEvent> {
        let event = CGEvent::new_keyboard_event(source, code, down)
            .map_err(|()| anyhow!("could not build a keyboard event"))?;
        event.set_flags(flags);
        Ok(event)
    }

    fn post(pid: i32, event: &CGEvent) {
        event.post_to_pid(pid);
        event.post(CGEventTapLocation::HID);
    }

    fn post_opt(pid: Option<i32>, event: &CGEvent) {
        if let Some(pid) = pid {
            post(pid, event);
        } else {
            event.post(CGEventTapLocation::HID);
        }
    }
}
