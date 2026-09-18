//! macOS harvest of visible context for vocabulary:
//! 1) On-screen window titles via CoreGraphics (works for Electron/Slack DMs)
//! 2) Priority-app Accessibility trees (when AX exposes content)
//! 3) Focused non-IDE UI

use core_foundation::base::{CFTypeRef, TCFType};
use core_foundation::string::{CFString, CFStringRef};
use std::ffi::c_void;
use std::ptr;

type AXUIElementRef = *mut c_void;
type CFIndex = isize;
type Pid = i32;

const K_AX_ERROR_SUCCESS: i32 = 0;
/// NSApplicationActivationPolicyRegular
const ACTIVATION_POLICY_REGULAR: i64 = 0;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCreateApplication(pid: Pid) -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;
    fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> i32;
    fn AXUIElementGetAttributeValueCount(
        element: AXUIElementRef,
        attribute: CFStringRef,
        count: *mut CFIndex,
    ) -> i32;
    fn AXUIElementCopyAttributeValues(
        element: AXUIElementRef,
        attribute: CFStringRef,
        index: CFIndex,
        max_values: CFIndex,
        values: *mut CFTypeRef,
    ) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: CFTypeRef);
    fn CFGetTypeID(cf: CFTypeRef) -> usize;
    fn CFStringGetTypeID() -> usize;
    static kCFBooleanTrue: CFTypeRef;
}

/// AX errors we care about when enabling Electron accessibility.
const K_AX_ERROR_ATTRIBUTE_UNSUPPORTED: i32 = -25205;
const K_AX_ERROR_FAILURE: i32 = -25200;

fn cf_str(name: &str) -> CFString {
    CFString::new(name)
}

unsafe fn copy_attr(element: AXUIElementRef, attr: &str) -> Option<CFTypeRef> {
    if element.is_null() {
        return None;
    }
    let mut value: CFTypeRef = ptr::null();
    let status =
        AXUIElementCopyAttributeValue(element, cf_str(attr).as_concrete_TypeRef(), &mut value);
    if status != K_AX_ERROR_SUCCESS || value.is_null() {
        None
    } else {
        Some(value)
    }
}

unsafe fn copy_string_attr(element: AXUIElementRef, attr: &str) -> Option<String> {
    let value = copy_attr(element, attr)?;
    if CFGetTypeID(value) != CFStringGetTypeID() {
        CFRelease(value);
        return None;
    }
    let s = CFString::wrap_under_create_rule(value as CFStringRef);
    let text = s.to_string();
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

unsafe fn copy_children(element: AXUIElementRef, max: CFIndex) -> Vec<AXUIElementRef> {
    if element.is_null() || max <= 0 {
        return Vec::new();
    }
    let attr = cf_str("AXChildren");
    let mut count: CFIndex = 0;
    if AXUIElementGetAttributeValueCount(element, attr.as_concrete_TypeRef(), &mut count)
        != K_AX_ERROR_SUCCESS
        || count <= 0
    {
        return Vec::new();
    }
    let take = count.min(max);
    let mut values: Vec<CFTypeRef> = vec![ptr::null(); take as usize];
    if AXUIElementCopyAttributeValues(
        element,
        attr.as_concrete_TypeRef(),
        0,
        take,
        values.as_mut_ptr(),
    ) != K_AX_ERROR_SUCCESS
    {
        return Vec::new();
    }
    values
        .into_iter()
        .filter(|v| !v.is_null())
        .map(|v| v as AXUIElementRef)
        .collect()
}

unsafe fn copy_windows(app: AXUIElementRef, max: CFIndex) -> Vec<AXUIElementRef> {
    if app.is_null() || max <= 0 {
        return Vec::new();
    }
    let attr = cf_str("AXWindows");
    let mut count: CFIndex = 0;
    if AXUIElementGetAttributeValueCount(app, attr.as_concrete_TypeRef(), &mut count)
        != K_AX_ERROR_SUCCESS
        || count <= 0
    {
        return Vec::new();
    }
    let take = count.min(max);
    let mut values: Vec<CFTypeRef> = vec![ptr::null(); take as usize];
    if AXUIElementCopyAttributeValues(app, attr.as_concrete_TypeRef(), 0, take, values.as_mut_ptr())
        != K_AX_ERROR_SUCCESS
    {
        return Vec::new();
    }
    values
        .into_iter()
        .filter(|v| !v.is_null())
        .map(|v| v as AXUIElementRef)
        .collect()
}

struct HarvestBudget {
    max_chars: usize,
    max_nodes: usize,
    chars: usize,
    nodes: usize,
    chunks: Vec<String>,
}

impl HarvestBudget {
    fn new(max_chars: usize) -> Self {
        Self {
            max_chars,
            max_nodes: 400,
            chars: 0,
            nodes: 0,
            chunks: Vec::new(),
        }
    }

    fn exhausted(&self) -> bool {
        self.chars >= self.max_chars || self.nodes >= self.max_nodes
    }

    fn push(&mut self, text: &str) {
        let trimmed = text.trim();
        if trimmed.is_empty() || self.exhausted() {
            return;
        }
        let room = self.max_chars.saturating_sub(self.chars);
        if room == 0 {
            return;
        }
        let slice: String = trimmed.chars().take(room).collect();
        self.chars += slice.chars().count();
        self.chunks.push(slice);
    }

    fn finish(self) -> String {
        self.chunks.join("\n")
    }
}

unsafe fn collect_element_text(
    element: AXUIElementRef,
    budget: &mut HarvestBudget,
    depth: usize,
    max_depth: usize,
) {
    if element.is_null() || budget.exhausted() || depth > max_depth {
        return;
    }
    budget.nodes += 1;

    for attr in [
        "AXValue",
        "AXTitle",
        "AXDescription",
        "AXSelectedText",
        "AXPlaceholderValue",
    ] {
        if let Some(text) = copy_string_attr(element, attr) {
            budget.push(&text);
            if budget.exhausted() {
                return;
            }
        }
    }

    let child_cap = if depth == 0 { 40 } else { 20 };
    for child in copy_children(element, child_cap) {
        collect_element_text(child, budget, depth + 1, max_depth);
        CFRelease(child as CFTypeRef);
        if budget.exhausted() {
            return;
        }
    }
}

/// On-screen window titles for priority apps (Slack, Mail, browsers, …).
/// CoreGraphics sees Electron titles (e.g. DM "Laurent Commarieu") even when AX is empty.
fn collect_priority_window_titles() -> String {
    list_priority_windows()
        .into_iter()
        .map(|w| w.title)
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug, Clone)]
pub struct OnScreenWindow {
    pub owner: String,
    pub title: String,
    pub window_id: u32,
}

/// Chat / Electron apps whose AX trees stay empty — OCR these window IDs.
fn is_chat_ocr_app(name: &str) -> bool {
    let lower = name.to_lowercase();
    [
        "slack",
        "discord",
        "microsoft teams",
        "teams",
        "messages",
        "whatsapp",
        "signal",
        "telegram",
        "notion",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

/// Visible priority windows (title + CGWindowID) for titles harvest and chat OCR.
pub fn list_priority_windows() -> Vec<OnScreenWindow> {
    use core_foundation::base::{CFType, TCFType as _};
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;
    use core_graphics::window::{
        copy_window_info, kCGNullWindowID, kCGWindowListExcludeDesktopElements,
        kCGWindowListOptionOnScreenOnly, kCGWindowName, kCGWindowNumber, kCGWindowOwnerName,
    };
    use std::ffi::c_void;

    let Some(windows) = copy_window_info(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID,
    ) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    let mut slack_titles = 0usize;
    for ptr in windows.get_all_values() {
        if ptr.is_null() {
            continue;
        }
        let dict: CFDictionary<CFString, CFType> =
            unsafe { CFDictionary::wrap_under_get_rule(ptr as *const c_void as _) };
        let owner_key = unsafe { CFString::wrap_under_get_rule(kCGWindowOwnerName) };
        let name_key = unsafe { CFString::wrap_under_get_rule(kCGWindowName) };
        let number_key = unsafe { CFString::wrap_under_get_rule(kCGWindowNumber) };
        let owner = dict
            .find(owner_key)
            .and_then(|v| v.downcast::<CFString>())
            .map(|s| s.to_string())
            .unwrap_or_default();
        if owner.is_empty() || !is_priority_context_app(&owner) {
            continue;
        }
        let title = dict
            .find(name_key)
            .and_then(|v| v.downcast::<CFString>())
            .map(|s| s.to_string())
            .unwrap_or_default();
        let title = title.trim().to_string();
        let window_id = dict
            .find(number_key)
            .and_then(|v| v.downcast::<CFNumber>())
            .and_then(|n| n.to_i64())
            .unwrap_or(0) as u32;
        // Chat apps: keep even with empty titles so OCR still works when
        // Screen Recording hides kCGWindowName / Accessibility is denied.
        if title.is_empty() && !is_chat_ocr_app(&owner) {
            continue;
        }
        if window_id == 0 {
            continue;
        }
        if owner.to_lowercase().contains("slack") {
            slack_titles += 1;
            if title.is_empty() {
                log::debug!("[screen_context] Slack CG window_id={window_id} (title empty)");
            } else {
                let preview: String = title.chars().take(80).collect();
                log::debug!("[screen_context] Slack CG title: {preview}");
            }
        }
        out.push(OnScreenWindow {
            owner,
            title,
            window_id,
        });
    }
    if !out.is_empty() {
        log::debug!(
            "[screen_context] CG window titles: {} (slack={})",
            out.len(),
            slack_titles
        );
    }
    out
}

/// Slack/Teams/… window IDs suitable for Vision OCR when AX is empty.
/// Frontmost chat app + Slack are listed first so a background WhatsApp
/// capture does not starve the Slack window the user just looked at.
pub fn list_chat_windows_for_ocr() -> Vec<OnScreenWindow> {
    let focused_name = frontmost_app_name().unwrap_or_default().to_lowercase();
    let mut windows: Vec<OnScreenWindow> = list_priority_windows()
        .into_iter()
        .filter(|w| w.window_id != 0 && is_chat_ocr_app(&w.owner))
        .collect();
    windows.sort_by_key(|w| {
        let owner = w.owner.to_lowercase();
        let is_focused = !focused_name.is_empty()
            && (owner.contains(focused_name.as_str()) || focused_name.contains(owner.as_str()));
        let is_slack = owner.contains("slack");
        let focus_rank: u8 = if is_focused { 0 } else { 1 };
        let slack_rank: u8 = if is_slack { 0 } else { 1 };
        (focus_rank, slack_rank, w.window_id)
    });
    windows
}

/// True when the frontmost app is one we OCR (Slack, Teams, …).
pub fn frontmost_is_chat_ocr_app() -> bool {
    frontmost_app_name().is_some_and(|n| is_chat_ocr_app(&n))
}

/// Compact signature of frontmost app + primary chat window (id + title).
/// Changes when the user alt-tabs or switches Slack channel / DM.
pub fn focus_signature() -> String {
    let app = frontmost_app_name().unwrap_or_default().to_lowercase();
    let primary = list_chat_windows_for_ocr().into_iter().next();
    let (wid, title) = match primary {
        Some(w) => (
            w.window_id,
            w.title.chars().take(120).collect::<String>(),
        ),
        None => (0u32, String::new()),
    };
    format!("{app}|{wid}|{title}")
}

fn frontmost_app_name() -> Option<String> {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};

    unsafe {
        let ws_cls = AnyClass::get("NSWorkspace")?;
        let workspace: *mut AnyObject = msg_send![ws_cls, sharedWorkspace];
        if workspace.is_null() {
            return None;
        }
        let app: *mut AnyObject = msg_send![workspace, frontmostApplication];
        if app.is_null() {
            return None;
        }
        let name: *mut AnyObject = msg_send![app, localizedName];
        if name.is_null() {
            return None;
        }
        let s: *const std::ffi::c_char = msg_send![name, UTF8String];
        if s.is_null() {
            return None;
        }
        Some(std::ffi::CStr::from_ptr(s).to_string_lossy().into_owned())
    }
}

fn is_priority_context_app(name: &str) -> bool {
    let lower = name.to_lowercase();
    [
        "slack",
        "mail",
        "messages",
        "microsoft teams",
        "teams",
        "discord",
        "notion",
        "linear",
        "chrome",
        "safari",
        "arc",
        "firefox",
        "edge",
        "outlook",
        "spark",
        "superhuman",
        "figma",
        "zoom",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

/// IDEs / terminals dump huge AX trees (source, tests, path chrome) and crowd out
/// real context like Slack filenames. Harvest them shallowly only.
fn is_noisy_dev_app(name: &str) -> bool {
    let lower = name.to_lowercase();
    [
        "cursor",
        "code",
        "visual studio",
        "xcode",
        "terminal",
        "iterm",
        "warp",
        "alacritty",
        "kitty",
        "zed",
        "sublime",
        "jetbrains",
        "intellij",
        "webstorm",
        "pycharm",
        "goland",
        "finder",
        "activity monitor",
        "console",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

/// Regular GUI apps: (pid, localized name). Focused app is listed first when known.
fn list_regular_apps(focused_pid: Option<Pid>) -> Vec<(Pid, String)> {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};

    unsafe {
        let Some(ws_cls) = AnyClass::get("NSWorkspace") else {
            return Vec::new();
        };
        let workspace: *mut AnyObject = msg_send![ws_cls, sharedWorkspace];
        if workspace.is_null() {
            return Vec::new();
        }
        let apps: *mut AnyObject = msg_send![workspace, runningApplications];
        if apps.is_null() {
            return Vec::new();
        }
        let count: usize = msg_send![apps, count];
        let mut out: Vec<(Pid, String, bool)> = Vec::new();
        for i in 0..count {
            let app: *mut AnyObject = msg_send![apps, objectAtIndex: i];
            if app.is_null() {
                continue;
            }
            let policy: i64 = msg_send![app, activationPolicy];
            if policy != ACTIVATION_POLICY_REGULAR {
                continue;
            }
            let pid: Pid = msg_send![app, processIdentifier];
            if pid <= 0 {
                continue;
            }
            let name_obj: *mut AnyObject = msg_send![app, localizedName];
            let name = if name_obj.is_null() {
                String::new()
            } else {
                let s: *const i8 = msg_send![name_obj, UTF8String];
                if s.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr(s).to_string_lossy().into_owned()
                }
            };
            let priority = is_priority_context_app(&name);
            out.push((pid, name, priority));
        }

        out.sort_by(|a, b| {
            let a_focus = focused_pid == Some(a.0);
            let b_focus = focused_pid == Some(b.0);
            b_focus
                .cmp(&a_focus)
                .then(b.2.cmp(&a.2))
                .then(a.1.to_lowercase().cmp(&b.1.to_lowercase()))
        });

        out.into_iter().map(|(pid, name, _)| (pid, name)).collect()
    }
}

fn frontmost_app_pid() -> Option<Pid> {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};

    unsafe {
        let ws_cls = AnyClass::get("NSWorkspace")?;
        let workspace: *mut AnyObject = msg_send![ws_cls, sharedWorkspace];
        if workspace.is_null() {
            return None;
        }
        let app: *mut AnyObject = msg_send![workspace, frontmostApplication];
        if app.is_null() {
            return None;
        }
        let pid: Pid = msg_send![app, processIdentifier];
        if pid > 0 {
            Some(pid)
        } else {
            None
        }
    }
}

unsafe fn harvest_focused_ui(
    system: AXUIElementRef,
    budget: &mut HarvestBudget,
    focused_name: &str,
) {
    let Some(focused_app) = copy_attr(system, "AXFocusedApplication") else {
        return;
    };
    // Never ingest IDE/editor buffers — they flood vocab with source identifiers.
    if is_noisy_dev_app(focused_name) {
        if let Some(focused_window) = copy_attr(focused_app as AXUIElementRef, "AXFocusedWindow") {
            if let Some(title) = copy_string_attr(focused_window as AXUIElementRef, "AXTitle") {
                budget.push(&title);
            }
            CFRelease(focused_window);
        }
        CFRelease(focused_app);
        return;
    }
    if let Some(title) = copy_string_attr(focused_app as AXUIElementRef, "AXTitle") {
        budget.push(&title);
    }
    if let Some(focused_window) = copy_attr(focused_app as AXUIElementRef, "AXFocusedWindow") {
        if let Some(title) = copy_string_attr(focused_window as AXUIElementRef, "AXTitle") {
            budget.push(&title);
        }
        collect_element_text(focused_window as AXUIElementRef, budget, 0, 6);
        CFRelease(focused_window);
    }
    if let Some(focused) = copy_attr(focused_app as AXUIElementRef, "AXFocusedUIElement") {
        collect_element_text(focused as AXUIElementRef, budget, 0, 6);
        CFRelease(focused);
    }
    CFRelease(focused_app);
}

/// Force Electron/Chromium apps (Slack, …) to build their full AX tree — same
/// trick VoiceOver-style clients use. Cached per pid for the process lifetime.
fn enable_electron_accessibility(pid: Pid) -> bool {
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};

    static ENABLED: OnceLock<Mutex<HashSet<Pid>>> = OnceLock::new();
    static LAST_ATTEMPT: Mutex<Option<(Pid, Instant)>> = Mutex::new(None);

    let enabled = ENABLED.get_or_init(|| Mutex::new(HashSet::new()));
    if let Ok(set) = enabled.lock() {
        if set.contains(&pid) {
            return true;
        }
    }

    // Avoid hammering the same pid if enable keeps failing.
    if let Ok(last) = LAST_ATTEMPT.lock() {
        if let Some((p, t)) = *last {
            if p == pid && t.elapsed() < Duration::from_secs(30) {
                return false;
            }
        }
    }

    unsafe {
        let app = AXUIElementCreateApplication(pid);
        if app.is_null() {
            return false;
        }

        let try_set = |attr: &str| -> i32 {
            AXUIElementSetAttributeValue(
                app,
                cf_str(attr).as_concrete_TypeRef(),
                kCFBooleanTrue,
            )
        };

        // Modern Electron opt-in (no VoiceOver side effects).
        let mut status = try_set("AXManualAccessibility");
        if status != K_AX_ERROR_SUCCESS
            && (status == K_AX_ERROR_ATTRIBUTE_UNSUPPORTED || status == K_AX_ERROR_FAILURE)
        {
            // Legacy flag used by VoiceOver / some Electron builds.
            status = try_set("AXEnhancedUserInterface");
        }

        CFRelease(app as CFTypeRef);

        if status == K_AX_ERROR_SUCCESS {
            log::info!(
                "[screen_context] enabled Electron accessibility for pid={pid} — settling tree"
            );
            // Tree builds asynchronously over IPC; walk too early → empty chrome.
            std::thread::sleep(Duration::from_millis(450));
            if let Ok(mut set) = enabled.lock() {
                set.insert(pid);
            }
            true
        } else {
            log::debug!(
                "[screen_context] Electron AX enable failed for pid={pid} status={status}"
            );
            if let Ok(mut last) = LAST_ATTEMPT.lock() {
                *last = Some((pid, Instant::now()));
            }
            false
        }
    }
}

fn is_electron_like_app(name: &str) -> bool {
    let lower = name.to_lowercase();
    [
        "slack",
        "discord",
        "microsoft teams",
        "teams",
        "notion",
        "figma",
        "chrome",
        "arc",
        "edge",
        "brave",
        "whatsapp",
        "signal",
        "telegram",
        "linear",
        "zoom",
        "code",
        "cursor",
        "spotify",
    ]
    .iter()
    .any(|n| lower.contains(n))
}

unsafe fn harvest_app_windows(
    pid: Pid,
    app_name: &str,
    budget: &mut HarvestBudget,
    windows_cap: CFIndex,
    deep: bool,
) {
    if is_electron_like_app(app_name) {
        // Drop the unsafe context briefly — enable sleeps.
        enable_electron_accessibility(pid);
    }
    let app = AXUIElementCreateApplication(pid);
    if app.is_null() {
        return;
    }
    // Do NOT push process display names ("Slack", "Chrome") — they pollute vocab.
    let max_depth = if deep { 8 } else { 4 };
    for window in copy_windows(app, windows_cap) {
        if budget.exhausted() {
            CFRelease(window as CFTypeRef);
            break;
        }
        if let Some(title) = copy_string_attr(window, "AXTitle") {
            budget.push(&title);
        }
        collect_element_text(window, budget, 0, max_depth);
        CFRelease(window as CFTypeRef);
    }
    CFRelease(app as CFTypeRef);
}

/// Visible context across displays: CG window titles (Electron-safe) + AX trees.
pub fn read_frontmost_context_text(max_chars: usize) -> String {
    let mut budget = HarvestBudget::new(max_chars);

    // 0) Window titles via CoreGraphics — Slack DM titles often hold the person name
    // even when Electron exposes no AX message text.
    let titles = collect_priority_window_titles();
    if !titles.is_empty() {
        budget.push(&titles);
    }

    unsafe {
        if !AXIsProcessTrusted() {
            log::warn!(
                "[screen_context] Accessibility not trusted — AX harvest skipped ({} title chars). Enable GladiaFlow in System Settings → Privacy → Accessibility (re-check after each ad-hoc rebuild). Chat OCR may still run if Screen Recording is allowed.",
                budget.chars
            );
            return budget.finish();
        }

        let system = AXUIElementCreateSystemWide();
        if system.is_null() {
            return budget.finish();
        }

        let focused_pid = frontmost_app_pid();
        let apps = list_regular_apps(focused_pid);
        let focused_name = apps
            .iter()
            .find(|(pid, _)| Some(*pid) == focused_pid)
            .map(|(_, n)| n.clone())
            .unwrap_or_default();

        let mut harvested_apps = 0usize;

        // 1) Priority apps AX (may be empty for Electron).
        for (pid, name) in &apps {
            if !is_priority_context_app(name) {
                continue;
            }
            if *pid == std::process::id() as Pid {
                continue;
            }
            if budget.chars >= (max_chars * 3) / 4 {
                break;
            }
            let before = budget.chars;
            harvest_app_windows(*pid, name, &mut budget, 4, true);
            if budget.chars > before {
                harvested_apps += 1;
                log::info!(
                    "[screen_context] AX priority '{}' pid={} (+{} chars)",
                    name,
                    pid,
                    budget.chars - before
                );
            } else {
                log::debug!(
                    "[screen_context] AX priority '{}' pid={} contributed 0 chars",
                    name,
                    pid
                );
            }
        }

        // 2) Focused UI (title-only when IDE/terminal — never source buffers).
        let before_focus = budget.chars;
        let focus_cap = if is_noisy_dev_app(&focused_name) {
            400
        } else {
            4_000
        };
        let mut focus_budget =
            HarvestBudget::new(focus_cap.min(max_chars.saturating_sub(budget.chars)));
        harvest_focused_ui(system, &mut focus_budget, &focused_name);
        let focus_text = focus_budget.finish();
        if !focus_text.is_empty() {
            budget.push(&focus_text);
            log::debug!(
                "[screen_context] AX focused '{}' (+{} chars)",
                focused_name,
                budget.chars - before_focus
            );
        }

        CFRelease(system as CFTypeRef);
        log::info!(
            "[screen_context] harvest done: {} chars, {} priority AX apps (focused={})",
            budget.chars,
            harvested_apps,
            focused_name
        );
        budget.finish()
    }
}
