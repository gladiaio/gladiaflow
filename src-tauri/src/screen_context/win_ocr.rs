//! Windows chat-window OCR via GDI capture + WinRT `Windows.Media.Ocr`.
//!
//! Used when UI Automation text is thin (Slack / Electron), mirroring macOS Vision.

#![cfg(target_os = "windows")]

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use windows::core::HSTRING;
use windows::Graphics::Imaging::{BitmapDecoder, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::{
    DataWriter, InMemoryRandomAccessStream, RandomAccessStreamReference,
};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
};
use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

const CHAT_OCR_MIN_INTERVAL: Duration = Duration::from_secs(20);

static LAST_OCR_BY_HWND: Mutex<Option<HashMap<isize, Instant>>> = Mutex::new(None);

fn last_map(guard: &mut Option<HashMap<isize, Instant>>) -> &mut HashMap<isize, Instant> {
    guard.get_or_insert_with(HashMap::new)
}

pub fn recognize_windows(hwnds: &[isize], max_chars: usize) -> String {
    recognize_windows_inner(hwnds, max_chars, false)
}

pub fn recognize_windows_forced(hwnds: &[isize], max_chars: usize) -> String {
    recognize_windows_inner(hwnds, max_chars, true)
}

fn recognize_windows_inner(hwnds: &[isize], max_chars: usize, force: bool) -> String {
    if hwnds.is_empty() || max_chars == 0 {
        return String::new();
    }

    let mut out = String::new();
    let mut attempted = 0usize;
    let mut skipped = 0usize;

    for &hwnd in hwnds {
        if out.len() >= max_chars {
            break;
        }
        if !force {
            if let Ok(mut slot) = LAST_OCR_BY_HWND.lock() {
                let map = last_map(&mut slot);
                if let Some(t) = map.get(&hwnd) {
                    if t.elapsed() < CHAT_OCR_MIN_INTERVAL {
                        skipped += 1;
                        continue;
                    }
                }
            }
        }
        attempted += 1;
        let remaining = max_chars.saturating_sub(out.len());
        let chunk = recognize_hwnd(HWND(hwnd as *mut _), remaining);
        if let Ok(mut slot) = LAST_OCR_BY_HWND.lock() {
            last_map(&mut slot).insert(hwnd, Instant::now());
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
            "[screen_context.ocr] Windows OCR empty (attempted={attempted}, throttled={skipped})"
        );
    }
    out
}

fn recognize_hwnd(hwnd: HWND, max_chars: usize) -> String {
    // WinRT OCR needs COM/WinRT apartment (shared with UIA).
    super::win_uia::ensure_com();
    let Some((bgra, width, height)) = capture_window_bgra(hwnd) else {
        return String::new();
    };
    if width == 0 || height == 0 || bgra.is_empty() {
        return String::new();
    }
    match ocr_bgra_bmp(&bgra, width, height) {
        Ok(mut text) => {
            if text.len() > max_chars {
                text.truncate(max_chars);
            }
            if !text.is_empty() {
                log::info!(
                    "[screen_context.ocr] hwnd={:?} → {} char(s)",
                    hwnd.0,
                    text.len()
                );
            }
            text
        }
        Err(err) => {
            log::debug!("[screen_context.ocr] OCR failed: {err}");
            String::new()
        }
    }
}

fn capture_window_bgra(hwnd: HWND) -> Option<(Vec<u8>, i32, i32)> {
    unsafe {
        let mut rect = RECT::default();
        if GetClientRect(hwnd, &mut rect).is_err() {
            return None;
        }
        let width = (rect.right - rect.left).clamp(1, 4_000);
        let height = (rect.bottom - rect.top).clamp(1, 4_000);
        if width < 32 || height < 32 {
            return None;
        }

        let window_dc = GetDC(hwnd);
        if window_dc.is_invalid() {
            return None;
        }
        let mem_dc = CreateCompatibleDC(window_dc);
        if mem_dc.is_invalid() {
            ReleaseDC(hwnd, window_dc);
            return None;
        }
        let bitmap = CreateCompatibleBitmap(window_dc, width, height);
        if bitmap.is_invalid() {
            let _ = DeleteDC(mem_dc);
            ReleaseDC(hwnd, window_dc);
            return None;
        }
        let old = SelectObject(mem_dc, bitmap);
        let printed = PrintWindow(hwnd, mem_dc, PRINT_WINDOW_FLAGS(2)).as_bool();
        if !printed {
            let _ = BitBlt(mem_dc, 0, 0, width, height, window_dc, 0, 0, SRCCOPY);
        }

        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0 as u32,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        let rows = GetDIBits(
            mem_dc,
            bitmap,
            0,
            height as u32,
            Some(pixels.as_mut_ptr() as *mut _),
            &mut info,
            DIB_RGB_COLORS,
        );

        SelectObject(mem_dc, old);
        let _ = DeleteObject(bitmap);
        let _ = DeleteDC(mem_dc);
        ReleaseDC(hwnd, window_dc);

        if rows == 0 {
            return None;
        }
        Some((pixels, width, height))
    }
}

/// Encode BGRA as a BMP in memory, decode via WinRT, then OCR.
fn ocr_bgra_bmp(bgra: &[u8], width: i32, height: i32) -> windows::core::Result<String> {
    let bmp = encode_bmp_bgra(bgra, width, height);
    let stream = InMemoryRandomAccessStream::new()?;
    let writer = DataWriter::CreateDataWriter(&stream)?;
    writer.WriteBytes(&bmp)?;
    writer.StoreAsync()?.get()?;
    writer.FlushAsync()?.get()?;
    drop(writer);
    stream.Seek(0)?;

    let decoder = BitmapDecoder::CreateAsync(&stream)?.get()?;
    let software: SoftwareBitmap = decoder.GetSoftwareBitmapAsync()?.get()?;
    let engine = match OcrEngine::TryCreateFromUserProfileLanguages() {
        Ok(e) => e,
        Err(_) => {
            // Fallback English if profile languages missing.
            let lang = windows::Globalization::Language::CreateLanguage(&HSTRING::from("en"))?;
            OcrEngine::TryCreateFromLanguage(&lang)?
        }
    };
    let result = engine.RecognizeAsync(&software)?.get()?;
    let _ = RandomAccessStreamReference::CreateFromStream(&stream);
    Ok(result.Text()?.to_string())
}

fn encode_bmp_bgra(bgra: &[u8], width: i32, height: i32) -> Vec<u8> {
    // 32-bit BI_RGB BMP (top-down).
    let pixel_size = (width * height * 4) as usize;
    let file_size = 14 + 40 + pixel_size;
    let mut out = Vec::with_capacity(file_size);
    // BITMAPFILEHEADER
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(file_size as u32).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    // BITMAPINFOHEADER
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&(-height).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&(pixel_size as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&bgra[..pixel_size.min(bgra.len())]);
    // Pad if needed (shouldn't for BGRA stride = width*4).
    if out.len() < file_size {
        out.resize(file_size, 0);
    }
    out
}
