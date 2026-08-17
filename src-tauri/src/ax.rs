//! Insert text at the live caret of the focused field in another app.
//!
//! The caret is re-read at insert time so a click or arrow key during dictation
//! still lands the transcript at the latest insertion point.

use std::ffi::c_void;
use std::ptr;

use anyhow::{anyhow, Result};
use core_foundation::array::{CFArrayGetCount, CFArrayGetValueAtIndex};
use core_foundation::base::{CFRelease, CFRetain, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::string::{CFString, CFStringRef};

pub type AXUIElementRef = *const c_void;
type AXError = i32;
type AXValueRef = *const c_void;

const AX_SUCCESS: AXError = 0;
const AX_ERROR_API_DISABLED: AXError = -25211;
const AX_VALUE_CF_RANGE: u32 = 4;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct CFRange {
    location: isize,
    length: isize,
}

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

    pub fn ptr_eq(&self, other: &Self) -> bool {
        self.raw == other.raw
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

/// Focused field plus the UTF-16 caret/selection inside it.
#[derive(Clone)]
pub struct Caret {
    pub pid: i32,
    pub element: AxElem,
    pub location: isize,
    pub length: isize,
}

/// The text field that currently has the caret in `pid`, if Accessibility can
/// see it.
fn focused_element_timeout(pid: i32, timeout: f32) -> Option<AxElem> {
    if pid <= 0 {
        return None;
    }
    let app = AxElem::from_create(unsafe { AXUIElementCreateApplication(pid) })?;
    unsafe {
        AXUIElementSetMessagingTimeout(app.raw, timeout);
    }
    copy_element(&app, "AXFocusedUIElement").or_else(|| {
        let window = copy_element(&app, "AXFocusedWindow")?;
        copy_element(&window, "AXFocusedUIElement")
    })
}

/// True when Accessibility is actually usable. `AXIsProcessTrusted()` is a
/// known false negative for ad-hoc signed accessory apps, so we ask the
/// system-wide element instead: `kAXErrorAPIDisabled` is the only "no".
pub fn api_available() -> bool {
    let Some(sys) = AxElem::from_create(unsafe { AXUIElementCreateSystemWide() }) else {
        return false;
    };
    unsafe {
        AXUIElementSetMessagingTimeout(sys.raw, 0.08);
    }
    let attr = CFString::from_static_string("AXFocusedUIElement");
    let mut value: CFTypeRef = ptr::null();
    let err = unsafe {
        AXUIElementCopyAttributeValue(sys.raw, attr.as_concrete_TypeRef(), &mut value)
    };
    if !value.is_null() {
        unsafe {
            CFRelease(value);
        }
    }
    err != AX_ERROR_API_DISABLED
}

/// Live caret anywhere on the system: the field that currently owns the
/// insertion point, no matter which application it belongs to. This is the
/// canonical global tracker — it follows the caret into any app the user
/// focuses after launch.
pub fn system_caret() -> Option<Caret> {
    let sys = AxElem::from_create(unsafe { AXUIElementCreateSystemWide() })?;
    unsafe {
        AXUIElementSetMessagingTimeout(sys.raw, 0.15);
    }
    let element = copy_element(&sys, "AXFocusedUIElement")?;
    let pid = element_pid(&element)?;
    let (location, length) = selected_range(&element).unwrap_or((0, 0));
    Some(resolve_caret(Caret {
        pid,
        element,
        location,
        length,
    }))
}

fn element_pid(element: &AxElem) -> Option<i32> {
    let mut pid: i32 = 0;
    let err = unsafe { AXUIElementGetPid(element.raw, &mut pid) };
    if err == AX_SUCCESS && pid > 0 {
        Some(pid)
    } else {
        None
    }
}

/// Live caret in `pid`: focused field and its current selected-text range.
pub fn snapshot_caret(pid: i32) -> Option<Caret> {
    snapshot_caret_timeout(pid, 0.12)
}

pub fn snapshot_caret_now(pid: i32) -> Option<Caret> {
    snapshot_caret_timeout(pid, 0.35)
}

fn snapshot_caret_timeout(pid: i32, timeout: f32) -> Option<Caret> {
    let element = focused_element_timeout(pid, timeout)?;
    let (location, length) = selected_range(&element).unwrap_or((0, 0));
    Some(resolve_caret(Caret {
        pid,
        element,
        location,
        length,
    }))
}

/// Raise the field and give it AX focus so a later write or keystroke lands
/// in it even if the overlay briefly stole key-window status.
pub fn prepare_insert(caret: &Caret) {
    unsafe {
        AXUIElementSetMessagingTimeout(caret.element.raw, 0.45);
    }
    focus_element(&caret.element);
}

pub fn is_editable(element: &AxElem) -> bool {
    attribute_settable(element, "AXSelectedText")
        || (attribute_settable(element, "AXValue") && is_text_role(element))
}

/// Insert `text` at the element's current caret, or at `caret`'s stored range
/// if the live range cannot be read. Returns Ok only when the write can be
/// trusted — a successful `AXSelectedText` on a web view is not enough.
pub fn insert_caret(caret: &Caret, text: &str) -> Result<()> {
    prepare_insert(caret);
    let live = selected_range(&caret.element);
    let range = live.or(Some((caret.location, caret.length)));
    insert_at_range(&caret.element, range, text)
}

pub fn insert_into_app(pid: i32, text: &str) -> Result<()> {
    let caret =
        snapshot_caret_now(pid).ok_or_else(|| anyhow!("no focused field in pid {pid}"))?;
    insert_caret(&caret, text)
}

fn insert_at_range(element: &AxElem, range: Option<(isize, isize)>, text: &str) -> Result<()> {
    if let Some((loc, len)) = range {
        let _ = set_selected_range(element, loc, len);
    }

    let before = string_attr(element, "AXValue");

    if attribute_settable(element, "AXSelectedText")
        && set_string_attr(element, "AXSelectedText", text).is_ok()
        && write_landed(element, before.as_deref(), range, text)
    {
        place_caret_after_insert(element, range, text);
        return Ok(());
    }

    let Some((loc, len)) = range.or_else(|| selected_range(element)) else {
        return Err(anyhow!(
            "AXSelectedText did not land (role {:?})",
            role(element)
        ));
    };
    if !attribute_settable(element, "AXValue") {
        return Err(anyhow!(
            "element role {:?} is not an AX-writable text field",
            role(element)
        ));
    }
    let existing = before
        .clone()
        .or_else(|| string_attr(element, "AXValue"))
        .unwrap_or_default();
    let spliced = utf16_splice(&existing, loc, len, text);
    set_string_attr(element, "AXValue", &spliced)?;
    if !write_landed(element, Some(&existing), Some((loc, len)), text) {
        return Err(anyhow!("AXValue write did not stick"));
    }
    place_caret_after_insert(element, Some((loc, len)), text);
    Ok(())
}

fn write_landed(
    element: &AxElem,
    before: Option<&str>,
    range: Option<(isize, isize)>,
    text: &str,
) -> bool {
    match string_attr(element, "AXValue") {
        None => is_text_role(element),
        Some(after) => {
            if let (Some(before), Some((loc, len))) = (before, range) {
                if after == utf16_splice(before, loc, len, text) {
                    return true;
                }
            }
            before.is_none_or(|b| b != after) && after.contains(text)
        }
    }
}

fn resolve_caret(mut caret: Caret) -> Caret {
    if is_editable(&caret.element) {
        return caret;
    }
    let mut budget = 48;
    if let Some(found) = find_editable(&caret.element, 5, &mut budget) {
        caret.element = found;
        if let Some((location, length)) = selected_range(&caret.element) {
            caret.location = location;
            caret.length = length;
        }
    }
    caret
}

fn find_editable(element: &AxElem, depth: u8, budget: &mut i32) -> Option<AxElem> {
    if *budget <= 0 || depth == 0 {
        return None;
    }
    *budget -= 1;
    let kids = children(element);
    for child in &kids {
        if bool_attr(child, "AXFocused") == Some(true) && is_editable(child) {
            return Some(child.clone());
        }
    }
    for child in &kids {
        if is_editable(child) {
            return Some(child.clone());
        }
    }
    for child in &kids {
        if bool_attr(child, "AXFocused") == Some(true) {
            if let Some(found) = find_editable(child, depth - 1, budget) {
                return Some(found);
            }
        }
    }
    for child in kids {
        if let Some(found) = find_editable(&child, depth - 1, budget) {
            return Some(found);
        }
    }
    None
}

fn children(element: &AxElem) -> Vec<AxElem> {
    let Some(value) = copy_attr(element, "AXChildren") else {
        return Vec::new();
    };
    let count = unsafe { CFArrayGetCount(value as _) };
    let mut out = Vec::with_capacity(count.max(0) as usize);
    for i in 0..count {
        let item = unsafe { CFArrayGetValueAtIndex(value as _, i) };
        if item.is_null() {
            continue;
        }
        unsafe {
            CFRetain(item);
        }
        if let Some(child) = AxElem::from_create(item as AXUIElementRef) {
            out.push(child);
        }
    }
    unsafe {
        CFRelease(value);
    }
    out
}

fn focus_element(element: &AxElem) {
    if let Some(window) = copy_element(element, "AXWindow") {
        let _ = perform_action(&window, "AXRaise");
    }
    let _ = set_bool_attr(element, "AXFocused", true);
}

fn is_text_role(element: &AxElem) -> bool {
    matches!(
        role(element).as_deref(),
        Some("AXTextField" | "AXTextArea" | "AXComboBox" | "AXSearchField")
    )
}

fn role(element: &AxElem) -> Option<String> {
    string_attr(element, "AXRole")
}

fn attribute_settable(element: &AxElem, name: &'static str) -> bool {
    let attr = CFString::from_static_string(name);
    let mut settable: u8 = 0;
    let err = unsafe {
        AXUIElementIsAttributeSettable(element.raw, attr.as_concrete_TypeRef(), &mut settable)
    };
    err == AX_SUCCESS && settable != 0
}

fn bool_attr(element: &AxElem, name: &'static str) -> Option<bool> {
    let value = copy_attr(element, name)?;
    let cf = unsafe { CFBoolean::wrap_under_create_rule(value as _) };
    Some(bool::from(cf))
}

fn set_bool_attr(element: &AxElem, name: &'static str, value: bool) -> Result<()> {
    let attr = CFString::from_static_string(name);
    let cf = CFBoolean::from(value);
    let err = unsafe {
        AXUIElementSetAttributeValue(
            element.raw,
            attr.as_concrete_TypeRef(),
            cf.as_concrete_TypeRef() as CFTypeRef,
        )
    };
    if err == AX_SUCCESS {
        Ok(())
    } else {
        Err(anyhow!("{name} failed ({err})"))
    }
}

fn perform_action(element: &AxElem, name: &'static str) -> Result<()> {
    let action = CFString::from_static_string(name);
    let err = unsafe { AXUIElementPerformAction(element.raw, action.as_concrete_TypeRef()) };
    if err == AX_SUCCESS {
        Ok(())
    } else {
        Err(anyhow!("{name} failed ({err})"))
    }
}

fn place_caret_after_insert(element: &AxElem, range: Option<(isize, isize)>, text: &str) {
    let loc = range.map(|(l, _)| l).unwrap_or(0);
    let inserted = text.encode_utf16().count() as isize;
    let _ = set_selected_range(element, loc + inserted, 0);
}

fn selected_range(element: &AxElem) -> Option<(isize, isize)> {
    let value = copy_attr(element, "AXSelectedTextRange")?;
    let mut range = CFRange {
        location: 0,
        length: 0,
    };
    let ok = unsafe {
        AXValueGetValue(value, AX_VALUE_CF_RANGE, &mut range as *mut _ as *mut c_void)
    } != 0;
    unsafe {
        CFRelease(value);
    }
    if ok {
        Some((range.location.max(0), range.length.max(0)))
    } else {
        None
    }
}

fn set_selected_range(element: &AxElem, location: isize, length: isize) -> Result<()> {
    let range = CFRange { location, length };
    let value = unsafe { AXValueCreate(AX_VALUE_CF_RANGE, &range as *const _ as *const c_void) };
    if value.is_null() {
        return Err(anyhow!("AXValueCreate range failed"));
    }
    let attr = CFString::from_static_string("AXSelectedTextRange");
    let err = unsafe {
        AXUIElementSetAttributeValue(element.raw, attr.as_concrete_TypeRef(), value)
    };
    unsafe {
        CFRelease(value);
    }
    if err == AX_SUCCESS {
        Ok(())
    } else {
        Err(anyhow!("AXSelectedTextRange failed ({err})"))
    }
}

fn string_attr(element: &AxElem, name: &'static str) -> Option<String> {
    let value = copy_attr(element, name)?;
    let cf = unsafe { CFString::wrap_under_create_rule(value as CFStringRef) };
    Some(cf.to_string())
}

fn set_string_attr(element: &AxElem, name: &'static str, text: &str) -> Result<()> {
    let attr = CFString::from_static_string(name);
    let value = CFString::new(text);
    let err = unsafe {
        AXUIElementSetAttributeValue(
            element.raw,
            attr.as_concrete_TypeRef(),
            value.as_concrete_TypeRef() as CFTypeRef,
        )
    };
    if err == AX_SUCCESS {
        Ok(())
    } else {
        Err(anyhow!("{name} failed ({err})"))
    }
}

fn copy_attr(element: &AxElem, name: &'static str) -> Option<CFTypeRef> {
    let attr = CFString::from_static_string(name);
    let mut value: CFTypeRef = ptr::null();
    let err = unsafe {
        AXUIElementCopyAttributeValue(element.raw, attr.as_concrete_TypeRef(), &mut value)
    };
    if err != AX_SUCCESS || value.is_null() {
        None
    } else {
        Some(value)
    }
}

fn copy_element(parent: &AxElem, name: &'static str) -> Option<AxElem> {
    let value = copy_attr(parent, name)?;
    AxElem::from_create(value as AXUIElementRef)
}

fn utf16_splice(existing: &str, location: isize, length: isize, insert: &str) -> String {
    let units: Vec<u16> = existing.encode_utf16().collect();
    let loc = (location.max(0) as usize).min(units.len());
    let end = loc.saturating_add(length.max(0) as usize).min(units.len());
    let mut out = Vec::with_capacity(units.len() + insert.encode_utf16().count());
    out.extend_from_slice(&units[..loc]);
    out.extend(insert.encode_utf16());
    out.extend_from_slice(&units[end..]);
    String::from_utf16_lossy(&out)
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementGetPid(element: AXUIElementRef, pid: *mut i32) -> AXError;
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
    fn AXUIElementIsAttributeSettable(
        element: AXUIElementRef,
        attribute: CFStringRef,
        settable: *mut u8,
    ) -> AXError;
    fn AXUIElementPerformAction(element: AXUIElementRef, action: CFStringRef) -> AXError;
    fn AXValueCreate(the_type: u32, value_ptr: *const c_void) -> AXValueRef;
    fn AXValueGetValue(value: AXValueRef, the_type: u32, value_ptr: *mut c_void) -> u8;
}

#[cfg(test)]
mod tests {
    use super::utf16_splice;

    #[test]
    fn splices_at_caret() {
        assert_eq!(utf16_splice("hello world", 6, 0, "there "), "hello there world");
    }

    #[test]
    fn replaces_selection() {
        assert_eq!(utf16_splice("hello world", 6, 5, "there"), "hello there");
    }
}
