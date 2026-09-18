#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{async_runtime::spawn, Emitter, Manager, State};
use tokio::sync::Mutex as TokioMutex;
use url::Url;

mod audio;
mod config;
mod gladia;
mod history;
mod hotkey;
mod hotkey_layout;
mod permissions;
mod region;
mod screen_context;
mod utterance_cleaner;
mod vocabulary;

use audio::{AudioCapture, AudioDeviceInfo, AudioDeviceSelection};
use gladia::GladiaClient;
use history::TranscriptionHistoryPage;
use hotkey::HotkeyManager;
use permissions::accessibility::AccessibilityState;
use vocabulary::CustomVocabEntry;

#[derive(serde::Serialize, Clone)]
pub struct RecordingState {
    pub is_recording: bool,
    pub is_processing: bool,
    pub phase: String,
    pub title: String,
    pub detail: String,
}

impl Default for RecordingState {
    fn default() -> Self {
        let detail = if cfg!(target_os = "macos") {
            "Hold Fn to dictate. Release to paste."
        } else {
            "Press Ctrl+Space to dictate."
        };
        Self {
            is_recording: false,
            is_processing: false,
            phase: "idle".to_string(),
            title: "Ready".to_string(),
            detail: detail.to_string(),
        }
    }
}

pub struct AppState {
    pub gladia: Arc<TokioMutex<GladiaClient>>,
    pub audio: Arc<AudioCapture>,
    pub audio_lifecycle: Arc<TokioMutex<()>>,
    pub hotkey_manager: Arc<TokioMutex<HotkeyManager>>,
    pub recording_state: Arc<TokioMutex<RecordingState>>,
    pub subscription_active: Arc<std::sync::atomic::AtomicBool>,
    pub audio_sender: Arc<TokioMutex<Option<tokio::task::JoinHandle<()>>>>,
    pub tray_animation: Arc<TokioMutex<Option<tauri::async_runtime::JoinHandle<()>>>>,
    pub region: &'static str,
    /// On-screen terms from the latest harvest — used to rewrite ASR output.
    pub session_screen_terms: Arc<std::sync::Mutex<Vec<String>>>,
}

const TRAY_ID: &str = "main-tray";
const TRAY_ICON_IDLE: &[u8] = include_bytes!("../icons/32x32.png");
const TRAY_ICON_DICTATING: &[u8] = include_bytes!("../icons/tray-microphone.png");
const TRAY_SPINNER_FRAMES: &[&[u8]] = &[
    include_bytes!("../icons/tray-spinner/frame_00.png"),
    include_bytes!("../icons/tray-spinner/frame_01.png"),
    include_bytes!("../icons/tray-spinner/frame_02.png"),
    include_bytes!("../icons/tray-spinner/frame_03.png"),
    include_bytes!("../icons/tray-spinner/frame_04.png"),
    include_bytes!("../icons/tray-spinner/frame_05.png"),
    include_bytes!("../icons/tray-spinner/frame_06.png"),
    include_bytes!("../icons/tray-spinner/frame_07.png"),
    include_bytes!("../icons/tray-spinner/frame_08.png"),
    include_bytes!("../icons/tray-spinner/frame_09.png"),
    include_bytes!("../icons/tray-spinner/frame_10.png"),
    include_bytes!("../icons/tray-spinner/frame_11.png"),
];

#[derive(Default)]
struct FirstAudioChunkGate {
    seen: bool,
}

impl FirstAudioChunkGate {
    fn observe(&mut self) -> bool {
        if self.seen {
            false
        } else {
            self.seen = true;
            true
        }
    }
}

fn tray_icon_from_bytes(bytes: &[u8]) -> Result<tauri::image::Image<'static>, String> {
    tauri::image::Image::from_bytes(bytes).map_err(|e| e.to_string())
}

fn tray_handle(app: &tauri::AppHandle) -> Result<tauri::tray::TrayIcon, String> {
    app.tray_by_id(TRAY_ID)
        .ok_or_else(|| format!("Tray icon '{TRAY_ID}' not found"))
}

async fn stop_tray_spinner(state: &AppState) {
    let mut guard = state.tray_animation.lock().await;
    if let Some(handle) = guard.take() {
        handle.abort();
    }
}

async fn apply_tray_activity(
    app: &tauri::AppHandle,
    state: &AppState,
    activity: &str,
) -> Result<(), String> {
    stop_tray_spinner(state).await;

    let tray = tray_handle(app)?;
    match activity {
        "recording" => {
            let icon = tray_icon_from_bytes(TRAY_ICON_DICTATING)?;
            tray.set_icon(Some(icon)).map_err(|e| e.to_string())?;
            tray.set_tooltip(Some("GladiaFlow — Dictating…"))
                .map_err(|e| e.to_string())?;
        }
        "starting" | "finalizing" => {
            let starting = activity == "starting";
            let app_handle = app.clone();
            let handle = spawn(async move {
                let mut frame_idx = 0usize;
                loop {
                    let bytes = TRAY_SPINNER_FRAMES[frame_idx];
                    if let Ok(icon) = tray_icon_from_bytes(bytes) {
                        if let Ok(tray) = tray_handle(&app_handle) {
                            let _ = tray.set_icon(Some(icon));
                            let tooltip = if starting {
                                "GladiaFlow — Starting microphone…"
                            } else {
                                "GladiaFlow — Finalizing…"
                            };
                            let _ = tray.set_tooltip(Some(tooltip));
                        }
                    }
                    frame_idx = (frame_idx + 1) % TRAY_SPINNER_FRAMES.len();
                    tokio::time::sleep(Duration::from_millis(80)).await;
                }
            });
            *state.tray_animation.lock().await = Some(handle);
        }
        _ => {
            let icon = tray_icon_from_bytes(TRAY_ICON_IDLE)?;
            tray.set_icon(Some(icon)).map_err(|e| e.to_string())?;
            tray.set_tooltip(Some("GladiaFlow"))
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
async fn set_tray_activity(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    activity: String,
) -> Result<(), String> {
    apply_tray_activity(&app, &state, &activity).await
}

fn show_main_window(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);

    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}!", name)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // Tauri commands expose a flat, backwards-compatible IPC payload.
async fn init_gladia_session(
    api_key: String,
    languages: Option<Vec<String>>,
    device_name: Option<String>,
    device_selection: Option<AudioDeviceSelection>,
    code_switching: Option<bool>,
    custom_vocabulary: Option<Vec<CustomVocabEntry>>,
    endpointing: Option<f64>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    // Preparation is the single source of truth for the capture/Gladia rate;
    // do not rediscover the device on this latency-sensitive path.
    let selection = AudioDeviceSelection::from_ipc(device_selection, device_name);
    let audio_capture = state.audio.clone();
    let sample_rate = tokio::task::spawn_blocking(move || audio_capture.prepare_capture(selection))
        .await
        .map_err(|error| format!("Failed to join audio preparation task: {error}"))?
        .map_err(|error| error.to_string())?;
    log::info!(
        "initialising Gladia session (region={}, sample_rate={}, code_switching={})",
        state.region,
        sample_rate,
        code_switching.unwrap_or(false)
    );
    let gladia = state.gladia.lock().await;
    gladia.set_audio_format(sample_rate, 1, 16).await;
    let custom_vocabulary = custom_vocabulary
        .filter(|entries| !entries.is_empty())
        .or_else(|| config::get_custom_vocabulary().ok())
        .unwrap_or_default();
    let custom_vocabulary = if config::use_screen_context_vocabulary().unwrap_or(false) {
        // Ensure sampler is running; then force a fresh chat OCR so names shown
        // seconds ago (Slack search, etc.) are in the snapshot — the rolling
        // store alone can miss them when OCR was throttled on another window.
        screen_context::start_background_sampler();
        let refresh = tokio::task::spawn_blocking(screen_context::refresh_chat_context_for_dictation);
        match tokio::time::timeout(screen_context::DICTATION_REFRESH_TIMEOUT, refresh).await {
            Ok(Ok(n)) if n > 0 => log::info!(
                "[screen_context] pre-dictation chat OCR refresh upserted {n} term(s)"
            ),
            Ok(Ok(_)) => log::info!("[screen_context] pre-dictation chat OCR refresh: no new terms"),
            Ok(Err(e)) => log::warn!("[screen_context] pre-dictation refresh join failed: {e}"),
            Err(_) => log::warn!(
                "[screen_context] pre-dictation chat OCR refresh timed out after {}ms",
                screen_context::DICTATION_REFRESH_TIMEOUT.as_millis()
            ),
        }
        let screen_terms = screen_context::snapshot_screen_vocabulary();
        if !screen_terms.is_empty() {
            log::info!(
                "[screen_context] using {} on-screen vocabulary term(s) from rolling store",
                screen_terms.len()
            );
        }
        let session_terms: Vec<String> = screen_terms.iter().map(|e| e.value.clone()).collect();
        if let Ok(mut slot) = state.session_screen_terms.lock() {
            *slot = session_terms;
        }
        screen_context::merge_vocabulary_manual_priority(custom_vocabulary, screen_terms)
    } else {
        if let Ok(mut slot) = state.session_screen_terms.lock() {
            slot.clear();
        }
        custom_vocabulary
    };
    let endpointing = match endpointing {
        Some(endpointing) => endpointing,
        None => config::endpointing()?,
    };
    gladia
        .init_session(
            &api_key,
            languages,
            code_switching.unwrap_or(false),
            custom_vocabulary,
            endpointing,
            state.region,
        )
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn test_gladia_connection(
    api_key: String,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let gladia = state.gladia.lock().await;
    gladia
        .test_connection(&api_key)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn list_transcription_history(
    query: Option<String>,
    page: Option<u32>,
    page_size: Option<u32>,
) -> Result<TranscriptionHistoryPage, String> {
    history::list_entries(query.as_deref(), page, page_size)
}

#[tauri::command]
fn open_external_url(url: String) -> Result<(), String> {
    let url = validate_external_url(&url)?;
    // Detached launch: default browser on macOS/Windows/Linux without blocking the app.
    open::that_detached(url.as_str()).map_err(|e| e.to_string())
}

fn validate_external_url(url: &str) -> Result<Url, String> {
    let url = Url::parse(url).map_err(|_| "Invalid external URL".to_string())?;
    let is_allowed_host = url.host_str().is_some_and(|host| {
        host == "gladia.io"
            || host
                .strip_suffix(".gladia.io")
                .is_some_and(|subdomain| !subdomain.is_empty())
    });
    let has_credentials = !url.username().is_empty() || url.password().is_some();

    if url.scheme() != "https"
        || !is_allowed_host
        || url.port_or_known_default() != Some(443)
        || has_credentials
    {
        return Err("Only trusted Gladia URLs can be opened".to_string());
    }

    Ok(url)
}

#[cfg(test)]
mod external_url_tests {
    use super::validate_external_url;

    #[test]
    fn accepts_trusted_gladia_urls() {
        for url in [
            "https://gladia.io/",
            "https://app.gladia.io/apikeys",
            "https://docs.gladia.io/chapters/audio-intelligence/custom-vocabulary",
            "https://app.gladia.io/transcriptions/live/session-id?source=gladiaflow",
            "https://nested.internal.gladia.io/path",
        ] {
            assert!(validate_external_url(url).is_ok(), "should allow {url}");
        }
    }

    #[test]
    fn rejects_untrusted_or_malformed_urls() {
        for url in [
            "http://app.gladia.io/apikeys",
            "https://evil.example/phish",
            "https://evilgladia.io/phish",
            "https://app.gladia.io.evil.example/phish",
            "https://app.gladia.io@evil.example/phish",
            "https://attacker@app.gladia.io/phish",
            "https://app.gladia.io:444/phish",
            "file:///tmp/phish",
            "javascript:alert(1)",
            "not a url",
        ] {
            assert!(validate_external_url(url).is_err(), "should reject {url}");
        }
    }
}

#[tauri::command]
async fn send_audio_chunk(audio_data: Vec<u8>, state: State<'_, AppState>) -> Result<(), String> {
    let gladia = state.gladia.lock().await;
    gladia
        .send_audio(&audio_data)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn stop_recording(state: State<'_, AppState>) -> Result<(), String> {
    let gladia = state.gladia.lock().await;
    gladia.stop_recording().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn request_notification_permission() -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        use block2_06::RcBlock;
        use objc2_06::runtime::Bool;
        use objc2_user_notifications::{UNAuthorizationOptions, UNUserNotificationCenter};
        use std::sync::{Arc, Mutex};

        let executable = std::env::current_exe()
            .map_err(|error| format!("Could not resolve current executable: {error}"))?;
        let is_bundled_app = executable
            .ancestors()
            .any(|path| path.extension().is_some_and(|extension| extension == "app"));
        if !is_bundled_app {
            log::warn!(
                "[notifications] native authorization unavailable for unbundled tauri dev process"
            );
            return Ok(false);
        }

        let (sender, receiver) = tokio::sync::oneshot::channel::<bool>();
        let sender = Arc::new(Mutex::new(Some(sender)));
        {
            let completion_sender = sender.clone();
            let completion = RcBlock::new(move |granted: Bool, _error| {
                if let Ok(mut sender) = completion_sender.lock() {
                    if let Some(sender) = sender.take() {
                        let _ = sender.send(granted.as_bool());
                    }
                }
            });

            let center = UNUserNotificationCenter::currentNotificationCenter();
            center.requestAuthorizationWithOptions_completionHandler(
                UNAuthorizationOptions::Alert
                    | UNAuthorizationOptions::Sound
                    | UNAuthorizationOptions::Badge,
                &completion,
            );
        }

        receiver
            .await
            .map_err(|_| "Notification permission request was cancelled".to_string())
    }

    #[cfg(not(target_os = "macos"))]
    Ok(true)
}

#[tauri::command]
async fn close_gladia_session(state: State<'_, AppState>) -> Result<(), String> {
    log::info!("closing Gladia session");
    let gladia = state.gladia.lock().await;
    gladia.close_session().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn subscribe_to_transcriptions(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    // Guard: if a subscription task is already alive, do not spawn a second emitter.
    if state
        .subscription_active
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(());
    }

    let mut rx = {
        let gladia = state.gladia.lock().await;
        gladia.subscribe_to_transcriptions().await
    };

    let session_id = {
        let gladia = state.gladia.lock().await;
        gladia.current_session_id().await
    };

    let active = state.subscription_active.clone();
    let screen_terms = state
        .session_screen_terms
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default();
    spawn(async move {
        let mut cleaner = utterance_cleaner::UtteranceCleaner::new();
        // When enabled, the final transcription is left on the clipboard at
        // session end instead of restoring the user's original clipboard.
        let copy_to_clipboard = config::copy_to_clipboard().unwrap_or_else(|error| {
            log::error!("[config] failed to load copy-to-clipboard setting; using false: {error}");
            false
        });
        let mut final_transcript: Option<String> = None;
        // Snapshot the user's clipboard once so it can be restored at session end.
        let original_clipboard = clipboard_get();

        // A dedicated worker thread serialises pastes so the recv loop never
        // blocks on the ~250ms paste round-trip and fragments land in order.
        // The worker restores the user's clipboard once, after it drains.
        let (paste_tx, paste_worker_handle) = {
            let (tx, paste_rx) = std::sync::mpsc::channel::<String>();
            let restore_clip = original_clipboard.clone();
            let handle = std::thread::spawn(move || {
                for fragment in paste_rx {
                    if let Err(e) = clipboard_set(&fragment) {
                        log::warn!("[paste] failed to set clipboard for fragment: {e}");
                        continue;
                    }
                    // Brief pause before the keystroke, then let the paste land
                    // before the next fragment overwrites the clipboard.
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    if let Err(e) = send_paste_keystroke() {
                        log::error!("[paste] keystroke failed: {e}");
                    }
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
                // Channel drained (session ended): restore the user's clipboard.
                match clipboard_set(&restore_clip) {
                    Ok(()) => log::info!("[paste] original clipboard restored"),
                    Err(e) => log::warn!("[paste] failed to restore clipboard: {e}"),
                }
            });
            (tx, handle)
        };

        // Per-session counters for lifecycle logging (text content is never logged).
        let mut partial_count: usize = 0;
        let mut final_count: usize = 0;
        log::info!("[transcription] subscription started");

        while let Ok(event) = rx.recv().await {
            match event {
                gladia::TranscriptionEvent::Error(message) => {
                    log::error!("[transcription] error: {message}");
                    let _ = app_handle.emit("transcription-error", message);
                }
                gladia::TranscriptionEvent::Partial(text) => {
                    partial_count += 1;
                    if partial_count == 1 {
                        log::info!(
                            "[transcription] first partial received ({} chars)",
                            text.chars().count()
                        );
                    }
                    let _ = app_handle.emit("transcription-partial", text);
                }
                gladia::TranscriptionEvent::Final { text, start, end } => {
                    final_count += 1;
                    // Always run local rewrite: spoken "underscore" → `_`, and map
                    // collapsed forms to harvested filenames when available.
                    let rewritten = screen_context::rewrite_transcript(&text, &screen_terms);
                    if rewritten != text {
                        log::info!(
                            "[screen_context] rewrote final utterance using on-screen terms"
                        );
                    }
                    let text = rewritten;
                    let fragment = cleaner.process_final(&text, start, end);
                    log::info!(
                        "[transcription] final #{final_count} [{start:.2}s-{end:.2}s] ({} chars, transcript now {} chars)",
                        text.chars().count(),
                        cleaner.accumulated_text().chars().count()
                    );
                    // Paste just the newly appended fragment as we go.
                    if let Some(frag) = fragment {
                        let _ = paste_tx.send(frag);
                    }
                    let _ = app_handle
                        .emit("transcription-final", cleaner.accumulated_text().to_owned());
                }
                gladia::TranscriptionEvent::SessionEnded => {
                    let last_period = cleaner.flush_pending_period();
                    let full_text = cleaner.accumulated_text().to_owned();
                    final_transcript = Some(full_text.clone());
                    let full_len = full_text.chars().count();
                    log::info!(
                        "[transcription] session ended: {partial_count} partials, {final_count} finals, {full_len} chars total"
                    );

                    if let Some(ref sid) = session_id {
                        if !full_text.trim().is_empty() {
                            let sid = sid.clone();
                            let text = full_text.clone();
                            match tokio::task::spawn_blocking(move || {
                                history::save_entry(&sid, &text)
                            })
                            .await
                            {
                                Ok(Ok(())) => log::info!("[history] saved transcription"),
                                Ok(Err(e)) => log::warn!("[history] save failed: {e}"),
                                Err(e) => log::warn!("[history] spawn failed: {e}"),
                            }
                        }
                    } else {
                        log::warn!("[history] no session id; transcription not saved");
                    }

                    // Queue the deferred final period before signaling the frontend.
                    if let Some(p) = last_period {
                        let _ = paste_tx.send(p.to_string());
                    }

                    // Gladia has finalized; the frontend can close the session while
                    // the paste worker continues draining in the background.
                    let _ = app_handle.emit("session-ended", full_text.clone());
                    break;
                }
            }
        }
        // Drop the sender so the paste worker drains its queue and restores
        // the clipboard, including when the loop ends without a SessionEnded event.
        drop(paste_tx);
        if let Err(e) = tokio::task::spawn_blocking(move || {
            if let Err(join_err) = paste_worker_handle.join() {
                log::warn!("[paste] worker thread panicked: {join_err:?}");
            }
        })
        .await
        {
            log::warn!("[paste] failed to join paste worker: {e}");
        }
        // The worker has restored the original clipboard by now; if the user
        // opted in, overwrite it with the final transcription instead.
        if copy_to_clipboard {
            if let Some(text) = final_transcript.filter(|t| !t.is_empty()) {
                match clipboard_set(&text) {
                    Ok(()) => log::info!("[paste] final transcription left on clipboard"),
                    Err(e) => log::warn!("[paste] failed to copy transcription to clipboard: {e}"),
                }
            }
        }
        let _ = app_handle.emit("paste-complete", ());
        active.store(false, Ordering::SeqCst);
        log::info!("[transcription] subscription task ended");
    });

    Ok(())
}

#[tauri::command]
async fn list_audio_devices() -> Result<Vec<String>, String> {
    Ok(audio::list_input_devices())
}

#[tauri::command]
async fn list_audio_device_info() -> Result<Vec<AudioDeviceInfo>, String> {
    Ok(audio::list_input_device_info())
}

#[tauri::command]
async fn prepare_audio_capture(
    device_name: Option<String>,
    device_selection: Option<AudioDeviceSelection>,
    state: State<'_, AppState>,
) -> Result<u32, String> {
    let _lifecycle = state.audio_lifecycle.lock().await;
    let audio = state.audio.clone();
    let selection = AudioDeviceSelection::from_ipc(device_selection, device_name);
    tokio::task::spawn_blocking(move || audio.prepare_capture(selection))
        .await
        .map_err(|error| format!("Failed to join audio preparation task: {error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn start_audio_capture(
    device_name: Option<String>,
    device_selection: Option<AudioDeviceSelection>,
    capture_id: Option<String>,
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let _lifecycle = state.audio_lifecycle.lock().await;

    if let Some(previous_sender) = state.audio_sender.lock().await.take() {
        if !previous_sender.is_finished() {
            *state.audio_sender.lock().await = Some(previous_sender);
            return Err("Previous audio session is still shutting down".to_string());
        }
        let _ = previous_sender.await;
    }

    // Channel for fatal audio-capture errors raised inside the capture thread
    // (no device, bad config, stream failure). Forward them to the frontend so a
    // dead microphone surfaces instead of failing silently.
    let (err_tx, err_rx) = crossbeam_channel::unbounded::<String>();
    let selection = AudioDeviceSelection::from_ipc(device_selection, device_name);
    let (rx, resolved_device_name) = state
        .audio
        .start_capture(selection, Some(err_tx))
        .map_err(|e| e.to_string())?;
    let sample_rate = state.audio.get_sample_rate();

    let err_app = app_handle.clone();
    tokio::task::spawn_blocking(move || {
        while let Ok(message) = err_rx.recv() {
            log::error!("audio capture error: {message}");
            let _ = err_app.emit("transcription-error", message);
        }
    });

    let gladia_arc = state.gladia.clone();

    // Bridge the blocking crossbeam channel into an async tokio channel so the
    // Tokio executor (which drives the WebSocket task) is never blocked.
    let (tx, mut async_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();

    let ready_app = app_handle.clone();
    let ready_capture_id = capture_id.unwrap_or_default();
    let capture_started = Instant::now();
    tokio::task::spawn_blocking(move || {
        let mut readiness = FirstAudioChunkGate::default();
        while let Ok(chunk) = rx.recv() {
            if readiness.observe() {
                let _ = ready_app.emit(
                    "audio-capture-ready",
                    serde_json::json!({
                        "captureId": ready_capture_id,
                        "deviceName": &resolved_device_name,
                        "wakeLatencyMs": capture_started.elapsed().as_millis(),
                    }),
                );
            }
            if tx.send(chunk.data).is_err() {
                break;
            }
        }
    });

    let sender_task = tokio::spawn(async move {
        // sample_rate Hz × 2 bytes/sample × 1 channel × 100 ms
        let chunk_bytes = (sample_rate as usize * 2 * 100) / 1000;
        let mut buf: Vec<u8> = Vec::with_capacity(chunk_bytes * 2);

        while let Some(data) = async_rx.recv().await {
            buf.extend_from_slice(&data);
            while buf.len() >= chunk_bytes {
                let chunk: Vec<u8> = buf.drain(..chunk_bytes).collect();
                let client = gladia_arc.lock().await;
                if let Err(e) = client.send_audio(&chunk).await {
                    log::error!("failed to send audio chunk to Gladia: {e}");
                }
            }
        }

        // Flush the partial tail so no audio is lost before stop_recording arrives.
        if !buf.is_empty() {
            let client = gladia_arc.lock().await;
            client.send_audio(&buf).await.ok();
        }

        log::info!("audio: sender task drained");
    });
    *state.audio_sender.lock().await = Some(sender_task);

    Ok(())
}

#[cfg(test)]
mod readiness_tests {
    use super::FirstAudioChunkGate;

    #[test]
    fn readiness_is_emitted_only_for_the_first_chunk() {
        let mut gate = FirstAudioChunkGate::default();
        assert!(gate.observe());
        assert!(!gate.observe());
        assert!(!gate.observe());
    }
}

async fn stop_and_drain_audio(state: &AppState) -> Result<(), String> {
    let audio = state.audio.clone();
    tokio::task::spawn_blocking(move || audio.stop_capture())
        .await
        .map_err(|e| format!("Failed to join audio cleanup task: {e}"))?
        .map_err(|e| e.to_string())?;

    if let Some(mut sender_task) = state.audio_sender.lock().await.take() {
        match tokio::time::timeout(std::time::Duration::from_secs(2), &mut sender_task).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) if e.is_cancelled() => {}
            Ok(Err(e)) => return Err(format!("Audio sender task failed: {e}")),
            Err(_) => {
                log::warn!("timed out waiting for audio buffer to drain; aborting sender task");
                sender_task.abort();
                let _ = sender_task.await;
            }
        }
    }

    Ok(())
}

#[tauri::command]
async fn stop_audio_capture(state: State<'_, AppState>) -> Result<(), String> {
    let _lifecycle = state.audio_lifecycle.lock().await;
    stop_and_drain_audio(&state).await
}

/// Stop audio capture AND send the stop_recording signal to Gladia in the
/// correct order: waits for the audio sender task to fully drain its buffer
/// before telling Gladia to finalise.
#[tauri::command]
async fn stop_dictation(state: State<'_, AppState>) -> Result<(), String> {
    let _lifecycle = state.audio_lifecycle.lock().await;
    log::info!("stopping dictation; draining audio and finalising Gladia session");
    stop_and_drain_audio(&state).await?;

    let gladia = state.gladia.lock().await;
    gladia.stop_recording().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_recording_state(state: State<'_, AppState>) -> Result<RecordingState, String> {
    let state = state.recording_state.lock().await.clone();
    Ok(state)
}

#[tauri::command]
async fn set_recording_state(
    phase: String,
    title: String,
    detail: String,
    is_recording: bool,
    is_processing: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut rec_state = state.recording_state.lock().await;
    rec_state.phase = phase;
    rec_state.title = title;
    rec_state.detail = detail;
    rec_state.is_recording = is_recording;
    rec_state.is_processing = is_processing;
    Ok(())
}

/// Read the current clipboard text (best-effort; empty string on failure).
fn clipboard_get() -> String {
    use arboard::Clipboard;
    Clipboard::new()
        .ok()
        .and_then(|mut cb| cb.get_text().ok())
        .unwrap_or_default()
}

/// Write text to the clipboard.
fn clipboard_set(text: &str) -> Result<(), String> {
    use arboard::Clipboard;
    let mut clipboard = Clipboard::new().map_err(|e| e.to_string())?;
    clipboard.set_text(text).map_err(|e| e.to_string())
}

/// CoreGraphics keyboard-event simulation. Posting a synthetic Cmd+V directly is
/// faster than `osascript` and — crucially — only needs Accessibility, not the
/// separate "control System Events" Automation permission an AppleScript prompts for.
#[cfg(target_os = "macos")]
mod cg_paste {
    use std::ffi::c_void;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventCreateKeyboardEvent(
            source: *const c_void,
            keycode: u16,
            key_down: bool,
        ) -> *mut c_void;
        fn CGEventSetFlags(event: *mut c_void, flags: u64);
        fn CGEventPost(tap: u32, event: *mut c_void);
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *const c_void);
    }

    const KCG_HID_EVENT_TAP: u32 = 0;
    const KVK_V: u16 = 9;
    const KCG_EVENT_FLAG_MASK_COMMAND: u64 = 1 << 20;

    /// Post a synthetic Cmd+V into the focused app. Requires Accessibility.
    pub fn simulate_cmd_v() -> Result<(), String> {
        unsafe {
            let key_down = CGEventCreateKeyboardEvent(std::ptr::null(), KVK_V, true);
            if key_down.is_null() {
                return Err("Failed to create key-down event".into());
            }
            CGEventSetFlags(key_down, KCG_EVENT_FLAG_MASK_COMMAND);
            CGEventPost(KCG_HID_EVENT_TAP, key_down);
            CFRelease(key_down as *const c_void);

            std::thread::sleep(std::time::Duration::from_millis(20));

            let key_up = CGEventCreateKeyboardEvent(std::ptr::null(), KVK_V, false);
            if key_up.is_null() {
                return Err("Failed to create key-up event".into());
            }
            CGEventSetFlags(key_up, KCG_EVENT_FLAG_MASK_COMMAND);
            CGEventPost(KCG_HID_EVENT_TAP, key_up);
            CFRelease(key_up as *const c_void);
        }
        Ok(())
    }
}

/// Simulate the paste keystroke into the focused app (Cmd+V on macOS, Ctrl+V
/// elsewhere). Requires Accessibility permission on macOS.
fn send_paste_keystroke() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        cg_paste::simulate_cmd_v()
    }
    #[cfg(target_os = "windows")]
    {
        use enigo::{Direction, Enigo, Key, Keyboard, Settings};
        let mut enigo = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;
        enigo
            .key(Key::Control, Direction::Press)
            .map_err(|e| e.to_string())?;
        enigo
            .key(Key::Unicode('v'), Direction::Click)
            .map_err(|e| e.to_string())?;
        enigo
            .key(Key::Control, Direction::Release)
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Ok(())
    }
}

#[tauri::command]
fn get_region(state: State<'_, AppState>) -> String {
    state.region.to_string()
}

/// Reveal the folder containing the rolling log file (e.g.
/// `~/Library/Logs/io.gladia.gladiaflow/` on macOS) so the user can attach or share it.
#[tauri::command]
fn open_log_folder(app: tauri::AppHandle) -> Result<(), String> {
    let dir = app
        .path()
        .app_log_dir()
        .map_err(|e| format!("Could not resolve log directory: {e}"))?;
    // Make sure it exists even if nothing has been logged yet this session.
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    #[cfg(target_os = "macos")]
    {
        // The log directory (~/Library/Logs/io.gladia.gladiaflow) can be
        // interpreted by macOS LaunchServices as an application bundle, so
        // `open <dir>` may try to *launch* it and fail ("the application cannot
        // be opened because its executable is missing"). Reveal the log file in
        // Finder with `open -R` instead, which sidesteps the package
        // interpretation and hands the user the exact file they need to share.
        // Falls back to revealing the directory itself if no log file exists yet.
        let log_file = dir.join("gladiaflow.log");
        let target: &std::path::Path = if log_file.exists() { &log_file } else { &dir };
        std::process::Command::new("open")
            .arg("-R")
            .arg(target)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&dir)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&dir)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn get_platform() -> String {
    std::env::consts::OS.to_string()
}

#[tauri::command]
async fn show_window(window: tauri::Window) -> Result<(), String> {
    window.show().map_err(|e| e.to_string())
}

// Hide the main window
#[tauri::command]
async fn hide_window(window: tauri::Window) -> Result<(), String> {
    window.hide().map_err(|e| e.to_string())
}

// Minimize the main window
#[tauri::command]
async fn minimize_window(window: tauri::Window) -> Result<(), String> {
    window.minimize().map_err(|e| e.to_string())
}

#[tauri::command]
async fn register_dictation_hotkey(
    hotkey: String,
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let manager = state.hotkey_manager.lock().await;
    manager.register(&hotkey, app_handle)
}

#[tauri::command]
async fn unregister_dictation_hotkey(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let manager = state.hotkey_manager.lock().await;
    manager.unregister(&app_handle)
}

#[tauri::command]
async fn get_accessibility_state() -> Result<AccessibilityState, String> {
    Ok(permissions::accessibility::check_and_log_state())
}

#[tauri::command]
async fn dev_reset_accessibility_tcc() -> Result<AccessibilityState, String> {
    permissions::accessibility::dev_reset_tcc()
}

#[tauri::command]
async fn open_system_settings(panel: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let url = match panel.as_str() {
            "accessibility" => {
                let _ = crate::config::mark_accessibility_prompted();
                permissions::accessibility::open_accessibility_settings();
                return Ok(());
            }
            "microphone" => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
            }
            "input_monitoring" => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"
            }
            "keyboard" => "x-apple.systempreferences:com.apple.Keyboard-Settings.extension",
            _ => return Err(format!("Unknown panel: {}", panel)),
        };
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "windows")]
    {
        let uri = match panel.as_str() {
            "accessibility" => "ms-settings:easeofaccess",
            "microphone" => "ms-settings:privacy-microphone",
            "input_monitoring" => "ms-settings:privacy-microphone",
            "keyboard" => "ms-settings:easeofaccess-keyboard",
            _ => return Err(format!("Unknown panel: {}", panel)),
        };
        std::process::Command::new("cmd")
            .args(["/C", "start", uri])
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[tauri::command]
async fn check_microphone_permission() -> Result<String, String> {
    tokio::task::spawn_blocking(microphone_permission::check_status_fresh)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn request_microphone_permission() -> Result<bool, String> {
    tokio::task::spawn_blocking(microphone_permission::request_access)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
mod microphone_permission {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject, Bool};

    #[link(name = "AVFoundation", kind = "framework")]
    extern "C" {
        static AVMediaTypeAudio: *const AnyObject;
    }

    pub fn check_status() -> String {
        unsafe {
            let cls = match AnyClass::get("AVCaptureDevice") {
                Some(c) => c,
                None => return "unknown".into(),
            };
            let status: i64 = msg_send![cls, authorizationStatusForMediaType: AVMediaTypeAudio];
            match status {
                0 => "not_determined",
                1 => "restricted",
                2 => "denied",
                3 => "authorized",
                _ => "unknown",
            }
            .into()
        }
    }

    /// `authorizationStatusForMediaType:` is cached for the process lifetime, so a
    /// grant the user just made (via the system prompt or System Settings) isn't
    /// seen until the app restarts. Spawn our own binary with `--check-mic` to read
    /// a fresh, uncached status; fall back to the in-process check if the
    /// subprocess can't be launched.
    pub fn check_status_fresh() -> String {
        if let Ok(exe) = std::env::current_exe() {
            if let Ok(output) = std::process::Command::new(&exe).arg("--check-mic").output() {
                if output.status.success() {
                    let status = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !status.is_empty() {
                        return status;
                    }
                }
            }
        }
        check_status()
    }

    pub fn request_access() -> bool {
        use std::sync::mpsc;
        unsafe {
            let cls = match AnyClass::get("AVCaptureDevice") {
                Some(c) => c,
                None => return false,
            };

            // Rely on the completion handler for the result. Polling
            // `authorizationStatusForMediaType:` in-process would never observe
            // the change because that value is cached for the process lifetime.
            let (tx, rx) = mpsc::channel::<bool>();
            let handler = block2::RcBlock::new(move |granted: Bool| {
                let _ = tx.send(granted.as_bool());
            });
            let _: () = msg_send![
                cls,
                requestAccessForMediaType: AVMediaTypeAudio
                completionHandler: &*handler
            ];

            match rx.recv_timeout(std::time::Duration::from_secs(120)) {
                Ok(granted) => granted,
                // Handler never fired (e.g. no prompt shown); read a fresh status.
                Err(_) => check_status_fresh() == "authorized",
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod microphone_permission {
    pub fn check_status() -> String {
        "authorized".into()
    }

    pub fn check_status_fresh() -> String {
        "authorized".into()
    }

    pub fn request_access() -> bool {
        true
    }
}

fn main() {
    // Fast path for the fresh-Accessibility probe: when invoked with `--check-ax`
    // (see `accessibility::is_trusted_fresh`), report trust via the exit code and
    // exit before any window or Tauri runtime spins up.
    if std::env::args().any(|a| a == "--check-ax-state") {
        print!("{}", permissions::accessibility::check_state().as_str());
        std::process::exit(0);
    }

    // Legacy fast path kept for any external tooling still using --check-ax.
    if std::env::args().any(|a| a == "--check-ax") {
        let trusted = matches!(
            permissions::accessibility::check_state(),
            AccessibilityState::GrantedWorking
        );
        std::process::exit(if trusted { 0 } else { 1 });
    }

    // Fast path for the fresh-microphone probe: like `--check-ax`, the macOS
    // microphone authorization status is cached for the process lifetime, so we
    // read it from a throwaway subprocess (see `microphone_permission::check_status_fresh`).
    if std::env::args().any(|a| a == "--check-mic") {
        print!("{}", microphone_permission::check_status());
        std::process::exit(0);
    }

    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("gladiaflow".into()),
                    }),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                ])
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            app.manage(AppState {
                gladia: Arc::new(TokioMutex::new(GladiaClient::new())),
                audio: Arc::new(AudioCapture::new()),
                audio_lifecycle: Arc::new(TokioMutex::new(())),
                hotkey_manager: Arc::new(TokioMutex::new(HotkeyManager::new())),
                recording_state: Arc::new(TokioMutex::new(RecordingState::default())),
                subscription_active: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                audio_sender: Arc::new(TokioMutex::new(None)),
                tray_animation: Arc::new(TokioMutex::new(None)),
                region: region::detect_region(),
                session_screen_terms: Arc::new(std::sync::Mutex::new(Vec::new())),
            });

            // Warm OpenPII NER off the UI thread (MLX sidecar preferred on macOS,
            // ONNX/ort fallback) so the first dictation harvest stays fast.
            if screen_context::resolve_mlx_model_dir().is_some()
                || screen_context::resolve_model_dir().is_some()
            {
                std::thread::Builder::new()
                    .name("openpii-ner-warmup".into())
                    .spawn(|| screen_context::warmup())
                    .ok();
            }

            // Rolling on-screen context: sample AX/titles into the vocabulary store.
            screen_context::start_background_sampler_if_enabled();

            let current_version = env!("CARGO_PKG_VERSION");
            let installed = match config::get_installed_version() {
                Ok(installed) => Some(installed),
                Err(error) => {
                    log::error!(
                        "[config] failed to load installed version; skipping config migrations: {error}"
                    );
                    None
                }
            };
            let version_changed = installed
                .as_ref()
                .is_some_and(|installed| installed.as_deref() != Some(current_version));

            // One-time TCC cleanup for legacy bundle ids / signing migrations (FDE-147).
            // Does NOT run on every version bump — stable signing keeps the grant alive.
            #[cfg(target_os = "macos")]
            {
                if let Err(e) = permissions::accessibility::maybe_run_migration_reset() {
                    log::warn!("Accessibility migration reset failed: {e}");
                }
                if version_changed {
                    log::info!(
                        "Post-update accessibility re-validation (v{} -> v{current_version})",
                        installed
                            .as_ref()
                            .and_then(|installed| installed.as_deref())
                            .unwrap_or("none")
                    );
                }
                let _ = permissions::accessibility::check_and_log_state();
            }

            if version_changed {
                match config::seed_default_vocabulary_if_empty() {
                    Ok(true) => log::info!(
                        "Seeded default custom vocabulary ({}) on upgrade to {current_version}",
                        vocabulary::DEFAULT_VOCAB_TERM
                    ),
                    Ok(false) => {}
                    Err(e) => log::warn!("Failed to seed default custom vocabulary: {e}"),
                }

                if let Err(e) = config::set_installed_version(current_version) {
                    log::warn!("Failed to persist installed version: {e}");
                }
            }

            let show_i = MenuItem::with_id(app, "show", "Open App", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

            #[cfg(all(target_os = "macos", debug_assertions))]
            let reset_ax_i = MenuItem::with_id(
                app,
                "reset_ax",
                "Reset Accessibility Permission (dev)",
                true,
                None::<&str>,
            )?;

            #[cfg(all(target_os = "macos", debug_assertions))]
            let menu = Menu::with_items(app, &[&show_i, &reset_ax_i, &quit_i])?;
            #[cfg(not(all(target_os = "macos", debug_assertions)))]
            let menu = Menu::with_items(app, &[&show_i, &quit_i])?;

            let idle_icon = tray_icon_from_bytes(TRAY_ICON_IDLE)?;
            let _tray = TrayIconBuilder::with_id(TRAY_ID)
                .icon(idle_icon)
                .tooltip("GladiaFlow")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        show_main_window(app);
                    }
                    #[cfg(all(target_os = "macos", debug_assertions))]
                    "reset_ax" => match permissions::accessibility::dev_reset_tcc() {
                        Ok(state) => log::info!("[accessibility] dev reset -> {:?}", state),
                        Err(e) => log::warn!("[accessibility] dev reset failed: {e}"),
                    },
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        show_main_window(app);
                    }
                })
                .build(app)?;

            // Intercept window close: hide instead of destroying
            if let Some(window) = app.get_webview_window("main") {
                let w = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = w.hide();
                        #[cfg(target_os = "macos")]
                        let _ = w
                            .app_handle()
                            .set_activation_policy(tauri::ActivationPolicy::Accessory);
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            config::save_api_key,
            config::get_api_key,
            config::delete_api_key,
            config::reset_corrupted_config,
            init_gladia_session,
            test_gladia_connection,
            list_transcription_history,
            open_external_url,
            send_audio_chunk,
            stop_recording,
            request_notification_permission,
            close_gladia_session,
            subscribe_to_transcriptions,
            list_audio_devices,
            list_audio_device_info,
            prepare_audio_capture,
            start_audio_capture,
            stop_audio_capture,
            stop_dictation,
            get_recording_state,
            set_recording_state,
            get_region,
            open_log_folder,
            get_platform,
            show_window,
            hide_window,
            minimize_window,
            config::save_hotkey,
            config::get_hotkey,
            config::save_language_settings,
            config::get_language_settings,
            config::save_endpointing,
            config::get_endpointing,
            config::save_copy_to_clipboard,
            config::get_copy_to_clipboard,
            config::save_use_screen_context_vocabulary,
            config::get_use_screen_context_vocabulary,
            config::save_use_screen_context_ocr,
            config::get_use_screen_context_ocr,
            config::get_screen_context_onboarding_done,
            config::complete_screen_context_onboarding,
            config::save_activation_mode,
            config::get_activation_mode,
            config::save_audio_device_selection,
            config::get_audio_device_selection,
            vocabulary::save_custom_vocabulary,
            vocabulary::get_custom_vocabulary,
            vocabulary::export_vocabulary_csv,
            vocabulary::import_vocabulary_csv,
            register_dictation_hotkey,
            unregister_dictation_hotkey,
            get_accessibility_state,
            dev_reset_accessibility_tcc,
            open_system_settings,
            check_microphone_permission,
            request_microphone_permission,
            set_tray_activity,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
