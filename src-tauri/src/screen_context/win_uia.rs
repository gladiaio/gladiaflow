//! Windows UI Automation + window-title harvest (Accessibility analogue).
//!
//! Electron/Slack often expose little UIA text — pair with [`super::win_ocr`].

#![cfg(target_os = "windows")]

use std::sync::atomic::{AtomicUsize, Ordering};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, MAX_PATH};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTreeWalker,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible,
};

static COM_READY: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone)]
pub struct OnScreenWindow {
    pub owner: String,
    pub title: String,
    pub hwnd: isize,
}

pub(super) fn ensure_com() {
    if COM_READY.load(Ordering::SeqCst) != 0 {
        return;
    }
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }
    COM_READY.store(1, Ordering::SeqCst);
}

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

fn is_priority_context_app(name: &str) -> bool {
    let lower = name.to_lowercase();
    [
        "slack",
        "mail",
        "outlook",
        "messages",
        "teams",
        "discord",
        "notion",
        "linear",
        "chrome",
        "msedge",
        "edge",
        "firefox",
        "brave",
        "opera",
        "figma",
        "zoom",
        "whatsapp",
        "telegram",
        "signal",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn is_noisy_dev_app(name: &str) -> bool {
    let lower = name.to_lowercase();
    [
        "cursor",
        "code",
        "devenv",
        "visual studio",
        "windows terminal",
        "cmd.exe",
        "powershell",
        "wt.exe",
        "windowsterminal",
        "idea64",
        "pycharm",
        "goland",
        "webstorm",
        "sublime",
        "notepad++",
        "taskmgr",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn hwnd_title(hwnd: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; (len as usize) + 1];
        let n = GetWindowTextW(hwnd, &mut buf);
        if n <= 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..n as usize])
    }
}

fn process_name_for_hwnd(hwnd: HWND) -> String {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return String::new();
        }
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let mut buf = [0u16; MAX_PATH as usize];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut size,
        );
        let _ = windows::Win32::Foundation::CloseHandle(handle);
        if ok.is_err() || size == 0 {
            return String::new();
        }
        let path = String::from_utf16_lossy(&buf[..size as usize]);
        path.rsplit(['\\', '/'])
            .next()
            .unwrap_or(&path)
            .trim()
            .to_string()
    }
}

fn is_top_level_candidate(hwnd: HWND) -> bool {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return false;
        }
        let mut class = [0u16; 64];
        let n = GetClassNameW(hwnd, &mut class);
        if n > 0 {
            let class_name = String::from_utf16_lossy(&class[..n as usize]);
            if class_name == "Shell_TrayWnd"
                || class_name == "Progman"
                || class_name == "WorkerW"
            {
                return false;
            }
        }
        true
    }
}

struct EnumState {
    out: Vec<OnScreenWindow>,
}

unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let state = &mut *(lparam.0 as *mut EnumState);
    if !is_top_level_candidate(hwnd) {
        return BOOL(1);
    }
    let title = hwnd_title(hwnd);
    let owner = process_name_for_hwnd(hwnd);
    if owner.is_empty() {
        return BOOL(1);
    }
    if !is_priority_context_app(&owner) && !is_chat_ocr_app(&owner) {
        return BOOL(1);
    }
    if title.is_empty() && !is_chat_ocr_app(&owner) {
        return BOOL(1);
    }
    state.out.push(OnScreenWindow {
        owner,
        title,
        hwnd: hwnd.0 as isize,
    });
    BOOL(1)
}

pub fn list_priority_windows() -> Vec<OnScreenWindow> {
    ensure_com();
    let mut state = EnumState { out: Vec::new() };
    unsafe {
        let _ = EnumWindows(
            Some(enum_windows_proc),
            LPARAM(&mut state as *mut _ as isize),
        );
    }
    state.out
}

pub fn list_chat_windows_for_ocr() -> Vec<OnScreenWindow> {
    let focused = foreground_process_name().to_lowercase();
    let mut windows: Vec<OnScreenWindow> = list_priority_windows()
        .into_iter()
        .filter(|w| w.hwnd != 0 && is_chat_ocr_app(&w.owner))
        .collect();
    windows.sort_by_key(|w| {
        let owner = w.owner.to_lowercase();
        let is_focused = !focused.is_empty()
            && (owner.contains(focused.as_str()) || focused.contains(owner.as_str()));
        let is_slack = owner.contains("slack");
        let focus_rank: u8 = if is_focused { 0 } else { 1 };
        let slack_rank: u8 = if is_slack { 0 } else { 1 };
        (focus_rank, slack_rank, w.hwnd as u32)
    });
    windows
}

fn foreground_hwnd() -> HWND {
    unsafe { GetForegroundWindow() }
}

fn foreground_process_name() -> String {
    let hwnd = foreground_hwnd();
    if hwnd.0.is_null() {
        return String::new();
    }
    process_name_for_hwnd(hwnd)
}

pub fn frontmost_is_chat_ocr_app() -> bool {
    is_chat_ocr_app(&foreground_process_name())
}

pub fn focus_signature() -> String {
    let app = foreground_process_name().to_lowercase();
    let hwnd = foreground_hwnd();
    let title = if !hwnd.0.is_null() {
        hwnd_title(hwnd).chars().take(120).collect::<String>()
    } else {
        String::new()
    };
    let primary = list_chat_windows_for_ocr().into_iter().next();
    let chat_hwnd = primary.map(|w| w.hwnd).unwrap_or(0);
    format!("{app}|{chat_hwnd}|{title}")
}

fn uia_automation() -> Option<IUIAutomation> {
    ensure_com();
    unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok() }
}

fn element_name(el: &IUIAutomationElement) -> Option<String> {
    unsafe {
        let name = el.CurrentName().ok()?;
        let s = name.to_string();
        let t = s.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    }
}

fn collect_names(
    walker: &IUIAutomationTreeWalker,
    root: &IUIAutomationElement,
    depth: usize,
    max_depth: usize,
    budget: &mut usize,
    out: &mut String,
) {
    if depth > max_depth || *budget == 0 {
        return;
    }
    if let Some(name) = element_name(root) {
        if name.len() >= 2 && *budget > 0 {
            if !out.is_empty() {
                out.push('\n');
            }
            let take = name.len().min(*budget);
            out.push_str(&name[..take]);
            *budget = budget.saturating_sub(take + 1);
        }
    }
    unsafe {
        let Ok(mut child) = walker.GetFirstChildElement(root) else {
            return;
        };
        while *budget > 0 {
            collect_names(walker, &child, depth + 1, max_depth, budget, out);
            match walker.GetNextSiblingElement(&child) {
                Ok(next) => child = next,
                Err(_) => break,
            }
        }
    }
}

/// Harvest priority window titles + shallow UIA Name tree from the foreground window.
pub fn read_frontmost_context_text(max_chars: usize) -> String {
    ensure_com();
    let mut out = String::new();
    let mut budget = max_chars;

    for w in list_priority_windows() {
        if w.title.is_empty() || budget == 0 {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        let take = w.title.len().min(budget);
        out.push_str(&w.title[..take]);
        budget = budget.saturating_sub(take + 1);
    }

    let focused_name = foreground_process_name();
    if is_noisy_dev_app(&focused_name) {
        log::info!(
            "[screen_context] harvest done: {} chars, UIA skipped (focused={})",
            out.len(),
            focused_name
        );
        return out;
    }

    let Some(automation) = uia_automation() else {
        log::warn!("[screen_context] UIAutomation unavailable");
        return out;
    };

    let hwnd = foreground_hwnd();
    if hwnd.0.is_null() {
        return out;
    }

    unsafe {
        let Ok(root) = automation.ElementFromHandle(hwnd) else {
            return out;
        };
        let Ok(walker) = automation.ControlViewWalker() else {
            return out;
        };
        let max_depth = if is_chat_ocr_app(&focused_name) { 4 } else { 6 };
        let before = out.len();
        collect_names(&walker, &root, 0, max_depth, &mut budget, &mut out);
        log::info!(
            "[screen_context] harvest done: {} chars (+{} UIA), focused={}",
            out.len(),
            out.len().saturating_sub(before),
            focused_name
        );
    }
    out
}
