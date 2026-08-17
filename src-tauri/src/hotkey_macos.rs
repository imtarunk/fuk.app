//! Raw key stream for macOS, used only when the binding is a bare key that
//! Carbon cannot express (a lone modifier, for example).
//!
//! Two independent sources, both listen-only so we never eat the key:
//!
//! * `NSEvent` global monitor — returns `nil` unless the process is trusted.
//! * A HID `CGEventTap` in listen-only mode — this is what actually sees keys
//!   while another app is focused. Creating it does not prove Accessibility;
//!   events arriving in the background does.
//!
//! A local NSEvent monitor covers keys while Fuk itself is focused.
//! Local-only is not treated as "listening": that was the Settings-only bug.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use block2::RcBlock;
use core_foundation::base::TCFType;
use core_foundation::mach_port::CFMachPortRef;
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
use core_graphics::event::{
    CGEvent, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
    EventField,
};
use objc2_app_kit::{NSEvent, NSEventMask, NSEventType};
use objc2_foundation::{ns_string, NSActivityOptions, NSProcessInfo};
use parking_lot::Mutex;

use crate::hotkey::{key_from_code, send, Signal};

static LOCAL: AtomicBool = AtomicBool::new(false);
static GLOBAL_MONITOR: AtomicBool = AtomicBool::new(false);
static TAP: AtomicBool = AtomicBool::new(false);
static NAP: AtomicBool = AtomicBool::new(false);
static TAP_PORT: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static LAST_FLAGS: AtomicU64 = AtomicU64::new(0);
static LAST_EMIT: Mutex<Option<(u16, bool, Instant)>> = Mutex::new(None);

pub fn is_installed() -> bool {
    GLOBAL_MONITOR.load(Ordering::SeqCst) || TAP.load(Ordering::SeqCst)
}

pub fn keepalive() {
    enable_tap();
}

/// Must be called on the main thread.
pub fn install() -> Result<(), String> {
    prevent_app_nap();
    install_local();
    install_global_monitor();
    install_hid_tap();

    if is_installed() {
        log::info!(
            "macOS raw hotkey sources: nsevent-global={} hid-tap={}",
            GLOBAL_MONITOR.load(Ordering::SeqCst),
            TAP.load(Ordering::SeqCst)
        );
        Ok(())
    } else {
        Err(
            "no system-wide keyboard source; toggle Fuk off and on in Accessibility and Input Monitoring"
                .into(),
        )
    }
}

fn install_local() {
    if LOCAL.load(Ordering::SeqCst) {
        return;
    }
    let mask = NSEventMask::KeyDown | NSEventMask::KeyUp | NSEventMask::FlagsChanged;
    let local_block = RcBlock::new(|event: NonNull<NSEvent>| -> *mut NSEvent {
        handle_ns_event(unsafe { event.as_ref() });
        event.as_ptr()
    });
    let local = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &local_block) };
    Box::leak(Box::new(local_block));
    if let Some(local) = local {
        Box::leak(Box::new(local));
        LOCAL.store(true, Ordering::SeqCst);
    }
}

fn install_global_monitor() {
    if GLOBAL_MONITOR.load(Ordering::SeqCst) {
        return;
    }
    let mask = NSEventMask::KeyDown | NSEventMask::KeyUp | NSEventMask::FlagsChanged;
    let global_block = RcBlock::new(|event: NonNull<NSEvent>| {
        handle_ns_event(unsafe { event.as_ref() });
    });
    let global = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(mask, &global_block);
    Box::leak(Box::new(global_block));
    if let Some(global) = global {
        Box::leak(Box::new(global));
        GLOBAL_MONITOR.store(true, Ordering::SeqCst);
    }
}

fn install_hid_tap() {
    if TAP.load(Ordering::SeqCst) {
        return;
    }
    let tap = match CGEventTap::new(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::ListenOnly,
        vec![
            CGEventType::KeyDown,
            CGEventType::KeyUp,
            CGEventType::FlagsChanged,
        ],
        |_proxy, etype, event| {
            match etype {
                CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
                    enable_tap();
                }
                CGEventType::KeyDown | CGEventType::KeyUp | CGEventType::FlagsChanged => {
                    handle_cg_event(etype, event);
                }
                _ => {}
            }
            None
        },
    ) {
        Ok(tap) => tap,
        Err(()) => {
            log::warn!("HID listen-only event tap was not created");
            return;
        }
    };

    let Ok(source) = tap.mach_port.create_runloop_source(0) else {
        log::warn!("HID event tap has no run-loop source");
        return;
    };
    CFRunLoop::get_main().add_source(&source, unsafe { kCFRunLoopCommonModes });
    tap.enable();

    let port = tap.mach_port.as_concrete_TypeRef() as *mut c_void;
    TAP_PORT.store(port, Ordering::SeqCst);
    Box::leak(Box::new(source));
    Box::leak(Box::new(tap));
    TAP.store(true, Ordering::SeqCst);
    log::info!("macOS HID listen-only keyboard tap installed");
}

fn enable_tap() {
    let port = TAP_PORT.load(Ordering::SeqCst);
    if port.is_null() {
        return;
    }
    unsafe {
        CGEventTapEnable(port as CFMachPortRef, true);
    }
}

fn handle_ns_event(event: &NSEvent) {
    let event_type = event.r#type();
    if event_type == NSEventType::KeyDown && event.isARepeat() {
        return;
    }

    let code = event.keyCode();
    let flags = event.modifierFlags().0 as u64;
    let previous = LAST_FLAGS.swap(flags, Ordering::SeqCst);
    let press = if event_type == NSEventType::FlagsChanged {
        match device_mask(code) {
            Some(mask) => flags & mask != 0,
            None => flags > previous,
        }
    } else {
        event_type == NSEventType::KeyDown
    };

    emit(code, press);
}

fn handle_cg_event(etype: CGEventType, event: &CGEvent) {
    let pid = event.get_integer_value_field(EventField::EVENT_TARGET_UNIX_PROCESS_ID) as i32;
    crate::focus::note_event_pid(pid);

    let is_repeat = event.get_integer_value_field(EventField::KEYBOARD_EVENT_AUTOREPEAT) != 0;
    let code = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u16;
    let flags = event.get_flags().bits();
    let previous = LAST_FLAGS.swap(flags, Ordering::SeqCst);
    let press = match etype {
        CGEventType::FlagsChanged => match device_mask(code) {
            Some(mask) => flags & mask != 0,
            None => flags > previous,
        },
        CGEventType::KeyDown => {
            if is_repeat {
                return;
            }
            true
        }
        CGEventType::KeyUp => false,
        _ => return,
    };

    emit(code, press);
}

fn emit(code: u16, press: bool) {
    {
        let mut last = LAST_EMIT.lock();
        if let Some((prev_code, prev_press, at)) = *last {
            if prev_code == code && prev_press == press && at.elapsed() < Duration::from_millis(16)
            {
                return;
            }
        }
        *last = Some((code, press, Instant::now()));
    }
    send(Signal::Raw {
        key: key_from_code(code),
        press,
    });
}

fn prevent_app_nap() {
    if NAP.swap(true, Ordering::SeqCst) {
        return;
    }
    let info = NSProcessInfo::processInfo();
    let token = info.beginActivityWithOptions_reason(
        NSActivityOptions::UserInitiated,
        ns_string!("Fuk push-to-talk hotkey"),
    );
    Box::leak(Box::new(token));
}

/// Per-modifier bits from `IOKit/hidsystem/IOLLEvent.h`, plus the two CGEvent
/// flags that have no left/right split.
fn device_mask(keycode: u16) -> Option<u64> {
    Some(match keycode {
        59 => 0x0000_0001, // NX_DEVICELCTLKEYMASK
        56 => 0x0000_0002, // NX_DEVICELSHIFTKEYMASK
        60 => 0x0000_0004, // NX_DEVICERSHIFTKEYMASK
        55 => 0x0000_0008, // NX_DEVICELCMDKEYMASK
        54 => 0x0000_0010, // NX_DEVICERCMDKEYMASK
        58 => 0x0000_0020, // NX_DEVICELALTKEYMASK
        61 => 0x0000_0040, // NX_DEVICERALTKEYMASK
        62 => 0x0000_2000, // NX_DEVICERCTLKEYMASK
        57 => 0x0001_0000, // kCGEventFlagMaskAlphaShift
        63 => 0x0080_0000, // kCGEventFlagMaskSecondaryFn
        _ => return None,
    })
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
}
