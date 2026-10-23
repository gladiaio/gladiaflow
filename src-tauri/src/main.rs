#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{async_runtime::spawn, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::Mutex as TokioMutex;
use url::Url;

mod audio;
mod config;
mod gladia;
mod history;
mod hotkey;
mod hotkey_layout;
mod keystrokes;
mod live_typing;
mod meeting_audio;
mod permissions;
mod region;
mod system_audio;
mod system_calendar;
mod utterance_cleaner;
mod vocabulary;

use audio::{AudioCapture, AudioDeviceInfo, AudioDeviceSelection};
use gladia::GladiaClient;
use history::TranscriptionHistoryPage;
use hotkey::HotkeyManager;
use meeting_audio::{MeetingAudioCapture, MEETING_SAMPLE_RATE};
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

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct MeetingTranscriptEvent {
    id: Option<String>,
    text: String,
    channel: u32,
    owner: bool,
    speaker: Option<u32>,
    is_final: bool,
    start: f64,
    end: f64,
}

#[derive(Debug, PartialEq)]
struct MeetingSpeakerTurn {
    text: String,
    speaker: Option<u32>,
    start: f64,
    end: f64,
}

fn meeting_speaker_turns(
    text: String,
    speaker: Option<u32>,
    start: f64,
    end: f64,
    words: Vec<gladia::TranscriptWord>,
) -> Vec<MeetingSpeakerTurn> {
    if words.is_empty() || words.iter().all(|word| word.speaker.is_none()) {
        return vec![MeetingSpeakerTurn {
            text,
            speaker,
            start,
            end,
        }];
    }

    let mut turns = Vec::<MeetingSpeakerTurn>::new();
    for word in words {
        let word_speaker = word
            .speaker
            .or(speaker)
            .or_else(|| turns.last().and_then(|turn| turn.speaker));
        if let Some(turn) = turns.last_mut().filter(|turn| turn.speaker == word_speaker) {
            turn.text.push(' ');
            turn.text.push_str(&word.word);
            turn.end = word.end.unwrap_or(turn.end);
        } else {
            turns.push(MeetingSpeakerTurn {
                text: word.word,
                speaker: word_speaker,
                start: word.start.unwrap_or(start),
                end: word.end.unwrap_or(end),
            });
        }
    }
    turns
}

fn meeting_partial_speaker(channel: u32, last_remote_speaker: Option<u32>) -> Option<u32> {
    if channel == 1 {
        last_remote_speaker
    } else {
        None
    }
}

#[cfg(test)]
mod meeting_speaker_turn_tests {
    use super::{
        gladia::TranscriptWord, meeting_partial_speaker, meeting_speaker_turns, MeetingSpeakerTurn,
    };

    fn word(text: &str, speaker: Option<u32>, start: f64, end: f64) -> TranscriptWord {
        TranscriptWord {
            word: text.to_string(),
            start: Some(start),
            end: Some(end),
            speaker,
        }
    }

    #[test]
    fn splits_a_remote_final_at_word_level_speaker_changes() {
        let turns = meeting_speaker_turns(
            "Hello. Yes, welcome.".to_string(),
            Some(1),
            1.0,
            2.0,
            vec![
                word("Hello.", Some(0), 1.0, 1.3),
                word("Yes,", Some(1), 1.4, 1.6),
                word("welcome.", Some(1), 1.7, 2.0),
            ],
        );

        assert_eq!(
            turns,
            vec![
                MeetingSpeakerTurn {
                    text: "Hello.".to_string(),
                    speaker: Some(0),
                    start: 1.0,
                    end: 1.3,
                },
                MeetingSpeakerTurn {
                    text: "Yes, welcome.".to_string(),
                    speaker: Some(1),
                    start: 1.4,
                    end: 2.0,
                },
            ]
        );
    }

    #[test]
    fn keeps_the_utterance_when_word_labels_are_unavailable() {
        assert_eq!(
            meeting_speaker_turns("Still live".to_string(), Some(2), 3.0, 4.0, vec![]),
            vec![MeetingSpeakerTurn {
                text: "Still live".to_string(),
                speaker: Some(2),
                start: 3.0,
                end: 4.0,
            }]
        );
    }

    #[test]
    fn partials_keep_the_last_confirmed_remote_speaker() {
        assert_eq!(meeting_partial_speaker(1, Some(2)), Some(2));
        assert_eq!(meeting_partial_speaker(1, None), None);
        assert_eq!(meeting_partial_speaker(0, Some(2)), None);
    }
}

pub struct AppState {
    pub gladia: Arc<TokioMutex<GladiaClient>>,
    pub audio: Arc<AudioCapture>,
    pub audio_lifecycle: Arc<TokioMutex<()>>,
    pub capture_mode: Arc<TokioMutex<Option<&'static str>>>,
    pub hotkey_manager: Arc<TokioMutex<HotkeyManager>>,
    pub recording_state: Arc<TokioMutex<RecordingState>>,
    pub subscription_active: Arc<std::sync::atomic::AtomicBool>,
    /// Set while a dictation runs, cleared by `stop_dictation`: in
    /// push-to-talk, whether the trigger key is still down.
    pub dictation_key_held: Arc<std::sync::atomic::AtomicBool>,
    pub audio_sender: Arc<TokioMutex<Option<tokio::task::JoinHandle<()>>>>,
    pub meeting: Arc<MeetingAudioCapture>,
    pub meeting_sender: Arc<TokioMutex<Option<tokio::task::JoinHandle<()>>>>,
    pub tray_animation: Arc<TokioMutex<Option<tauri::async_runtime::JoinHandle<()>>>>,
    pub region: &'static str,
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
    let endpoint = gladia::ApiEndpoint::new(config::api_base_url()?, state.region);
    let model = config::live_model()?;
    log::info!(
        "initialising Gladia session (region={}, sample_rate={}, code_switching={}, api_base_url={}, model={})",
        endpoint.region.as_deref().unwrap_or("from host"),
        sample_rate,
        code_switching.unwrap_or(false),
        endpoint.base_url,
        model.as_deref().unwrap_or("default")
    );
    let gladia = state.gladia.lock().await;
    gladia
        .set_audio_format(sample_rate, 1, 16)
        .await
        .map_err(|error| error.to_string())?;
    let custom_vocabulary = custom_vocabulary
        .filter(|entries| !entries.is_empty())
        .or_else(|| config::get_custom_vocabulary().ok())
        .unwrap_or_default();
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
            model,
            false,
            &endpoint,
        )
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn init_meeting_session(
    api_key: String,
    languages: Option<Vec<String>>,
    code_switching: Option<bool>,
    custom_vocabulary: Option<Vec<CustomVocabEntry>>,
    endpointing: Option<f64>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let endpoint = gladia::ApiEndpoint::new(config::api_base_url()?, state.region);
    let model = config::live_model()?;
    let custom_vocabulary = custom_vocabulary
        .filter(|entries| !entries.is_empty())
        .or_else(|| config::get_custom_vocabulary().ok())
        .unwrap_or_default();
    let endpointing = endpointing.unwrap_or(config::endpointing()?);
    let gladia = state.gladia.lock().await;
    gladia
        .set_audio_format(MEETING_SAMPLE_RATE, 2, 16)
        .await
        .map_err(|error| error.to_string())?;
    gladia
        .init_session(
            &api_key,
            languages,
            code_switching.unwrap_or(false),
            custom_vocabulary,
            endpointing,
            model,
            true,
            &endpoint,
        )
        .await
        .map_err(|error| error.to_string())
}

/// The live models the current API base URL offers `api_key`.
#[tauri::command]
async fn list_live_models(
    api_key: String,
    state: State<'_, AppState>,
) -> Result<gladia::LiveModels, String> {
    let api_base_url = config::api_base_url()?;
    let gladia = state.gladia.lock().await.clone();
    gladia
        .list_live_models(&api_key, &api_base_url)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn test_gladia_connection(
    api_key: String,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let api_base_url = config::api_base_url()?;
    let gladia = state.gladia.lock().await;
    gladia
        .test_connection(&api_key, &api_base_url)
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
fn list_meetings() -> Result<Vec<history::MeetingRecord>, String> {
    history::list_meetings()
}

#[tauri::command]
fn get_meeting(id: String) -> Result<Option<history::MeetingRecord>, String> {
    history::get_meeting(&id)
}

#[tauri::command]
fn update_meeting(
    id: String,
    update: history::MeetingUpdate,
) -> Result<history::MeetingRecord, String> {
    history::update_meeting(&id, update)
}

#[tauri::command]
fn delete_meeting(id: String) -> Result<bool, String> {
    history::delete_meeting(&id)
}

#[tauri::command]
fn export_meeting_markdown(id: String) -> Result<String, String> {
    history::meeting_markdown(&id)
}

#[tauri::command]
async fn save_meeting_markdown(id: String, app: tauri::AppHandle) -> Result<bool, String> {
    let markdown = history::meeting_markdown(&id)?;
    let mut builder = app
        .dialog()
        .file()
        .set_file_name("meeting-notes.md")
        .add_filter("Markdown", &["md"]);
    if let Some(downloads) = dirs::download_dir() {
        builder = builder.set_directory(downloads);
    }
    match builder.blocking_save_file() {
        Some(file_path) => {
            let path = file_path.into_path().map_err(|error| error.to_string())?;
            std::fs::write(path, markdown).map_err(|error| error.to_string())?;
            Ok(true)
        }
        None => Ok(false),
    }
}

#[tauri::command]
async fn get_calendar_meetings(
    request_access: bool,
) -> Result<system_calendar::CalendarSnapshot, String> {
    tokio::task::spawn_blocking(move || system_calendar::meetings(request_access))
        .await
        .map_err(|error| format!("Failed to join Calendar task: {error}"))?
}

#[tauri::command]
fn open_calendar_meeting_url(url: String) -> Result<(), String> {
    let parsed = Url::parse(&url).map_err(|_| "Invalid meeting URL".to_string())?;
    if parsed.scheme() != "https" || parsed.host_str().is_none() {
        return Err("Only HTTPS meeting links can be opened".to_string());
    }
    open::that_detached(parsed.as_str()).map_err(|error| error.to_string())
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
    let key_held = state.dictation_key_held.clone();
    key_held.store(true, Ordering::SeqCst);
    spawn(async move {
        let mut cleaner = utterance_cleaner::UtteranceCleaner::new();
        // When enabled, the final transcription is left on the clipboard at
        // session end instead of restoring the user's original clipboard.
        let copy_to_clipboard = config::copy_to_clipboard().unwrap_or_else(|error| {
            log::error!("[config] failed to load copy-to-clipboard setting; using false: {error}");
            false
        });
        let live = config::live_typing().unwrap_or_else(|error| {
            log::error!("[config] failed to load live-typing setting; using false: {error}");
            false
        });
        let mut final_transcript: Option<String> = None;

        // Live typing: a dedicated thread types the newest target (finals so
        // far + the open partial) and never touches the clipboard.
        let mut provisional = live_typing::Provisional::default();
        let (live_tx, live_worker_handle) = if live {
            // On Windows, keys sent while a held modifier is down combine
            // with it, so edits wait for the trigger key's release.
            let hold_edits = cfg!(target_os = "windows")
                && config::held_hotkey()
                    .ok()
                    .flatten()
                    .is_some_and(|hotkey| live_typing::hotkey_holds_modifier(&hotkey));
            log::info!("[live-typing] on (edits wait for key release: {hold_edits})");
            let (tx, rx) = std::sync::mpsc::channel::<live_typing::Target>();
            let handle = std::thread::spawn(move || {
                let held = hold_edits.then_some(&*key_held);
                live_typing::run(rx, &mut live_typing::SystemKeyboard, held)
            });
            (Some(tx), Some(handle))
        } else {
            (None, None)
        };

        // Otherwise a dedicated worker thread serialises pastes so the recv
        // loop never blocks on the ~250ms paste round-trip and fragments land
        // in order. The worker restores the user's clipboard once, after it
        // drains.
        let (paste_tx, paste_worker_handle) = if live {
            (None, None)
        } else {
            // Snapshot the user's clipboard once so it can be restored at session end.
            let original_clipboard = clipboard_get();
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
                    if let Err(e) = keystrokes::send_paste() {
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
            (Some(tx), Some(handle))
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
                gladia::TranscriptionEvent::Partial {
                    id,
                    text,
                    channel: _,
                    start,
                    end,
                    speaker: _,
                    words: _,
                } => {
                    partial_count += 1;
                    if partial_count == 1 {
                        log::info!(
                            "[transcription] first partial received ({} chars)",
                            text.chars().count()
                        );
                    }
                    if let Some(tx) = &live_tx {
                        if provisional.partial(id, &text, start, end) {
                            let _ = tx.send(live_typing::Target::new(&cleaner, &provisional));
                        }
                    }
                    let _ = app_handle.emit("transcription-partial", text);
                }
                gladia::TranscriptionEvent::Final {
                    id,
                    text,
                    channel: _,
                    start,
                    end,
                    speaker: _,
                    words: _,
                } => {
                    final_count += 1;
                    // Live typing keeps the sentence boundary it already
                    // typed when the partial carried no start of its own.
                    let clean_start = if live_tx.is_some() {
                        provisional.final_start(id.as_ref(), start, cleaner.last_final_end())
                    } else {
                        start
                    };
                    let fragment = cleaner.process_final(&text, clean_start, end);
                    log::info!(
                        "[transcription] final #{final_count} [{start:.2}s-{end:.2}s] ({} chars, transcript now {} chars)",
                        text.chars().count(),
                        cleaner.accumulated_text().chars().count()
                    );
                    if let Some(tx) = &live_tx {
                        provisional.finalized(id);
                        let _ = tx.send(live_typing::Target::new(&cleaner, &provisional));
                    } else if let (Some(tx), Some(frag)) = (&paste_tx, fragment) {
                        // Paste just the newly appended fragment as we go.
                        let _ = tx.send(frag);
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

                    // Queue the deferred final period before signaling the frontend
                    // (live typing gets it with the last target, below).
                    if let (Some(tx), Some(p)) = (&paste_tx, last_period) {
                        let _ = tx.send(p.to_string());
                    }

                    // Gladia has finalized; the frontend can close the session while
                    // the paste worker continues draining in the background.
                    let _ = app_handle.emit("session-ended", full_text.clone());
                    break;
                }
            }
        }
        // Live typing ends on the cleaned transcript: a partial left without
        // its final (session error, lost connection) is erased.
        if let Some(tx) = live_tx {
            provisional.clear();
            let _ = tx.send(live_typing::Target::new(&cleaner, &provisional));
        }
        // Drop the sender so the paste worker drains its queue and restores
        // the clipboard, including when the loop ends without a SessionEnded event.
        drop(paste_tx);
        if let Some(handle) = paste_worker_handle {
            if let Err(e) = tokio::task::spawn_blocking(move || {
                if let Err(join_err) = handle.join() {
                    log::warn!("[paste] worker thread panicked: {join_err:?}");
                }
            })
            .await
            {
                log::warn!("[paste] failed to join paste worker: {e}");
            }
        }
        let mut live_typing_incomplete = false;
        if let Some(handle) = live_worker_handle {
            match tokio::task::spawn_blocking(move || handle.join()).await {
                Ok(Ok(report)) => {
                    log::info!(
                        "[live-typing] {} targets, {} edits, {} backspaces, {} typed",
                        report.targets,
                        report.edits,
                        report.backspaces,
                        report.typed
                    );
                    if let Some(reason) = report.stopped {
                        log::warn!("[live-typing] stopped early: {reason}");
                        live_typing_incomplete = true;
                    }
                }
                Ok(Err(join_err)) => {
                    log::warn!("[live-typing] worker thread panicked: {join_err:?}");
                    live_typing_incomplete = true;
                }
                Err(e) => log::warn!("[live-typing] failed to join worker: {e}"),
            }
        }
        // The worker has restored the original clipboard by now; if the user
        // opted in, overwrite it with the final transcription instead. When
        // live typing stopped early, the focused app is missing part of the
        // dictation, so the whole of it is left on the clipboard.
        if copy_to_clipboard || live_typing_incomplete {
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
async fn subscribe_to_meeting_transcriptions(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    if state
        .subscription_active
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("Another transcription subscription is active".to_string());
    }
    let mut rx = state
        .gladia
        .lock()
        .await
        .subscribe_to_transcriptions()
        .await;
    let session_id = state.gladia.lock().await.current_session_id().await;
    let active = state.subscription_active.clone();

    spawn(async move {
        let mut utterances = Vec::<history::MeetingHistoryUtterance>::new();
        let mut last_remote_speaker = None;
        let mut partial_count = 0usize;
        let mut final_count = 0usize;
        log::info!("[meeting-transcription] subscription started");
        while let Ok(event) = rx.recv().await {
            match event {
                gladia::TranscriptionEvent::Partial {
                    id,
                    text,
                    channel,
                    start,
                    end,
                    speaker: _,
                    words: _,
                } => {
                    partial_count += 1;
                    let Some(channel @ 0..=1) = channel else {
                        let _ = app_handle.emit(
                            "meeting-transcription-error",
                            "Gladia returned a transcript without a valid meeting channel",
                        );
                        continue;
                    };
                    if partial_count == 1 {
                        log::info!(
                            "[meeting-transcription] first partial received on channel {channel} ({} chars)",
                            text.chars().count()
                        );
                    }
                    let _ = app_handle.emit(
                        "meeting-transcription",
                        MeetingTranscriptEvent {
                            id,
                            text,
                            channel,
                            owner: channel == 0,
                            speaker: meeting_partial_speaker(channel, last_remote_speaker),
                            is_final: false,
                            start,
                            end,
                        },
                    );
                }
                gladia::TranscriptionEvent::Final {
                    id,
                    text,
                    channel,
                    start,
                    end,
                    speaker,
                    words,
                } => {
                    final_count += 1;
                    let Some(channel @ 0..=1) = channel else {
                        let _ = app_handle.emit(
                            "meeting-transcription-error",
                            "Gladia returned a transcript without a valid meeting channel",
                        );
                        continue;
                    };
                    let text_length = text.chars().count();
                    let turns = if channel == 0 {
                        vec![MeetingSpeakerTurn {
                            text,
                            speaker: None,
                            start,
                            end,
                        }]
                    } else {
                        meeting_speaker_turns(text, speaker, start, end, words)
                    };
                    log::info!(
                        "[meeting-transcription] final #{final_count} on channel {channel} [{start:.2}s-{end:.2}s] ({text_length} chars, {} speaker turns)",
                        turns.len()
                    );
                    for (index, turn) in turns.into_iter().enumerate() {
                        let turn_id = if index == 0 {
                            id.clone()
                        } else {
                            id.as_ref().map(|id| format!("{id}:{index}"))
                        };
                        utterances.push(history::MeetingHistoryUtterance {
                            text: turn.text.clone(),
                            channel,
                            owner: channel == 0,
                            speaker: turn.speaker,
                            start: turn.start,
                            end: turn.end,
                        });
                        let _ = app_handle.emit(
                            "meeting-transcription",
                            MeetingTranscriptEvent {
                                id: turn_id,
                                text: turn.text,
                                channel,
                                owner: channel == 0,
                                speaker: turn.speaker,
                                is_final: true,
                                start: turn.start,
                                end: turn.end,
                            },
                        );
                        if channel == 1 && turn.speaker.is_some() {
                            last_remote_speaker = turn.speaker;
                        }
                    }
                }
                gladia::TranscriptionEvent::Error(message) => {
                    let _ = app_handle.emit("meeting-transcription-error", message);
                }
                gladia::TranscriptionEvent::SessionEnded => {
                    log::info!(
                        "[meeting-transcription] session ended: {partial_count} partials, {final_count} finals, {} stored utterances",
                        utterances.len()
                    );
                    utterances.sort_by(|left, right| {
                        left.start
                            .partial_cmp(&right.start)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                    if let Some(session_id) = session_id.as_deref() {
                        if let Err(error) = history::save_meeting_entry(session_id, &utterances) {
                            log::warn!("[history] meeting save failed: {error}");
                        }
                    }
                    let _ = app_handle.emit("meeting-session-ended", utterances.clone());
                    break;
                }
            }
        }
        active.store(false, Ordering::SeqCst);
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
    let mut capture_mode = state.capture_mode.lock().await;
    if capture_mode.is_some() {
        return Err("Another audio capture is already active".to_string());
    }

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
    *capture_mode = Some("dictation");
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

#[tauri::command]
async fn start_meeting_capture(
    device_name: Option<String>,
    device_selection: Option<AudioDeviceSelection>,
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let _lifecycle = state.audio_lifecycle.lock().await;
    let mut capture_mode = state.capture_mode.lock().await;
    if capture_mode.is_some() {
        return Err("Another audio capture is already active".to_string());
    }
    if let Some(previous_sender) = state.meeting_sender.lock().await.take() {
        if !previous_sender.is_finished() {
            *state.meeting_sender.lock().await = Some(previous_sender);
            return Err("Previous meeting capture is still shutting down".to_string());
        }
        let _ = previous_sender.await;
    }

    let selection = AudioDeviceSelection::from_ipc(device_selection, device_name);
    let meeting = state.meeting.clone();
    let (rx, error_rx, microphone_name) =
        tokio::task::spawn_blocking(move || meeting.start(selection))
            .await
            .map_err(|error| format!("Failed to join meeting capture task: {error}"))??;
    *capture_mode = Some("meeting");

    let error_app = app_handle.clone();
    tokio::task::spawn_blocking(move || {
        while let Ok(message) = error_rx.recv() {
            let event = if message.starts_with("System audio") {
                "meeting-audio-warning"
            } else {
                "meeting-transcription-error"
            };
            let _ = error_app.emit(event, message);
        }
    });
    let _ = app_handle.emit(
        "meeting-capture-ready",
        serde_json::json!({ "deviceName": microphone_name }),
    );

    let gladia = state.gladia.clone();
    let sender = tokio::spawn(async move {
        let (tx, mut async_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        tokio::task::spawn_blocking(move || {
            while let Ok(chunk) = rx.recv() {
                if tx.send(chunk).is_err() {
                    break;
                }
            }
        });
        let chunk_bytes = MEETING_SAMPLE_RATE as usize * 2 * 2 / 10;
        let mut buffer = Vec::with_capacity(chunk_bytes * 2);
        while let Some(chunk) = async_rx.recv().await {
            buffer.extend_from_slice(&chunk);
            while buffer.len() >= chunk_bytes {
                let chunk: Vec<u8> = buffer.drain(..chunk_bytes).collect();
                if let Err(error) = gladia.lock().await.send_audio(&chunk).await {
                    log::error!("failed to send meeting audio to Gladia: {error}");
                }
            }
        }
        if !buffer.is_empty() {
            let _ = gladia.lock().await.send_audio(&buffer).await;
        }
    });
    *state.meeting_sender.lock().await = Some(sender);
    Ok(())
}

async fn stop_and_drain_meeting(state: &AppState) -> Result<(), String> {
    let meeting = state.meeting.clone();
    tokio::task::spawn_blocking(move || meeting.stop())
        .await
        .map_err(|error| format!("Failed to join meeting cleanup task: {error}"))??;
    if let Some(mut sender) = state.meeting_sender.lock().await.take() {
        match tokio::time::timeout(Duration::from_secs(2), &mut sender).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) if error.is_cancelled() => {}
            Ok(Err(error)) => return Err(format!("Meeting sender task failed: {error}")),
            Err(_) => {
                sender.abort();
                let _ = sender.await;
                return Err("Timed out draining meeting audio".to_string());
            }
        }
    }
    Ok(())
}

#[tauri::command]
async fn pause_meeting_capture(state: State<'_, AppState>) -> Result<(), String> {
    let _lifecycle = state.audio_lifecycle.lock().await;
    if *state.capture_mode.lock().await != Some("meeting") {
        return Err("Meeting capture is not active".to_string());
    }
    let result = stop_and_drain_meeting(&state).await;
    *state.capture_mode.lock().await = None;
    result
}

#[tauri::command]
async fn stop_meeting(state: State<'_, AppState>) -> Result<(), String> {
    let _lifecycle = state.audio_lifecycle.lock().await;
    let result = stop_and_drain_meeting(&state).await;
    *state.capture_mode.lock().await = None;
    result?;
    state
        .gladia
        .lock()
        .await
        .stop_recording()
        .await
        .map_err(|error| error.to_string())
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
    let result = stop_and_drain_audio(&state).await;
    *state.capture_mode.lock().await = None;
    result
}

/// Stop audio capture AND send the stop_recording signal to Gladia in the
/// correct order: waits for the audio sender task to fully drain its buffer
/// before telling Gladia to finalise.
#[tauri::command]
async fn stop_dictation(state: State<'_, AppState>) -> Result<(), String> {
    let _lifecycle = state.audio_lifecycle.lock().await;
    log::info!("stopping dictation; draining audio and finalising Gladia session");
    state
        .dictation_key_held
        .store(false, std::sync::atomic::Ordering::SeqCst);
    let stop_result = stop_and_drain_audio(&state).await;
    *state.capture_mode.lock().await = None;
    stop_result?;

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
fn set_floating_window(
    app_handle: tauri::AppHandle,
    label: String,
    visible: bool,
) -> Result<(), String> {
    if !matches!(label.as_str(), "meeting-controller" | "meeting-reminder") {
        return Err("Unknown floating window".to_string());
    }
    let window = app_handle
        .get_webview_window(&label)
        .ok_or_else(|| format!("Floating window {label} is unavailable"))?;
    if !visible {
        return window.hide().map_err(|error| error.to_string());
    }

    window
        .set_always_on_top(true)
        .map_err(|error| error.to_string())?;
    #[cfg(target_os = "macos")]
    window
        .set_visible_on_all_workspaces(true)
        .map_err(|error| error.to_string())?;

    if let Some(monitor) = window
        .current_monitor()
        .map_err(|error| error.to_string())?
        .or_else(|| app_handle.primary_monitor().ok().flatten())
    {
        let monitor_position = monitor.position();
        let monitor_size = monitor.size();
        let window_size = window.outer_size().map_err(|error| error.to_string())?;
        let scale = monitor.scale_factor();
        let margin = (24.0 * scale).round() as i32;
        let dock_clearance = (96.0 * scale).round() as i32;
        let (x, y) = if label == "meeting-controller" {
            (
                monitor_position.x + margin,
                monitor_position.y + monitor_size.height as i32
                    - window_size.height as i32
                    - dock_clearance,
            )
        } else {
            (
                monitor_position.x + (monitor_size.width as i32 - window_size.width as i32) / 2,
                monitor_position.y + 48,
            )
        };
        window
            .set_position(tauri::PhysicalPosition::new(x, y))
            .map_err(|error| error.to_string())?;
    }
    window.show().map_err(|error| error.to_string())
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
            "screen_recording" => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
            }
            "audio_capture" => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_AudioCapture"
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

#[tauri::command]
async fn check_system_audio_capability() -> Result<system_audio::SystemAudioCapability, String> {
    Ok(system_audio::capability())
}

#[tauri::command]
async fn request_system_audio_permission() -> Result<system_audio::SystemAudioCapability, String> {
    tokio::task::spawn_blocking(system_audio::request_permission)
        .await
        .map_err(|e| e.to_string())?
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
            let audio = Arc::new(AudioCapture::new());
            app.manage(AppState {
                gladia: Arc::new(TokioMutex::new(GladiaClient::new())),
                meeting: Arc::new(MeetingAudioCapture::new(audio.clone())),
                audio,
                audio_lifecycle: Arc::new(TokioMutex::new(())),
                capture_mode: Arc::new(TokioMutex::new(None)),
                hotkey_manager: Arc::new(TokioMutex::new(HotkeyManager::new())),
                recording_state: Arc::new(TokioMutex::new(RecordingState::default())),
                subscription_active: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                dictation_key_held: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                audio_sender: Arc::new(TokioMutex::new(None)),
                meeting_sender: Arc::new(TokioMutex::new(None)),
                tray_animation: Arc::new(TokioMutex::new(None)),
                region: region::detect_region(),
            });

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
            init_meeting_session,
            test_gladia_connection,
            list_live_models,
            list_transcription_history,
            list_meetings,
            get_meeting,
            update_meeting,
            delete_meeting,
            export_meeting_markdown,
            save_meeting_markdown,
            get_calendar_meetings,
            open_calendar_meeting_url,
            open_external_url,
            send_audio_chunk,
            stop_recording,
            request_notification_permission,
            close_gladia_session,
            subscribe_to_transcriptions,
            subscribe_to_meeting_transcriptions,
            list_audio_devices,
            list_audio_device_info,
            prepare_audio_capture,
            start_audio_capture,
            stop_audio_capture,
            stop_dictation,
            start_meeting_capture,
            pause_meeting_capture,
            stop_meeting,
            get_recording_state,
            set_recording_state,
            get_region,
            open_log_folder,
            get_platform,
            show_window,
            hide_window,
            minimize_window,
            set_floating_window,
            config::save_hotkey,
            config::get_hotkey,
            config::save_language_settings,
            config::get_language_settings,
            config::save_endpointing,
            config::get_endpointing,
            config::save_copy_to_clipboard,
            config::get_copy_to_clipboard,
            config::save_live_typing,
            config::get_live_typing,
            config::save_activation_mode,
            config::get_activation_mode,
            config::get_api_base_url_settings,
            config::save_api_base_url,
            config::get_live_model,
            config::save_live_model,
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
            check_system_audio_capability,
            request_system_audio_permission,
            set_tray_activity,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
