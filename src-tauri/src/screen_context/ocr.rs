//! Screen OCR via Apple Vision (Electron / chat-window fallback).
//!
//! Chat windows (Slack, …) are OCR'd when Accessibility text is empty.
//! Full-display OCR stays behind the opt-in setting. Needs Screen Recording.

#![cfg(target_os = "macos")]

use core_graphics::display::CGDisplay;
use core_graphics::geometry::CGRect;
use core_graphics::image::CGImage;
use core_graphics::window::{
    create_image, kCGWindowImageBoundsIgnoreFraming, kCGWindowListOptionIncludingWindow,
};
use foreign_types::ForeignType;
use objc2::encode::{Encode, Encoding};
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{class, msg_send};
use std::ffi::c_void;
use std::ptr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[link(name = "Vision", kind = "framework")]
extern "C" {}

/// Opaque CGImageRef for objc2 encoding (`^{CGImage=}`).
#[repr(transparent)]
#[derive(Clone, Copy)]
struct CgImageRef(*mut c_void);

unsafe impl Encode for CgImageRef {
    const ENCODING: Encoding = Encoding::Pointer(&Encoding::Struct("CGImage", &[]));
}
/// Fast recognition is enough for proper nouns and keeps the sampler light.
/// VNRequestTextRecognitionLevel is NSInteger (signed).
const VN_REQUEST_TEXT_RECOGNITION_LEVEL_FAST: i64 = 1;

/// Don't hammer Vision on the same window every 8s sampler tick.
const CHAT_OCR_MIN_INTERVAL: Duration = Duration::from_secs(20);

/// Per-window last OCR time — a WhatsApp capture must not block Slack.
static LAST_CHAT_OCR_BY_WINDOW: Mutex<Option<std::collections::HashMap<u32, Instant>>> =
    Mutex::new(None);

fn last_ocr_map(
    guard: &mut Option<std::collections::HashMap<u32, Instant>>,
) -> &mut std::collections::HashMap<u32, Instant> {
    guard.get_or_insert_with(std::collections::HashMap::new)
}

/// Capture + OCR specific on-screen windows (Slack body text, etc.).
///
/// `force` ignores the per-window throttle (used right before dictation so a
/// name shown ~seconds ago is still harvested).
pub fn recognize_windows(window_ids: &[u32], max_chars: usize) -> String {
    recognize_windows_inner(window_ids, max_chars, false)
}

pub fn recognize_windows_forced(window_ids: &[u32], max_chars: usize) -> String {
    recognize_windows_inner(window_ids, max_chars, true)
}

fn recognize_windows_inner(window_ids: &[u32], max_chars: usize, force: bool) -> String {
    if window_ids.is_empty() || max_chars == 0 {
        return String::new();
    }

    let mut out = String::new();
    let mut attempted = 0usize;
    let mut skipped_throttle = 0usize;
    for &id in window_ids {
        if out.len() >= max_chars {
            break;
        }
        if !force {
            if let Ok(mut slot) = LAST_CHAT_OCR_BY_WINDOW.lock() {
                let map = last_ocr_map(&mut slot);
                if let Some(t) = map.get(&id) {
                    if t.elapsed() < CHAT_OCR_MIN_INTERVAL {
                        skipped_throttle += 1;
                        continue;
                    }
                }
            }
        }
        attempted += 1;
        let remaining = max_chars.saturating_sub(out.len());
        let chunk = recognize_window(id, remaining);
        if let Ok(mut slot) = LAST_CHAT_OCR_BY_WINDOW.lock() {
            last_ocr_map(&mut slot).insert(id, Instant::now());
        }
        if chunk.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&chunk);
    }

    if out.is_empty() && attempted > 0 {
        log::info!(
            "[screen_context.ocr] chat window OCR returned empty (attempted={attempted}, throttled={skipped_throttle}) — grant Screen Recording if denied"
        );
    } else if out.is_empty() && skipped_throttle > 0 {
        log::debug!(
            "[screen_context.ocr] skipped {skipped_throttle} chat window(s) (per-window throttle)"
        );
    }
    out
}

fn recognize_window(window_id: u32, max_chars: usize) -> String {
    // Null rect + IncludingWindow → capture that window's frame (Apple docs).
    let null_bounds = unsafe { std::mem::zeroed::<CGRect>() };
    let Some(image) = create_image(
        null_bounds,
        kCGWindowListOptionIncludingWindow,
        window_id,
        kCGWindowImageBoundsIgnoreFraming,
    ) else {
        log::debug!("[screen_context.ocr] CGWindowListCreateImage null for window_id={window_id}");
        return String::new();
    };
    let text = unsafe { recognize_cg_image(&image, max_chars) };
    if !text.is_empty() {
        log::info!(
            "[screen_context.ocr] window_id={window_id} → {} char(s)",
            text.len()
        );
    }
    text
}

/// Capture + OCR active displays (opt-in full-screen path).
pub fn recognize_screen_text(max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let ids = match CGDisplay::active_displays() {
        Ok(ids) => ids,
        Err(err) => {
            log::debug!("[screen_context.ocr] active_displays failed: {err:?}");
            return String::new();
        }
    };
    let mut out = String::new();
    for id in ids {
        if out.len() >= max_chars {
            break;
        }
        let remaining = max_chars.saturating_sub(out.len());
        let display = CGDisplay::new(id);
        let Some(image) = display.image() else {
            continue;
        };
        let chunk = unsafe { recognize_cg_image(&image, remaining) };
        if chunk.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&chunk);
    }
    if out.len() > max_chars {
        out.truncate(max_chars);
    }
    out
}

unsafe fn recognize_cg_image(image: &CGImage, max_chars: usize) -> String {
    let cg_image = CgImageRef(image.as_ptr() as *mut c_void);
    if cg_image.0.is_null() {
        return String::new();
    }

    let Some(request_cls) = AnyClass::get("VNRecognizeTextRequest") else {
        log::warn!("[screen_context.ocr] VNRecognizeTextRequest class missing");
        return String::new();
    };
    let Some(handler_cls) = AnyClass::get("VNImageRequestHandler") else {
        log::warn!("[screen_context.ocr] VNImageRequestHandler class missing");
        return String::new();
    };

    let request: *mut AnyObject = msg_send![request_cls, new];
    if request.is_null() {
        return String::new();
    }
    let _: () = msg_send![request, setRecognitionLevel: VN_REQUEST_TEXT_RECOGNITION_LEVEL_FAST];
    let _: () = msg_send![
        request,
        setUsesLanguageCorrection: objc2::runtime::Bool::from(false)
    ];

    let options: *mut AnyObject = msg_send![class!(NSDictionary), dictionary];
    let handler: *mut AnyObject = msg_send![handler_cls, alloc];
    let handler: *mut AnyObject =
        msg_send![handler, initWithCGImage: cg_image options: options];
    if handler.is_null() {
        let _: () = msg_send![request, release];
        return String::new();
    }

    let requests: *mut AnyObject = msg_send![class!(NSArray), arrayWithObject: request];
    let mut error: *mut AnyObject = ptr::null_mut();
    let ok: objc2::runtime::Bool =
        msg_send![handler, performRequests: requests error: &mut error];
    if !bool::from(ok) {
        log::debug!("[screen_context.ocr] performRequests failed");
        let _: () = msg_send![handler, release];
        let _: () = msg_send![request, release];
        return String::new();
    }

    let results: *mut AnyObject = msg_send![request, results];
    let mut out = String::new();
    if !results.is_null() {
        let count: usize = msg_send![results, count];
        for i in 0..count {
            if out.len() >= max_chars {
                break;
            }
            let obs: *mut AnyObject = msg_send![results, objectAtIndex: i];
            if obs.is_null() {
                continue;
            }
            let candidates: *mut AnyObject = msg_send![obs, topCandidates: 1usize];
            if candidates.is_null() {
                continue;
            }
            let cand_count: usize = msg_send![candidates, count];
            if cand_count == 0 {
                continue;
            }
            let top: *mut AnyObject = msg_send![candidates, objectAtIndex: 0usize];
            if top.is_null() {
                continue;
            }
            let s_obj: *mut AnyObject = msg_send![top, string];
            if s_obj.is_null() {
                continue;
            }
            let utf8: *const i8 = msg_send![s_obj, UTF8String];
            if utf8.is_null() {
                continue;
            }
            let piece = std::ffi::CStr::from_ptr(utf8).to_string_lossy();
            if piece.trim().is_empty() {
                continue;
            }
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&piece);
        }
    }

    let _: () = msg_send![handler, release];
    let _: () = msg_send![request, release];

    if out.len() > max_chars {
        out.truncate(max_chars);
    }
    out
}
