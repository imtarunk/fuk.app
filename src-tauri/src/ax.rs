//! Insert text at the caret of the focused field in another app.
//!
//! Cmd+V only works if that app is key and Accessibility is actually trusted.
//! Setting `AXSelectedText` on the focused AX element writes at the cursor
//! without stealing the clipboard, which is what the user asked for.

use std::ffi::c_void;
use std::ptr;

use anyhow::{anyhow, Result};
use core_foundation::base::{CFRelease, CFRetain, CFTypeRef, TCFType};
use core_foundation::string::{CFString, CFStringRef};

pub type AXUIElementRef = *const c_void;
type AXError = i32;

const AX_SUCCESS: AXError = 0;

pub struct AxElem {
    raw: AXUIElementRef,
}

unsafe impl Send for AxElem {}
unsafe impl Sync for AxElem {}

impl AxElem {
    fn from_create(raw: AXUIElementRef) -> Option<Self> {
        if raw.is_null() {
            None
        } else {
            Some(Self { raw })
        }
    }
}

impl Clone for AxElem {
    fn clone(&self) -> Self {
        unsafe {
            CFRetain(self.raw);
        }
        Self { raw: self.raw }
    }
}

impl Drop for AxElem {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe {
                CFRelease(self.raw);
            }
        }
    }
}

/// The text field that currently has the caret in `pid`, if Accessibility can
/// see it.
pub fn focused_element(pid: i32) -> Option<AxElem> {
    if pid <= 0 {
        return None;
    }
    let app = AxElem::from_create(unsafe { AXUIElementCreateApplication(pid) })?;
    unsafe {
        AXUIElementSetMessagingTimeout(app.raw, 0.4);
    }
    copy_element(&app, "AXFocusedUIElement").or_else(|| {
        // Some apps only expose the focused widget through the focused window.
        let window = copy_element(&app, "AXFocusedWindow")?;
        copy_element(&window, "AXFocusedUIElement")
    })
}

/// Replace the current selection (or insert at an empty caret) on `element`.
pub fn insert_at_caret(element: &AxElem, text: &str) -> Result<()> {
    let attr = CFString::from_static_string("AXSelectedText");
    let value = CFString::new(text);
    let err = unsafe {
        AXUIElementSetAttributeValue(
            element.raw,
            attr.as_concrete_TypeRef(),
            value.as_concrete_TypeRef() as CFTypeRef,
        )
    };
    if err == AX_SUCCESS {
        return Ok(());
    }
    Err(anyhow!("AXSelectedText failed ({err})"))
}

pub fn insert_into_app(pid: i32, text: &str) -> Result<()> {
    let element =
        focused_element(pid).ok_or_else(|| anyhow!("no focused field in pid {pid}"))?;
    insert_at_caret(&element, text)
}

fn copy_element(parent: &AxElem, name: &'static str) -> Option<AxElem> {
    let attr = CFString::from_static_string(name);
    let mut value: CFTypeRef = ptr::null();
    let err = unsafe {
        AXUIElementCopyAttributeValue(parent.raw, attr.as_concrete_TypeRef(), &mut value)
    };
    if err != AX_SUCCESS || value.is_null() {
        return None;
    }
    AxElem::from_create(value as AXUIElementRef)
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> AXError;
    fn AXUIElementSetMessagingTimeout(element: AXUIElementRef, timeout_in_seconds: f32);
}
