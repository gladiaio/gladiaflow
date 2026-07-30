//! Resolves logical (layout) key tokens to physical `keyboard_types::Code` values
//! for the active keyboard layout.

use tauri_plugin_global_shortcut::Code;

/// Map a persisted logical key token to a physical `Code` for global shortcut registration.
pub fn resolve_logical_key(key: &str) -> Result<Code, String> {
    if let Some(code) = stable_logical_to_code(key) {
        log::info!(
            "[hotkey-layout] resolved stable logical key {key:?} -> physical {code:?}"
        );
        return Ok(code);
    }

    let mut chars = key.chars();
    let ch = chars
        .next()
        .ok_or_else(|| "Empty shortcut key in hotkey".to_string())?;
    if chars.next().is_some() {
        return Err(format!(
            "Unsupported shortcut key \"{key}\". Choose a single letter, number, or symbol key."
        ));
    }

    let code = resolve_char_in_current_layout(ch)?;
    log::info!(
        "[hotkey-layout] resolved layout logical key {key:?} (char={ch:?}) -> physical {code:?}"
    );
    Ok(code)
}

fn stable_logical_to_code(key: &str) -> Option<Code> {
    use Code::*;
    match key.to_uppercase().as_str() {
        "SPACE" => Some(Space),
        "ENTER" | "RETURN" => Some(Enter),
        "TAB" => Some(Tab),
        "UP" => Some(ArrowUp),
        "DOWN" => Some(ArrowDown),
        "LEFT" => Some(ArrowLeft),
        "RIGHT" => Some(ArrowRight),
        "BACKSPACE" => Some(Backspace),
        "DELETE" => Some(Delete),
        "ESCAPE" | "ESC" => Some(Escape),
        "F1" => Some(F1),
        "F2" => Some(F2),
        "F3" => Some(F3),
        "F4" => Some(F4),
        "F5" => Some(F5),
        "F6" => Some(F6),
        "F7" => Some(F7),
        "F8" => Some(F8),
        "F9" => Some(F9),
        "F10" => Some(F10),
        "F11" => Some(F11),
        "F12" => Some(F12),
        _ => None,
    }
}

#[cfg(all(target_os = "macos", not(test)))]
fn chars_match(expected: char, produced: char) -> bool {
    if expected.is_ascii_alphabetic() && produced.is_ascii_alphabetic() {
        expected.eq_ignore_ascii_case(&produced)
    } else {
        expected == produced
    }
}

#[cfg(all(target_os = "macos", not(test)))]
mod macos_layout {
    use super::{chars_match, scancode_to_code};
    use std::ffi::c_void;
    use tauri_plugin_global_shortcut::Code;

    #[link(name = "Carbon", kind = "framework")]
    extern "C" {
        fn LMGetKbdType() -> u8;
        fn TISCopyCurrentKeyboardLayoutInputSource() -> *const c_void;
        fn TISGetInputSourceProperty(
            input_source: *const c_void,
            property_key: *const c_void,
        ) -> *const c_void;
        fn CFRelease(cf: *const c_void);
        fn CFDataGetBytePtr(data: *const c_void) -> *const u8;
        fn UCKeyTranslate(
            key_layout_ptr: *const u8,
            virtual_key_code: u16,
            key_action: u16,
            modifier_key_state: u32,
            keyboard_type: u32,
            key_translate_options: u32,
            dead_key_state: *mut u32,
            max_string_length: u32,
            actual_string_length: *mut u32,
            unicode_string: *mut u16,
        ) -> i32;
        static kTISPropertyUnicodeKeyLayoutData: *const c_void;
    }

    const K_UC_KEY_ACTION_DISPLAY: u16 = 3;
    const K_UC_KEY_TRANSLATE_NO_DEAD_KEYS_MASK: u32 = 1 << 16;
    const SHIFT_MODIFIER_STATE: u32 = 1 << 17;

    /// RAII guard for CoreFoundation objects returned with a +1 retain count.
    struct CfOwned(*const c_void);

    impl CfOwned {
        fn new(ptr: *const c_void) -> Option<Self> {
            if ptr.is_null() {
                None
            } else {
                Some(Self(ptr))
            }
        }

        fn as_ptr(&self) -> *const c_void {
            self.0
        }
    }

    impl Drop for CfOwned {
        fn drop(&mut self) {
            unsafe { CFRelease(self.0) };
        }
    }

    extern "C" {
        fn pthread_main_np() -> std::os::raw::c_int;
        static _dispatch_main_q: c_void;
        fn dispatch_sync_f(
            queue: *const c_void,
            context: *mut c_void,
            work: extern "C" fn(*mut c_void),
        );
    }

    /// Resolve `ch` against the active keyboard layout, on the main thread.
    ///
    /// The Carbon Text Input Source APIs (`TISCopyCurrentKeyboardLayoutInputSource`
    /// / `TISGetInputSourceProperty`) call `dispatch_assert_queue(main)` internally
    /// and `SIGTRAP` when invoked off the main thread. Tauri command handlers run on
    /// Tokio worker threads, so the lookup is marshalled onto the main thread.
    pub fn resolve_char(ch: char) -> Result<Code, String> {
        run_on_main_sync(move || resolve_char_on_main(ch))
    }

    /// Performs the keyboard-layout lookup. Must run on the main thread.
    ///
    /// `TISGetInputSourceProperty` returns a non-owned ("Get") reference whose
    /// lifetime is tied to the retained input source, so the `CfOwned` source must
    /// stay alive for the entire translation loop.
    fn resolve_char_on_main(ch: char) -> Result<Code, String> {
        let source = CfOwned::new(unsafe { TISCopyCurrentKeyboardLayoutInputSource() })
            .ok_or_else(|| "Could not read the current keyboard layout.".to_string())?;

        let layout_data = unsafe {
            TISGetInputSourceProperty(source.as_ptr(), kTISPropertyUnicodeKeyLayoutData)
        };
        if layout_data.is_null() {
            return Err("Could not read keyboard layout data.".into());
        }

        let layout_ptr = unsafe { CFDataGetBytePtr(layout_data) };
        if layout_ptr.is_null() {
            return Err("Keyboard layout data was empty.".into());
        }

        let keyboard_type = unsafe { LMGetKbdType() } as u32;
        translate_char_in_layout(layout_ptr, keyboard_type, ch)
    }

    /// Runs `work` on the main thread and blocks until it returns its value.
    ///
    /// Falls back to running inline when already on the main thread to avoid a
    /// `dispatch_sync` self-deadlock.
    fn run_on_main_sync<T>(work: impl FnOnce() -> T) -> T {
        if unsafe { pthread_main_np() } != 0 {
            return work();
        }

        let mut result: Option<T> = None;
        let result_ptr: *mut Option<T> = &mut result;

        // `dispatch_sync_f` blocks until the work runs, so the borrow of `result`
        // through the raw pointer never outlives this stack frame.
        dispatch_closure(move || {
            let value = work();
            unsafe { *result_ptr = Some(value) };
        });

        result.expect("main-thread work did not run")
    }

    /// Submits `closure` to the main queue via `dispatch_sync_f`.
    ///
    /// The trampoline is monomorphised to the concrete closure type so the boxed
    /// closure can be passed through a thin context pointer (no `dyn`).
    fn dispatch_closure<F: FnOnce()>(closure: F) {
        extern "C" fn trampoline<F: FnOnce()>(ctx: *mut c_void) {
            let closure: Box<F> = unsafe { Box::from_raw(ctx as *mut F) };
            closure();
        }

        let ctx = Box::into_raw(Box::new(closure)) as *mut c_void;
        unsafe {
            dispatch_sync_f(core::ptr::addr_of!(_dispatch_main_q), ctx, trampoline::<F>);
        }
    }

    fn translate_char_in_layout(
        layout_ptr: *const u8,
        keyboard_type: u32,
        ch: char,
    ) -> Result<Code, String> {
        let mut resolved: Option<Code> = None;
        'search: for vk in 0u16..128 {
            for &shift_state in &[0u32, SHIFT_MODIFIER_STATE] {
                let mut dead_key_state = 0u32;
                let mut actual_len = 0u32;
                let mut unicode = [0u16; 8];

                let status = unsafe {
                    UCKeyTranslate(
                        layout_ptr,
                        vk,
                        K_UC_KEY_ACTION_DISPLAY,
                        shift_state,
                        keyboard_type,
                        K_UC_KEY_TRANSLATE_NO_DEAD_KEYS_MASK,
                        &mut dead_key_state,
                        unicode.len() as u32,
                        &mut actual_len,
                        unicode.as_mut_ptr(),
                    )
                };

                if status != 0 || actual_len == 0 {
                    continue;
                }

                let produced = String::from_utf16_lossy(
                    &unicode[..actual_len.min(unicode.len() as u32) as usize],
                );
                let Some(produced_ch) = produced.chars().next() else {
                    continue;
                };

                if chars_match(ch, produced_ch) {
                    if let Some(code) = scancode_to_code(vk) {
                        resolved = Some(code);
                        break 'search;
                    }
                }
            }
        }

        resolved.ok_or_else(|| {
            format!(
                "The key \"{ch}\" is not available on the current keyboard layout. \
                 Re-save the shortcut or choose a different key."
            )
        })
    }
}

// In test builds the resolver is routed to the deterministic US-QWERTY fallback
// below: the libtest harness owns the main thread but never drains the dispatch
// main queue, so the real macOS TIS lookup (which marshals onto the main thread)
// would deadlock.
#[cfg(test)]
fn resolve_char_in_current_layout(ch: char) -> Result<Code, String> {
    fallback_us_qwerty_char_to_code(ch).ok_or_else(|| {
        format!("The key \"{ch}\" is not available on the current keyboard layout.")
    })
}

#[cfg(all(target_os = "macos", not(test)))]
fn resolve_char_in_current_layout(ch: char) -> Result<Code, String> {
    macos_layout::resolve_char(ch)
}

#[cfg(all(target_os = "windows", not(test)))]
fn resolve_char_in_current_layout(ch: char) -> Result<Code, String> {
    #[link(name = "user32")]
    extern "system" {
        fn GetKeyboardLayout(id_thread: u32) -> isize;
        fn VkKeyScanExW(ch: u16, layout: isize) -> i16;
    }

    let wide = ch as u32;
    if wide > 0xFFFF {
        return Err(format!(
            "The key \"{ch}\" is not supported for global shortcuts."
        ));
    }

    let layout = unsafe { GetKeyboardLayout(0) };
    let scan = unsafe { VkKeyScanExW(wide as u16, layout) };
    if scan == -1 {
        return Err(format!(
            "The key \"{ch}\" is not available on the current keyboard layout. \
             Re-save the shortcut or choose a different key."
        ));
    }

    let vk = (scan & 0xFF) as u16;
    vk_to_code(vk).ok_or_else(|| {
        format!(
            "Could not map \"{ch}\" to a physical key on this keyboard layout."
        )
    })
}

#[cfg(all(not(any(target_os = "macos", target_os = "windows")), not(test)))]
fn resolve_char_in_current_layout(ch: char) -> Result<Code, String> {
  fallback_us_qwerty_char_to_code(ch).ok_or_else(|| {
        format!(
            "The key \"{ch}\" is not available on the current keyboard layout."
        )
    })
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn scancode_to_code(vk: u16) -> Option<Code> {
    use Code::*;
    match vk {
        0x00 => Some(KeyA),
        0x01 => Some(KeyS),
        0x02 => Some(KeyD),
        0x03 => Some(KeyF),
        0x04 => Some(KeyH),
        0x05 => Some(KeyG),
        0x06 => Some(KeyZ),
        0x07 => Some(KeyX),
        0x08 => Some(KeyC),
        0x09 => Some(KeyV),
        0x0b => Some(KeyB),
        0x0c => Some(KeyQ),
        0x0d => Some(KeyW),
        0x0e => Some(KeyE),
        0x0f => Some(KeyR),
        0x10 => Some(KeyY),
        0x11 => Some(KeyT),
        0x12 => Some(Digit1),
        0x13 => Some(Digit2),
        0x14 => Some(Digit3),
        0x15 => Some(Digit4),
        0x16 => Some(Digit6),
        0x17 => Some(Digit5),
        0x18 => Some(Equal),
        0x19 => Some(Digit9),
        0x1a => Some(Digit7),
        0x1b => Some(Minus),
        0x1c => Some(Digit8),
        0x1d => Some(Digit0),
        0x1e => Some(BracketRight),
        0x1f => Some(KeyO),
        0x20 => Some(KeyU),
        0x21 => Some(BracketLeft),
        0x22 => Some(KeyI),
        0x23 => Some(KeyP),
        0x24 => Some(Enter),
        0x25 => Some(KeyL),
        0x26 => Some(KeyJ),
        0x27 => Some(Quote),
        0x28 => Some(KeyK),
        0x29 => Some(Semicolon),
        0x2a => Some(Backslash),
        0x2b => Some(Comma),
        0x2c => Some(Slash),
        0x2d => Some(KeyN),
        0x2e => Some(KeyM),
        0x2f => Some(Period),
        0x30 => Some(Tab),
        0x31 => Some(Space),
        0x32 => Some(Backquote),
        0x33 => Some(Backspace),
        0x35 => Some(Escape),
        0x60 => Some(F5),
        0x61 => Some(F6),
        0x62 => Some(F7),
        0x63 => Some(F3),
        0x64 => Some(F8),
        0x65 => Some(F9),
        0x67 => Some(F11),
        0x6d => Some(F10),
        0x6f => Some(F12),
        0x7a => Some(F1),
        0x78 => Some(F2),
        0x76 => Some(F4),
        0x7b => Some(ArrowLeft),
        0x7c => Some(ArrowRight),
        0x7d => Some(ArrowDown),
        0x7e => Some(ArrowUp),
        _ => None,
    }
}

#[cfg(target_os = "windows")]
fn vk_to_code(vk: u16) -> Option<Code> {
    use Code::*;
    match vk {
        0x41 => Some(KeyA),
        0x42 => Some(KeyB),
        0x43 => Some(KeyC),
        0x44 => Some(KeyD),
        0x45 => Some(KeyE),
        0x46 => Some(KeyF),
        0x47 => Some(KeyG),
        0x48 => Some(KeyH),
        0x49 => Some(KeyI),
        0x4A => Some(KeyJ),
        0x4B => Some(KeyK),
        0x4C => Some(KeyL),
        0x4D => Some(KeyM),
        0x4E => Some(KeyN),
        0x4F => Some(KeyO),
        0x50 => Some(KeyP),
        0x51 => Some(KeyQ),
        0x52 => Some(KeyR),
        0x53 => Some(KeyS),
        0x54 => Some(KeyT),
        0x55 => Some(KeyU),
        0x56 => Some(KeyV),
        0x57 => Some(KeyW),
        0x58 => Some(KeyX),
        0x59 => Some(KeyY),
        0x5A => Some(KeyZ),
        0x30 => Some(Digit0),
        0x31 => Some(Digit1),
        0x32 => Some(Digit2),
        0x33 => Some(Digit3),
        0x34 => Some(Digit4),
        0x35 => Some(Digit5),
        0x36 => Some(Digit6),
        0x37 => Some(Digit7),
        0x38 => Some(Digit8),
        0x39 => Some(Digit9),
        0xBB => Some(Equal),
        0xBC => Some(Comma),
        0xBD => Some(Minus),
        0xBE => Some(Period),
        0xBF => Some(Slash),
        0xC0 => Some(Backquote),
        0xDB => Some(BracketLeft),
        0xDC => Some(Backslash),
        0xDD => Some(BracketRight),
        0xDE => Some(Quote),
        0xBA => Some(Semicolon),
        0x08 => Some(Backspace),
        0x09 => Some(Tab),
        0x20 => Some(Space),
        0x0D => Some(Enter),
        0x1B => Some(Escape),
        0x21 => Some(PageUp),
        0x22 => Some(PageDown),
        0x23 => Some(End),
        0x24 => Some(Home),
        0x25 => Some(ArrowLeft),
        0x26 => Some(ArrowUp),
        0x27 => Some(ArrowRight),
        0x28 => Some(ArrowDown),
        0x2D => Some(Insert),
        0x2E => Some(Delete),
        0x70 => Some(F1),
        0x71 => Some(F2),
        0x72 => Some(F3),
        0x73 => Some(F4),
        0x74 => Some(F5),
        0x75 => Some(F6),
        0x76 => Some(F7),
        0x77 => Some(F8),
        0x78 => Some(F9),
        0x79 => Some(F10),
        0x7A => Some(F11),
        0x7B => Some(F12),
        _ => None,
    }
}

#[cfg(any(not(any(target_os = "macos", target_os = "windows")), test))]
fn fallback_us_qwerty_char_to_code(ch: char) -> Option<Code> {
    use Code::*;
    match ch {
        'a' | 'A' => Some(KeyA),
        'b' | 'B' => Some(KeyB),
        'c' | 'C' => Some(KeyC),
        'd' | 'D' => Some(KeyD),
        'e' | 'E' => Some(KeyE),
        'f' | 'F' => Some(KeyF),
        'g' | 'G' => Some(KeyG),
        'h' | 'H' => Some(KeyH),
        'i' | 'I' => Some(KeyI),
        'j' | 'J' => Some(KeyJ),
        'k' | 'K' => Some(KeyK),
        'l' | 'L' => Some(KeyL),
        'm' | 'M' => Some(KeyM),
        'n' | 'N' => Some(KeyN),
        'o' | 'O' => Some(KeyO),
        'p' | 'P' => Some(KeyP),
        'q' | 'Q' => Some(KeyQ),
        'r' | 'R' => Some(KeyR),
        's' | 'S' => Some(KeyS),
        't' | 'T' => Some(KeyT),
        'u' | 'U' => Some(KeyU),
        'v' | 'V' => Some(KeyV),
        'w' | 'W' => Some(KeyW),
        'x' | 'X' => Some(KeyX),
        'y' | 'Y' => Some(KeyY),
        'z' | 'Z' => Some(KeyZ),
        '0' => Some(Digit0),
        '1' => Some(Digit1),
        '2' => Some(Digit2),
        '3' => Some(Digit3),
        '4' => Some(Digit4),
        '5' => Some(Digit5),
        '6' => Some(Digit6),
        '7' => Some(Digit7),
        '8' => Some(Digit8),
        '9' => Some(Digit9),
        '`' => Some(Backquote),
        '-' => Some(Minus),
        '=' => Some(Equal),
        '[' => Some(BracketLeft),
        ']' => Some(BracketRight),
        '\\' => Some(Backslash),
        ';' => Some(Semicolon),
        '\'' => Some(Quote),
        ',' => Some(Comma),
        '.' => Some(Period),
        '/' => Some(Slash),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_keys_resolve_without_layout() {
        assert_eq!(resolve_logical_key("Space").unwrap(), Code::Space);
        assert_eq!(resolve_logical_key("F5").unwrap(), Code::F5);
        assert_eq!(resolve_logical_key("Up").unwrap(), Code::ArrowUp);
    }

    #[test]
    fn scancode_mapping_covers_azerty_a_position() {
        // On AZERTY, logical A maps to physical Q position (scancode 0x0c).
        assert_eq!(scancode_to_code(0x0c).unwrap(), Code::KeyQ);
        assert_eq!(scancode_to_code(0x00).unwrap(), Code::KeyA);
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    #[test]
    fn fallback_maps_us_letters() {
        assert_eq!(
            fallback_us_qwerty_char_to_code('A').unwrap(),
            Code::KeyA
        );
    }
}
