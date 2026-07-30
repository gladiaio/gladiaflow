use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use crate::hotkey_layout;

#[cfg(target_os = "macos")]
mod macos_fn {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::ffi::c_void;
    use std::time::Duration;

    const NS_FLAGS_CHANGED_MASK: u64 = 1 << 12; // NSEventMaskFlagsChanged

    pub struct FnKeyTap {
        running: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
        /// NSObject* returned by addGlobalMonitorForEventsMatchingMask (if NSEvent path was used)
        #[allow(dead_code)]
        ns_monitor: Arc<std::sync::Mutex<Option<*mut c_void>>>,
    }

    unsafe impl Send for FnKeyTap {}
    unsafe impl Sync for FnKeyTap {}

    impl FnKeyTap {
        /// Try to start a modifier-key monitor watching the given flag `mask`
        /// (a device-dependent modifier mask, e.g. Right-Cmd = 0x10, or Fn = 0x800000).
        /// Returns Ok(Self) if the monitor is running, Err(reason) otherwise.
        pub fn start<F: Fn(bool) + Send + Sync + 'static>(
            mask: u64,
            callback: F,
        ) -> Result<Self, String> {
            let cb = Arc::new(callback);
            let running = Arc::new(AtomicBool::new(true));
            #[allow(clippy::arc_with_non_send_sync)]
            let ns_monitor: Arc<std::sync::Mutex<Option<*mut c_void>>> =
                Arc::new(std::sync::Mutex::new(None));

            // Channel so the spawned thread can report whether the tap was created
            let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();

            let running_clone = running.clone();
            let cb_clone = cb.clone();

            let thread = std::thread::spawn(move || {
                // Try NSEvent global monitor first (more reliable on modern macOS for Globe key)
                match Self::try_nsevent_monitor(mask, cb_clone.clone(), running_clone.clone()) {
                    Ok(monitor_ptr) => {
                        tx.send(Ok(())).ok();
                        // Keep thread alive; the handler fires on the main run loop,
                        // so we just need to pump the CF run loop here so events flow.
                        Self::pump_runloop(running_clone);
                        // Cleanup: remove monitor
                        Self::remove_nsevent_monitor(monitor_ptr);
                        return;
                    }
                    Err(ns_err) => {
                        log::warn!("NSEvent monitor failed ({ns_err}), falling back to CGEventTap");
                    }
                }

                // Fallback: CGEventTap
                match Self::try_cg_event_tap(mask, cb_clone, running_clone.clone()) {
                    Ok(()) => {
                        tx.send(Ok(())).ok();
                        // pump_runloop already ran inside try_cg_event_tap
                    }
                    Err(cg_err) => {
                        tx.send(Err(format!(
                            "Both NSEvent and CGEventTap failed. \
                             CGEventTap error: {cg_err}. \
                             Accessibility permission is likely not granted."
                        ))).ok();
                    }
                }
            });

            // Wait up to 3s for the thread to report success/failure
            let result = rx
                .recv_timeout(Duration::from_secs(3))
                .map_err(|_| "Timeout waiting for Fn key monitor to initialize".to_string())?;

            result.map(|()| Self {
                running,
                thread: Some(thread),
                ns_monitor,
            })
        }

        fn try_nsevent_monitor(
            mask: u64,
            cb: Arc<dyn Fn(bool) + Send + Sync>,
            _running: Arc<AtomicBool>,
        ) -> Result<*mut c_void, String> {
            use objc2::runtime::{AnyClass, AnyObject};
            use objc2::msg_send;
            use block2::RcBlock;

            let ns_event_cls = AnyClass::get("NSEvent")
                .ok_or("NSEvent class not found")?;

            let was_pressed = Arc::new(AtomicBool::new(false));
            let was_pressed_clone = was_pressed.clone();
            let cb_clone = cb.clone();

            let handler = RcBlock::new(move |event: *mut AnyObject| {
                let modifier_flags: u64 = unsafe { msg_send![event, modifierFlags] };
                let fn_down = (modifier_flags & mask) != 0;
                let prev = was_pressed_clone.swap(fn_down, Ordering::SeqCst);
                if fn_down && !prev {
                    cb_clone(true);
                } else if !fn_down && prev {
                    cb_clone(false);
                }
            });

            let monitor: *mut AnyObject = unsafe {
                msg_send![
                    ns_event_cls,
                    addGlobalMonitorForEventsMatchingMask: NS_FLAGS_CHANGED_MASK
                    handler: &*handler
                ]
            };

            if monitor.is_null() {
                return Err("addGlobalMonitorForEventsMatchingMask returned nil".into());
            }

            Ok(monitor as *mut c_void)
        }

        fn remove_nsevent_monitor(monitor: *mut c_void) {
            use objc2::runtime::AnyClass;
            use objc2::msg_send;
            if !monitor.is_null() {
                let cls = AnyClass::get("NSEvent").unwrap();
                unsafe {
                    let _: () = msg_send![cls, removeMonitor: monitor as *mut objc2::runtime::AnyObject];
                }
            }
        }

        fn try_cg_event_tap(
            mask: u64,
            cb: Arc<dyn Fn(bool) + Send + Sync>,
            running: Arc<AtomicBool>,
        ) -> Result<(), String> {
            use core_graphics::event::{
                CGEvent, CGEventTap, CGEventTapLocation,
                CGEventTapPlacement, CGEventTapOptions, CGEventType,
            };
            use core_foundation::runloop::{CFRunLoop, kCFRunLoopCommonModes};

            let was_pressed = Arc::new(AtomicBool::new(false));

            let tap = CGEventTap::new(
                CGEventTapLocation::Session,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::ListenOnly,
                vec![CGEventType::FlagsChanged],
                move |_proxy, _event_type, event: &CGEvent| {
                    let flags = event.get_flags();
                    let fn_down = (flags.bits() & mask) != 0;
                    let prev = was_pressed.swap(fn_down, Ordering::SeqCst);
                    if fn_down && !prev {
                        cb(true);
                    } else if !fn_down && prev {
                        cb(false);
                    }
                    None
                },
            ).map_err(|()| "CGEventTap::new failed — Accessibility permission required")?;

            unsafe {
                let source = tap.mach_port.create_runloop_source(0)
                    .expect("failed to create runloop source");
                CFRunLoop::get_current().add_source(&source, kCFRunLoopCommonModes);
                tap.enable();
            }

            Self::pump_runloop(running);
            Ok(())
        }

        fn pump_runloop(running: Arc<AtomicBool>) {
            use core_foundation::runloop::{CFRunLoop, CFRunLoopRunResult};
            while running.load(Ordering::SeqCst) {
                let result = CFRunLoop::run_in_mode(
                    unsafe { core_foundation::runloop::kCFRunLoopDefaultMode },
                    Duration::from_millis(250),
                    false,
                );
                // When the run loop has no sources/timers (the NSEvent global-monitor
                // path — its handler fires on the main run loop, not this thread),
                // run_in_mode returns Finished immediately instead of waiting the
                // timeout. Sleep so we don't busy-spin a CPU core at 100%.
                if result == CFRunLoopRunResult::Finished {
                    std::thread::sleep(Duration::from_millis(200));
                }
            }
        }

        pub fn stop(&mut self) {
            self.running.store(false, Ordering::SeqCst);
            if let Some(handle) = self.thread.take() {
                let _ = handle.join();
            }
        }
    }

    impl Drop for FnKeyTap {
        fn drop(&mut self) {
            self.stop();
        }
    }
}

pub struct HotkeyManager {
    current_hotkey: Arc<std::sync::Mutex<String>>,
    active: Arc<AtomicBool>,
    #[cfg(target_os = "macos")]
    fn_tap: Arc<std::sync::Mutex<Option<macos_fn::FnKeyTap>>>,
}

impl HotkeyManager {
    pub fn new() -> Self {
        Self {
            current_hotkey: Arc::new(std::sync::Mutex::new("Fn".to_string())),
            active: Arc::new(AtomicBool::new(false)),
            #[cfg(target_os = "macos")]
            fn_tap: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    pub fn register(&self, hotkey: &str, app_handle: AppHandle) -> Result<(), String> {
        self.unregister(&app_handle)?;

        log::info!("[hotkey] registering shortcut config: {hotkey:?}");

        {
            let mut current = self.current_hotkey.lock().unwrap();
            *current = hotkey.to_string();
        }
        self.active.store(true, Ordering::SeqCst);

        self.register_impl(hotkey, app_handle).map_err(|e| {
            log::error!("[hotkey] failed to register {hotkey:?}: {e}");
            e
        })?;

        log::info!(
            "[hotkey] registered {hotkey:?} via {}",
            registration_method(hotkey)
        );
        Ok(())
    }

    /// On macOS, a lone modifier (incl. left/right variants and Fn) is watched
    /// natively via a flags-changed tap; everything else (composites) goes through
    /// the global-shortcut plugin.
    #[cfg(target_os = "macos")]
    fn register_impl(&self, hotkey: &str, app_handle: AppHandle) -> Result<(), String> {
        if let Some(mask) = single_modifier_mask(hotkey) {
            log::info!(
                "[hotkey] using modifier tap for {hotkey:?} (mask=0x{mask:x})"
            );
            self.register_modifier_tap(hotkey, mask, app_handle)
        } else {
            log::info!("[hotkey] using global-shortcut plugin for {hotkey:?}");
            self.register_modifier_key(hotkey, app_handle)
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn register_impl(&self, hotkey: &str, app_handle: AppHandle) -> Result<(), String> {
        if hotkey == "Fn" {
            return Err("Fn key is only supported on macOS".to_string());
        }
        log::info!("[hotkey] using global-shortcut plugin for {hotkey:?}");
        // Single modifiers can't be watched natively here; register_modifier_key
        // falls back to the Modifier+Space workaround.
        self.register_modifier_key(hotkey, app_handle)
    }

    pub fn unregister(&self, app_handle: &AppHandle) -> Result<(), String> {
        self.active.store(false, Ordering::SeqCst);

        #[cfg(target_os = "macos")]
        {
            let mut tap = self.fn_tap.lock().unwrap();
            if let Some(mut t) = tap.take() {
                t.stop();
            }
        }

        // Unregister any global shortcut plugin shortcuts
        let _ = app_handle.global_shortcut().unregister_all();

        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn register_modifier_tap(&self, hotkey: &str, mask: u64, app_handle: AppHandle) -> Result<(), String> {
        let hotkey_label = hotkey.to_string();
        let handle = app_handle.clone();
        let tap = macos_fn::FnKeyTap::start(mask, move |pressed| {
            if pressed {
                log::info!(
                    "[hotkey] modifier tap: pressed hotkey={hotkey_label:?} mask=0x{mask:x}"
                );
                let _ = handle.emit("hotkey-pressed", ());
            } else {
                log::info!(
                    "[hotkey] modifier tap: released hotkey={hotkey_label:?} mask=0x{mask:x}"
                );
                let _ = handle.emit("hotkey-released", ());
            }
        })?;

        let mut fn_tap = self.fn_tap.lock().unwrap();
        *fn_tap = Some(tap);
        Ok(())
    }

    fn register_modifier_key(&self, key: &str, app_handle: AppHandle) -> Result<(), String> {
        use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

        let shortcut = if key.contains('+') {
            parse_combo_shortcut(key)?
        } else {
            // A lone modifier — possibly side-qualified (e.g. "CmdRight"). The plugin
            // can't bind a bare modifier, so map it to Modifier+Space as a fallback.
            let base = key.trim_end_matches("Left").trim_end_matches("Right");
            let modifiers = match base {
                "Ctrl" => Modifiers::CONTROL,
                "Cmd" => Modifiers::META,
                "Alt" => Modifiers::ALT,
                "Shift" => Modifiers::SHIFT,
                _ => return Err(format!("Unsupported hotkey: {}", key)),
            };
            Shortcut::new(Some(modifiers), Code::Space)
        };

        log::info!(
            "[hotkey] global-shortcut binding: config={key:?} physical={shortcut:?}"
        );

        let configured = key.to_string();
        let handle = app_handle.clone();
        app_handle
            .global_shortcut()
            .on_shortcut(shortcut, move |_app, shortcut, event| {
                use tauri_plugin_global_shortcut::ShortcutState;
                match event.state {
                    ShortcutState::Pressed => {
                        log::info!(
                            "[hotkey] global-shortcut: pressed config={configured:?} physical={shortcut:?}"
                        );
                        let _ = handle.emit("hotkey-pressed", ());
                    }
                    ShortcutState::Released => {
                        log::info!(
                            "[hotkey] global-shortcut: released config={configured:?} physical={shortcut:?}"
                        );
                        let _ = handle.emit("hotkey-released", ());
                    }
                }
            })
            .map_err(|e| e.to_string())
    }
}

#[cfg(target_os = "macos")]
fn registration_method(hotkey: &str) -> &'static str {
    if single_modifier_mask(hotkey).is_some() {
        "modifier-tap"
    } else if hotkey.contains('+') {
        "global-shortcut (combo)"
    } else {
        "global-shortcut (modifier+Space fallback)"
    }
}

#[cfg(not(target_os = "macos"))]
fn registration_method(hotkey: &str) -> &'static str {
    if hotkey.contains('+') {
        "global-shortcut (combo)"
    } else {
        "global-shortcut (modifier+Space fallback)"
    }
}

impl Default for HotkeyManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Maps a single-modifier hotkey token to its macOS device-dependent flag mask
/// (`NX_DEVICE*KEYMASK` from IOLLEvent.h). Left/right are distinct. Returns `None`
/// for composites or any token that isn't a lone modifier.
#[cfg(target_os = "macos")]
fn single_modifier_mask(hotkey: &str) -> Option<u64> {
    let mask = match hotkey {
        "Fn" => 0x0080_0000, // NSEventModifierFlagFunction / kCGEventFlagMaskSecondaryFn
        "CtrlLeft" => 0x0000_0001,
        "ShiftLeft" => 0x0000_0002,
        "ShiftRight" => 0x0000_0004,
        "CmdLeft" => 0x0000_0008,
        "CmdRight" => 0x0000_0010,
        "AltLeft" => 0x0000_0020,
        "AltRight" => 0x0000_0040,
        "CtrlRight" => 0x0000_2000,
        _ => return None,
    };
    Some(mask)
}

/// Validate a hotkey string before persisting or registering it.
pub fn validate_hotkey(hotkey: &str) -> Result<(), String> {
    let hotkey = hotkey.trim();
    if hotkey.is_empty() {
        return Err("Hotkey cannot be empty".into());
    }

    if hotkey == "Fn" {
        #[cfg(not(target_os = "macos"))]
        return Err("Fn key is only supported on macOS".into());
        #[cfg(target_os = "macos")]
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    if single_modifier_mask(hotkey).is_some() {
        return Ok(());
    }

    if hotkey.contains('+') {
        parse_combo_shortcut(hotkey)?;
        return Ok(());
    }

    let base = hotkey.trim_end_matches("Left").trim_end_matches("Right");
    match base {
        "Ctrl" | "Cmd" | "Alt" | "Shift" => Ok(()),
        _ => Err(format!("Unsupported hotkey: {hotkey}")),
    }
}

fn parse_combo_shortcut(
    combo: &str,
) -> Result<tauri_plugin_global_shortcut::Shortcut, String> {
    use tauri_plugin_global_shortcut::{Modifiers, Shortcut};

    let parts: Vec<&str> = combo.split('+').collect();
    if parts.len() < 2 {
        return Err(format!("Shortcut needs at least modifier+key, got: {combo}"));
    }

    let mut mods = Modifiers::empty();
    let mut key_part: Option<&str> = None;

    for part in &parts {
        match part.to_lowercase().as_str() {
            "cmd" | "super" | "meta" | "command" => mods |= Modifiers::SUPER,
            "shift" => mods |= Modifiers::SHIFT,
            "ctrl" | "control" => mods |= Modifiers::CONTROL,
            "alt" | "option" => mods |= Modifiers::ALT,
            _ => {
                if key_part.is_some() {
                    return Err(format!("Multiple non-modifier keys in shortcut: {combo}"));
                }
                key_part = Some(part);
            }
        }
    }

    let key_str = key_part.ok_or_else(|| format!("No non-modifier key in shortcut: {combo}"))?;

    let code = hotkey_layout::resolve_logical_key(key_str)?;
    log::info!(
        "[hotkey] parsed combo {combo:?}: logical_key={key_str:?} physical_code={code:?} modifiers={mods:?}"
    );

    if mods.is_empty() {
        return Err(format!("Shortcut must include at least one modifier: {combo}"));
    }

    Ok(Shortcut::new(Some(mods), code))
}

#[cfg(test)]
mod validation_tests {
    use super::validate_hotkey;

    #[test]
    fn rejects_empty_hotkey() {
        assert!(validate_hotkey("").is_err());
        assert!(validate_hotkey("   ").is_err());
    }

    #[test]
    fn rejects_multiple_non_modifier_keys() {
        assert!(validate_hotkey("Ctrl+A+B").is_err());
    }

    #[test]
    fn accepts_composite_with_stable_key() {
        assert!(validate_hotkey("Ctrl+Space").is_ok());
    }

    #[test]
    fn accepts_composite_with_letter() {
        assert!(validate_hotkey("Cmd+A").is_ok());
    }

    #[test]
    fn accepts_lone_ctrl_modifier() {
        assert!(validate_hotkey("Ctrl").is_ok());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn accepts_fn_on_macos() {
        assert!(validate_hotkey("Fn").is_ok());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn accepts_side_qualified_modifier_on_macos() {
        assert!(validate_hotkey("CmdRight").is_ok());
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn rejects_fn_off_macos() {
        assert!(validate_hotkey("Fn").is_err());
    }

    #[test]
    fn rejects_unknown_token() {
        assert!(validate_hotkey("NotARealHotkey").is_err());
    }
}
