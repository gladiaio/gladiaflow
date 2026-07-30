use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::{Mutex as TokioMutex, broadcast};
use tokio_tungstenite::tungstenite::protocol::Message;
use futures_util::{StreamExt, SinkExt};
use tokio::task::JoinHandle;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use crate::vocabulary::{CustomVocabEntry, expand_vocabulary_for_languages, normalize_vocabulary};

const MAX_PENDING_AUDIO_BYTES: usize = 96_000; // ~3s at 16kHz/16-bit/mono

#[derive(Debug, Default)]
struct PendingAudioBuffer {
    chunks: VecDeque<Vec<u8>>,
    total_bytes: usize,
}

const EUROPEAN_LANGUAGE_CODES: &[&str] = &[
    "en", "es", "fr", "de", "it", "pt", "nl", "pl", "cs", "da", "sv", "no", "fi", "ro", "hu",
    "el", "tr", "uk", "ru",
];

fn resolve_session_languages(languages: Option<Vec<String>>) -> Vec<String> {
    if let Some(selected_languages) = languages {
        let filtered = selected_languages
            .into_iter()
            .filter(|lang| !lang.trim().is_empty())
            .collect::<Vec<String>>();
        if !filtered.is_empty() {
            return filtered;
        }
    }
    EUROPEAN_LANGUAGE_CODES
        .iter()
        .map(|lang| (*lang).to_string())
        .collect()
}

#[derive(Debug, Clone)]
pub struct GladiaConfig {
    pub encoding: String,
    pub sample_rate: u32,
    pub bit_depth: u32,
    pub channels: u32,
    pub receive_partial: bool,
    pub receive_final: bool,
    pub languages: Vec<String>,
    pub code_switching: bool,
    pub endpointing: f64,
    pub custom_vocabulary: Vec<CustomVocabEntry>,
}

impl Default for GladiaConfig {
    fn default() -> Self {
        Self {
            encoding: "wav/pcm".to_string(),
            sample_rate: 16000,
            bit_depth: 16,
            channels: 1,
            receive_partial: true,
            receive_final: true,
            languages: vec![],
            code_switching: false,
            endpointing: 0.6,
            custom_vocabulary: vec![],
        }
    }
}

#[derive(Debug, Clone)]
pub struct GladiaClient {
    api_key: Arc<TokioMutex<Option<String>>>,
    ws_url: Arc<TokioMutex<Option<String>>>,
    session_id: Arc<TokioMutex<Option<String>>>,
    config: Arc<TokioMutex<GladiaConfig>>,
    ws_tx: Arc<TokioMutex<Option<tokio::sync::mpsc::UnboundedSender<Message>>>>,
    pending_audio: Arc<TokioMutex<PendingAudioBuffer>>,
    transcription_tx: Arc<TokioMutex<Option<broadcast::Sender<TranscriptionEvent>>>>,
    task_handle: Arc<TokioMutex<Option<JoinHandle<()>>>>,
}

#[derive(Debug, Clone, Serialize)]
struct SessionRequest {
    encoding: String,
    sample_rate: u32,
    bit_depth: u32,
    channels: u32,
    endpointing: f64,
    messages_config: MessagesConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    language_config: Option<LanguageConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    realtime_processing: Option<RealtimeProcessing>,
}

#[derive(Debug, Clone, Serialize)]
struct RealtimeProcessing {
    custom_vocabulary: bool,
    custom_vocabulary_config: CustomVocabularyConfig,
}

#[derive(Debug, Clone, Serialize)]
struct CustomVocabularyConfig {
    vocabulary: Vec<CustomVocabEntry>,
}

#[derive(Debug, Clone, Serialize)]
struct MessagesConfig {
    receive_partial_transcripts: bool,
    receive_final_transcripts: bool,
}

#[derive(Debug, Clone, Serialize)]
struct LanguageConfig {
    languages: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    code_switching: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct SessionResponse {
    id: String,
    url: String,
}

#[derive(Debug, Deserialize)]
struct ServerMessage {
    #[serde(rename = "type")]
    #[allow(dead_code)]
    message_type: String,
    #[serde(default)]
    data: Option<TranscriptionData>,
}

#[derive(Debug, Deserialize)]
struct TranscriptionData {
    #[serde(default)]
    is_final: bool,
    #[serde(default)]
    utterance: Option<Utterance>,
}

#[derive(Debug, Deserialize)]
struct Utterance {
    text: String,
    #[serde(default)]
    start: Option<f64>,
    #[serde(default)]
    end: Option<f64>,
    #[serde(default)]
    #[allow(dead_code)]
    speaker: Option<u32>,
    #[serde(default)]
    #[allow(dead_code)]
    confidence: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub enum TranscriptionEvent {
    Partial(String),
    Final { text: String, start: f64, end: f64 },
    /// The session ended abnormally (network drop, protocol error). Carries a
    /// human-readable reason so the frontend can surface it. A normal,
    /// server-initiated close emits `SessionEnded` only — never `Error`.
    Error(String),
    SessionEnded,
}

#[derive(Debug, thiserror::Error)]
pub enum GladiaError {
    #[error("API key not set")]
    ApiKeyNotSet,
    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),
    #[error("WebSocket error: {0}")]
    WebSocketError(String),
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
    #[error("Session not initialized")]
    SessionNotInitialized,
}

impl From<GladiaError> for String {
    fn from(error: GladiaError) -> Self {
        error.to_string()
    }
}

impl GladiaClient {
    pub fn new() -> Self {
        Self {
            api_key: Arc::new(TokioMutex::new(None)),
            ws_url: Arc::new(TokioMutex::new(None)),
            session_id: Arc::new(TokioMutex::new(None)),
            config: Arc::new(TokioMutex::new(GladiaConfig::default())),
            ws_tx: Arc::new(TokioMutex::new(None)),
            pending_audio: Arc::new(TokioMutex::new(PendingAudioBuffer::default())),
            transcription_tx: Arc::new(TokioMutex::new(None)),
            task_handle: Arc::new(TokioMutex::new(None)),
        }
    }

    pub async fn init_session(
        &self,
        api_key: &str,
        languages: Option<Vec<String>>,
        code_switching: bool,
        custom_vocabulary: Vec<CustomVocabEntry>,
        endpointing: f64,
        region: &str,
    ) -> Result<String, GladiaError> {
        let normalized_vocabulary = normalize_vocabulary(custom_vocabulary);
        let resolved_languages = resolve_session_languages(languages.clone());

        // Guard: reuse an existing live session only when config is unchanged.
        {
            let config = self.config.lock().await;
            let session_unchanged = config.custom_vocabulary == normalized_vocabulary
                && config.languages == resolved_languages
                && config.code_switching == code_switching
                && config.endpointing == endpointing;
            drop(config);

            if session_unchanged {
                let existing = self.session_id.lock().await;
                if let Some(ref id) = *existing {
                    let alive = self.task_handle.lock().await
                        .as_ref()
                        .map(|h| !h.is_finished())
                        .unwrap_or(false);
                    if alive {
                        return Ok(id.clone());
                    }
                }
            } else {
                self.close_session().await?;
            }
        }

        *self.api_key.lock().await = Some(api_key.to_string());

        // Build the request body then release the lock before any awaits.
        let request = {
            let mut config = self.config.lock().await;
            config.code_switching = code_switching;
            config.languages = resolved_languages.clone();
            config.custom_vocabulary = normalized_vocabulary.clone();
            config.endpointing = endpointing;
            let api_vocabulary =
                expand_vocabulary_for_languages(normalized_vocabulary, &config.languages);
            let realtime_processing = if api_vocabulary.is_empty() {
                None
            } else {
                Some(RealtimeProcessing {
                    custom_vocabulary: true,
                    custom_vocabulary_config: CustomVocabularyConfig {
                        vocabulary: api_vocabulary,
                    },
                })
            };
            SessionRequest {
                encoding: config.encoding.clone(),
                sample_rate: config.sample_rate,
                bit_depth: config.bit_depth,
                channels: config.channels,
                endpointing: config.endpointing,
                messages_config: MessagesConfig {
                    receive_partial_transcripts: config.receive_partial,
                    receive_final_transcripts: config.receive_final,
                },
                language_config: if config.languages.is_empty() {
                    if config.code_switching {
                        Some(LanguageConfig {
                            languages: vec![],
                            code_switching: Some(true),
                        })
                    } else {
                        None
                    }
                } else {
                    Some(LanguageConfig {
                        languages: config.languages.clone(),
                        code_switching: if config.code_switching { Some(true) } else { None },
                    })
                },
                realtime_processing,
            }
        }; // config lock dropped here

        if let Some(ref realtime) = request.realtime_processing {
            let vocab = &realtime.custom_vocabulary_config.vocabulary;
            let unique_terms: std::collections::HashSet<&str> =
                vocab.iter().map(|entry| entry.value.as_str()).collect();
            let languages: std::collections::HashSet<&str> = vocab
                .iter()
                .filter_map(|entry| entry.language.as_deref())
                .collect();
            log::info!(
                "[gladiaflow] Live session custom vocabulary: {} entries ({} unique terms) across languages {:?}",
                vocab.len(),
                unique_terms.len(),
                languages
            );
        } else {
            log::info!("[gladiaflow] Live session request has no custom vocabulary");
        }

        let client = reqwest::Client::new();
        let response = client
            .post(format!("https://api.gladia.io/v2/live?region={region}"))
            .header("x-gladia-key", api_key)
            .header("x-gladia-version", format!("Gladiaflow/{}", env!("CARGO_PKG_VERSION")))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(GladiaError::WebSocketError(format!(
                "Failed to create session: {}",
                response.status()
            )));
        }

        let session: SessionResponse = response.json().await?;
        let session_id = session.id.clone();
        let ws_url = session.url.clone();

        log::info!("Gladia live session created (id={session_id})");

        *self.session_id.lock().await = Some(session_id.clone());
        *self.ws_url.lock().await = Some(ws_url);

        // Create broadcast channel for transcription events
        let (tx, _rx) = broadcast::channel::<TranscriptionEvent>(100);
        *self.transcription_tx.lock().await = Some(tx);

        // Start WebSocket connection
        self.start_websocket_task().await?;

        Ok(session_id)
    }

    pub async fn set_audio_format(
        &self,
        sample_rate: u32,
        channels: u32,
        bit_depth: u32,
    ) {
        let mut config = self.config.lock().await;
        config.sample_rate = sample_rate;
        config.channels = channels;
        config.bit_depth = bit_depth;
    }

    async fn start_websocket_task(&self) -> Result<(), GladiaError> {
        let ws_url = {
            let url = self.ws_url.lock().await.clone();
            url.ok_or(GladiaError::SessionNotInitialized)?
        };

        let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
            .await
            .map_err(|e| GladiaError::WebSocketError(e.to_string()))?;

        // Create channel for sending messages
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
        *self.ws_tx.lock().await = Some(tx);
        self.flush_pending_audio().await;

        // Split the stream
        let (mut sender, mut receiver) = ws_stream.split();

        let transcription_tx = self.transcription_tx.lock().await.clone();
        let session_id = self
            .session_id
            .lock()
            .await
            .clone()
            .unwrap_or_else(|| "unknown".to_string());

        log::info!("[gladia-ws] websocket connected (session id={session_id})");

        // Store the handle so we can abort the task if needed
        let handle = tokio::spawn(async move {
            // Reason for an abnormal termination, surfaced to the frontend as an
            // Error event. Stays None for a clean, server-initiated close.
            let mut error_reason: Option<String> = None;
            loop {
                tokio::select! {
                    result = receiver.next() => {
                        match result {
                            Some(Ok(Message::Text(text))) => {
                                if let Ok(server_msg) = serde_json::from_str::<ServerMessage>(&text) {
                                    if let Some(data) = server_msg.data {
                                        if let Some(utterance) = data.utterance {
                                            if let Some(tx) = transcription_tx.as_ref() {
                                                let event = if data.is_final {
                                                    TranscriptionEvent::Final {
                                                        text: utterance.text,
                                                        start: utterance.start.unwrap_or(-1.0),
                                                        end: utterance.end.unwrap_or(-1.0),
                                                    }
                                                } else {
                                                    TranscriptionEvent::Partial(utterance.text)
                                                };
                                                let _ = tx.send(event);
                                            }
                                        }
                                    }
                                } else {
                                    log::warn!("[gladia-ws] (session id={session_id}) unrecognised message: {text}");
                                }
                            }
                            Some(Ok(Message::Binary(_))) => {}
                            Some(Ok(Message::Ping(msg))) => {
                                let _ = sender.send(Message::Pong(msg)).await;
                            }
                            Some(Ok(Message::Close(frame))) => {
                                // Graceful close (normal stop, or a server-side
                                // session limit handled by reconnect upstream).
                                log::info!("[gladia-ws] (session id={session_id}) server closed: {frame:?}");
                                break;
                            }
                            Some(Err(e)) => {
                                log::error!("[gladia-ws] (session id={session_id}) websocket error: {e}");
                                error_reason = Some(format!("Connection error: {e}"));
                                break;
                            }
                            None => {
                                log::error!("[gladia-ws] (session id={session_id}) stream ended unexpectedly");
                                error_reason =
                                    Some("Connection to Gladia was lost unexpectedly".to_string());
                                break;
                            }
                            _ => {}
                        }
                    }
                    Some(msg) = rx.recv() => {
                        if let Err(e) = sender.send(msg).await {
                            log::error!("[gladia-ws] (session id={session_id}) failed to send to ws, closing: {e}");
                            error_reason = Some(format!("Failed to send audio to Gladia: {e}"));
                            break;
                        }
                    }
                    else => break,
                }
            }
            log::info!("[gladia-ws] (session id={session_id}) session ended");
            if let Some(tx) = transcription_tx.as_ref() {
                if let Some(reason) = error_reason {
                    let _ = tx.send(TranscriptionEvent::Error(reason));
                }
                let _ = tx.send(TranscriptionEvent::SessionEnded);
            }
        });
        
        *self.task_handle.lock().await = Some(handle);

        Ok(())
    }

    pub async fn test_connection(&self, api_key: &str) -> Result<bool, GladiaError> {
        let client = reqwest::Client::new();
        let response = client
            .get("https://api.gladia.io/v2/pre-recorded?limit=1")
            .header("x-gladia-key", api_key)
            .header("x-gladia-version", format!("Gladiaflow/{}", env!("CARGO_PKG_VERSION")))
            .send()
            .await?;

        Ok(response.status().is_success())
    }

    pub async fn current_session_id(&self) -> Option<String> {
        self.session_id.lock().await.clone()
    }

    pub async fn send_audio(&self, audio_data: &[u8]) -> Result<(), GladiaError> {
        if audio_data.is_empty() {
            return Ok(());
        }

        let sender = self.ws_tx.lock().await.clone();
        if let Some(tx) = sender {
            let payload = serde_json::json!({
                "type": "audio_chunk",
                "data": {
                    "chunk": STANDARD.encode(audio_data),
                }
            });

            tx.send(Message::Text(payload.to_string()))
                .map_err(|e| GladiaError::WebSocketError(e.to_string()))?;
        } else {
            self.enqueue_pending_audio(audio_data.to_vec()).await;
        }
        Ok(())
    }

    async fn enqueue_pending_audio(&self, chunk: Vec<u8>) {
        let mut pending = self.pending_audio.lock().await;
        pending.total_bytes += chunk.len();
        pending.chunks.push_back(chunk);

        while pending.total_bytes > MAX_PENDING_AUDIO_BYTES {
            if let Some(dropped) = pending.chunks.pop_front() {
                pending.total_bytes = pending.total_bytes.saturating_sub(dropped.len());
            } else {
                break;
            }
        }
    }

    async fn flush_pending_audio(&self) {
        let sender = self.ws_tx.lock().await.clone();
        let Some(tx) = sender else {
            return;
        };

        let queued_chunks = {
            let mut pending = self.pending_audio.lock().await;
            pending.total_bytes = 0;
            pending.chunks.drain(..).collect::<Vec<Vec<u8>>>()
        };

        for chunk in queued_chunks {
            let payload = serde_json::json!({
                "type": "audio_chunk",
                "data": {
                    "chunk": STANDARD.encode(&chunk),
                }
            });
            if tx.send(Message::Text(payload.to_string())).is_err() {
                break;
            }
        }
    }

    /// Signal Gladia to stop recording. The server will process remaining
    /// audio buffers, send final transcriptions, and then close the WebSocket.
    pub async fn stop_recording(&self) -> Result<(), GladiaError> {
        let sender = self.ws_tx.lock().await;
        if let Some(tx) = sender.as_ref() {
            let msg = serde_json::json!({"type": "stop_recording"});
            tx.send(Message::Text(msg.to_string()))
                .map_err(|e| GladiaError::WebSocketError(e.to_string()))?;
        }
        Ok(())
    }

    pub async fn subscribe_to_transcriptions(&self) -> broadcast::Receiver<TranscriptionEvent> {
        let tx = self.transcription_tx.lock().await;
        if let Some(ref tx) = *tx {
            tx.subscribe()
        } else {
            // Return a dummy receiver if no session
            let (_, rx) = broadcast::channel(1);
            rx
        }
    }

    pub async fn close_session(&self) -> Result<(), GladiaError> {
        // Drop the sender so the WS task's rx closes, then abort the task.
        // (stop_recording already sent the graceful stop before this is called.)
        *self.ws_tx.lock().await = None;

        if let Some(handle) = self.task_handle.lock().await.take() {
            handle.abort();
            // Wait for abort to complete; ignore the expected Cancelled error.
            let _ = handle.await;
        }

        *self.ws_url.lock().await = None;
        *self.session_id.lock().await = None;
        *self.pending_audio.lock().await = PendingAudioBuffer::default();
        *self.transcription_tx.lock().await = None;
        Ok(())
    }
}

impl Default for GladiaClient {
    fn default() -> Self {
        Self::new()
    }
}