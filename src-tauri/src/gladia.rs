use crate::config::DEFAULT_API_BASE_URL;
use crate::vocabulary::{expand_vocabulary_for_languages, normalize_vocabulary, CustomVocabEntry};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex as TokioMutex};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type WsStream = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

const PENDING_AUDIO_SECONDS: usize = 3;
/// Unacknowledged audio kept for replay after a reconnection.
const RESEND_AUDIO_SECONDS: usize = 60;
/// Reconnection attempts per drop, and their delays.
const RECONNECT_DELAYS_MS: [u64; 3] = [250, 1_000, 2_000];
/// Drops tolerated per session before giving up.
const MAX_RECONNECTS: u32 = 5;
/// Live models offered when a host can't list its own: those Gladia's global
/// API publishes for `POST /v2/live` (`StreamingSupportedModels`).
const FALLBACK_LIVE_MODELS: &[&str] = &["solaria-1"];

#[derive(Debug, Default)]
struct PendingAudioBuffer {
    chunks: VecDeque<Vec<u8>>,
    total_bytes: usize,
}

const EUROPEAN_LANGUAGE_CODES: &[&str] = &[
    "en", "es", "fr", "de", "it", "pt", "nl", "pl", "cs", "da", "sv", "no", "fi", "ro", "hu", "el",
    "tr", "uk", "ru",
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

/// Where live sessions are created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiEndpoint {
    pub base_url: String,
    /// `?region=` value. Only Gladia's global API takes one: on a regional
    /// host (e.g. `eu1.api.gladia.io`) the region is the address itself, and a
    /// region the host does not serve finds no model.
    pub region: Option<String>,
}

impl ApiEndpoint {
    pub fn new(base_url: String, detected_region: &str) -> Self {
        let region = (base_url == DEFAULT_API_BASE_URL).then(|| detected_region.to_string());
        Self { base_url, region }
    }

    fn live_url(&self) -> String {
        match &self.region {
            Some(region) => format!("{}/v2/live?region={region}", self.base_url),
            None => format!("{}/v2/live", self.base_url),
        }
    }
}

impl Default for ApiEndpoint {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_API_BASE_URL.to_string(),
            region: None,
        }
    }
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
    /// Enterprise live speaker diarization. Meeting sessions enable this so
    /// several remote voices mixed into the system-audio channel stay distinct.
    pub diarization: bool,
    pub custom_vocabulary: Vec<CustomVocabEntry>,
    /// Live model code name; `None` lets the host pick its default.
    pub model: Option<String>,
    pub endpoint: ApiEndpoint,
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
            diarization: false,
            custom_vocabulary: vec![],
            model: None,
            endpoint: ApiEndpoint::default(),
        }
    }
}

fn audio_bytes_for_seconds(config: &GladiaConfig, seconds: usize) -> usize {
    config.sample_rate as usize
        * config.channels as usize
        * (config.bit_depth as usize / 8)
        * seconds
}

/// What the client asks the WebSocket task to send.
#[derive(Debug)]
enum Outgoing {
    Audio(Vec<u8>),
    StopRecording,
}

#[derive(Debug, Clone)]
pub struct GladiaClient {
    api_key: Arc<TokioMutex<Option<String>>>,
    ws_url: Arc<TokioMutex<Option<String>>>,
    session_id: Arc<TokioMutex<Option<String>>>,
    config: Arc<TokioMutex<GladiaConfig>>,
    ws_tx: Arc<TokioMutex<Option<tokio::sync::mpsc::UnboundedSender<Outgoing>>>>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    diarization: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diarization_config: Option<DiarizationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    messages_config: MessagesConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    language_config: Option<LanguageConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    realtime_processing: Option<RealtimeProcessing>,
}

#[derive(Debug, Clone, Serialize)]
struct DiarizationConfig {
    max_speakers: u32,
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
    /// Needed to know which audio can be dropped from the resend buffer.
    receive_acknowledgments: bool,
    receive_errors: bool,
}

#[derive(Debug, Clone, Serialize)]
struct LanguageConfig {
    languages: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    code_switching: Option<bool>,
}

/// `GET /v2/live/capabilities`: the models `POST /v2/live` accepts for the key.
#[derive(Debug, Deserialize)]
struct LiveCapabilities {
    models: Vec<ModelCapabilities>,
}

#[derive(Debug, Deserialize)]
struct ModelCapabilities {
    model: String,
}

/// The live models a host offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveModels {
    pub models: Vec<String>,
    /// Whether the host listed them itself; otherwise they are
    /// `FALLBACK_LIVE_MODELS`.
    pub discovered: bool,
}

#[derive(Debug, Deserialize)]
struct SessionResponse {
    id: String,
    url: String,
}

#[derive(Debug, Deserialize)]
struct ServerMessage {
    #[serde(rename = "type")]
    message_type: String,
    #[serde(default)]
    acknowledged: Option<bool>,
    #[serde(default)]
    data: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct TranscriptionData {
    /// Utterance id (`"<channel>_<seq>"`), shared by its partials and final.
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    is_final: bool,
    #[serde(default)]
    utterance: Option<Utterance>,
}

#[derive(Debug, Deserialize)]
struct Utterance {
    text: String,
    #[serde(default)]
    channel: Option<u32>,
    #[serde(default)]
    start: Option<f64>,
    #[serde(default)]
    end: Option<f64>,
    #[serde(default)]
    speaker: Option<u32>,
    #[serde(default)]
    words: Vec<TranscriptWord>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct TranscriptWord {
    pub word: String,
    #[serde(default)]
    pub start: Option<f64>,
    #[serde(default)]
    pub end: Option<f64>,
    #[serde(default)]
    pub speaker: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct AudioChunkAckData {
    byte_range: Vec<u64>,
}

/// A server message, reduced to what the client acts on.
#[derive(Debug, PartialEq)]
enum Incoming {
    Transcript(TranscriptionEvent),
    /// Audio acknowledged up to this byte offset since the session start.
    Acknowledged(u64),
    Error(String),
    Ignored,
}

fn parse_server_message(text: &str) -> Result<Incoming, serde_json::Error> {
    let message: ServerMessage = serde_json::from_str(text)?;
    Ok(match message.message_type.as_str() {
        "transcript" => {
            let data: TranscriptionData = serde_json::from_value(message.data.unwrap_or_default())?;
            match data.utterance {
                Some(utterance) if data.is_final => {
                    Incoming::Transcript(TranscriptionEvent::Final {
                        id: data.id,
                        text: utterance.text,
                        channel: utterance.channel,
                        start: utterance.start.unwrap_or(-1.0),
                        end: utterance.end.unwrap_or(-1.0),
                        speaker: utterance.speaker,
                        words: utterance.words,
                    })
                }
                Some(utterance) => Incoming::Transcript(TranscriptionEvent::Partial {
                    id: data.id,
                    text: utterance.text,
                    channel: utterance.channel,
                    start: utterance.start.unwrap_or(-1.0),
                    end: utterance.end.unwrap_or(-1.0),
                    speaker: utterance.speaker,
                    words: utterance.words,
                }),
                None => Incoming::Ignored,
            }
        }
        "audio_chunk" if message.acknowledged == Some(true) => message
            .data
            .and_then(|data| serde_json::from_value::<AudioChunkAckData>(data).ok())
            .and_then(|data| data.byte_range.get(1).copied())
            .map_or(Incoming::Ignored, Incoming::Acknowledged),
        "error" => Incoming::Error(describe_server_error(message.error.as_ref())),
        _ => Incoming::Ignored,
    })
}

/// A readable reason from an `error` payload (a string, or an object with a
/// `message`).
fn describe_server_error(error: Option<&serde_json::Value>) -> String {
    match error {
        Some(serde_json::Value::String(message)) => message.clone(),
        Some(value) => value
            .get("message")
            .and_then(serde_json::Value::as_str)
            .map_or_else(|| value.to_string(), str::to_string),
        None => "unknown error".to_string(),
    }
}

/// Audio sent but not yet acknowledged, replayed after a reconnection so
/// no words are lost.
#[derive(Debug)]
struct ResendBuffer {
    /// Each chunk with its end offset (bytes since the session start).
    chunks: VecDeque<(u64, Vec<u8>)>,
    sent_bytes: u64,
    buffered_bytes: usize,
    limit_bytes: usize,
}

impl Default for ResendBuffer {
    fn default() -> Self {
        Self {
            chunks: VecDeque::new(),
            sent_bytes: 0,
            buffered_bytes: 0,
            limit_bytes: usize::MAX,
        }
    }
}

impl ResendBuffer {
    fn with_limit(limit_bytes: usize) -> Self {
        Self {
            limit_bytes,
            ..Self::default()
        }
    }

    fn push(&mut self, chunk: &[u8]) {
        self.sent_bytes += chunk.len() as u64;
        self.buffered_bytes += chunk.len();
        self.chunks.push_back((self.sent_bytes, chunk.to_vec()));
        while self.buffered_bytes > self.limit_bytes {
            match self.chunks.pop_front() {
                Some((_, dropped)) => self.buffered_bytes -= dropped.len(),
                None => break,
            }
        }
    }

    fn acknowledge(&mut self, acknowledged_end: u64) {
        while self
            .chunks
            .front()
            .is_some_and(|(end, _)| *end <= acknowledged_end)
        {
            if let Some((_, chunk)) = self.chunks.pop_front() {
                self.buffered_bytes -= chunk.len();
            }
        }
    }
}

fn audio_chunk_message(audio: &[u8]) -> Message {
    let payload = serde_json::json!({
        "type": "audio_chunk",
        "data": { "chunk": STANDARD.encode(audio) },
    });
    Message::Text(payload.to_string())
}

fn stop_recording_message() -> Message {
    Message::Text(serde_json::json!({ "type": "stop_recording" }).to_string())
}

/// How one WebSocket connection of a session ended.
enum ConnectionOutcome {
    /// Normal end (code 1000), or the client closed the session.
    Ended,
    /// The session failed and cannot resume; carries a user-facing reason.
    Failed(String),
    /// The connection dropped; the session can resume on the same URL.
    Dropped(String),
}

fn close_outcome(frame: Option<CloseFrame<'_>>) -> ConnectionOutcome {
    let Some(frame) = frame else {
        return ConnectionOutcome::Ended;
    };
    let reason = frame.reason.trim().to_string();
    match u16::from(frame.code) {
        1000 => ConnectionOutcome::Ended,
        1011 => ConnectionOutcome::Failed(if reason.is_empty() {
            "Gladia ended the session.".to_string()
        } else {
            format!("Gladia ended the session: {reason}")
        }),
        4401 => {
            ConnectionOutcome::Failed("The Gladia session URL is invalid or expired.".to_string())
        }
        code => ConnectionOutcome::Dropped(format!("closed with code {code} {reason}")),
    }
}

async fn reconnect(url: &str, session_id: &str) -> Option<WsStream> {
    for (attempt, delay_ms) in RECONNECT_DELAYS_MS.iter().enumerate() {
        tokio::time::sleep(std::time::Duration::from_millis(*delay_ms)).await;
        match tokio_tungstenite::connect_async(url).await {
            Ok((stream, _)) => {
                log::info!("[gladia-ws] (session id={session_id}) reconnected");
                return Some(stream);
            }
            Err(error) => log::warn!(
                "[gladia-ws] (session id={session_id}) reconnect attempt {} failed: {error}",
                attempt + 1
            ),
        }
    }
    None
}

/// Drive a session's WebSocket: forward audio, deliver transcripts, and
/// reconnect to the same URL (replaying unacknowledged audio) when the
/// connection drops, as Gladia's reconnection protocol requires.
async fn run_session_socket(
    mut stream: WsStream,
    url: String,
    session_id: String,
    mut outgoing: tokio::sync::mpsc::UnboundedReceiver<Outgoing>,
    events: Option<broadcast::Sender<TranscriptionEvent>>,
    resend_limit_bytes: usize,
) {
    let mut resend = ResendBuffer::with_limit(resend_limit_bytes);
    let mut stop_sent = false;
    let mut reconnects = 0u32;

    let failure = loop {
        let (mut sender, mut receiver) = stream.split();

        let mut outcome = None;
        if reconnects > 0 {
            for (_, chunk) in &resend.chunks {
                if let Err(error) = sender.send(audio_chunk_message(chunk)).await {
                    outcome = Some(ConnectionOutcome::Dropped(error.to_string()));
                    break;
                }
            }
            if outcome.is_none() && stop_sent {
                if let Err(error) = sender.send(stop_recording_message()).await {
                    outcome = Some(ConnectionOutcome::Dropped(error.to_string()));
                }
            }
        }

        let outcome = match outcome {
            Some(outcome) => outcome,
            None => loop {
                tokio::select! {
                    result = receiver.next() => match result {
                        Some(Ok(Message::Text(text))) => match parse_server_message(&text) {
                            Ok(Incoming::Transcript(event)) => {
                                if let Some(events) = events.as_ref() {
                                    let _ = events.send(event);
                                }
                            }
                            Ok(Incoming::Acknowledged(end)) => resend.acknowledge(end),
                            Ok(Incoming::Error(reason)) => {
                                log::error!("[gladia-ws] (session id={session_id}) server error: {reason}");
                                break ConnectionOutcome::Failed(format!("Gladia error: {reason}"));
                            }
                            Ok(Incoming::Ignored) => {}
                            Err(error) => log::warn!(
                                "[gladia-ws] (session id={session_id}) unrecognised message ({error}): {text}"
                            ),
                        },
                        Some(Ok(Message::Ping(payload))) => {
                            let _ = sender.send(Message::Pong(payload)).await;
                        }
                        Some(Ok(Message::Close(frame))) => {
                            log::info!("[gladia-ws] (session id={session_id}) server closed: {frame:?}");
                            break close_outcome(frame);
                        }
                        Some(Ok(_)) => {}
                        Some(Err(error)) => break ConnectionOutcome::Dropped(error.to_string()),
                        None => break ConnectionOutcome::Dropped("connection lost".to_string()),
                    },
                    message = outgoing.recv() => {
                        let sent = match message {
                            Some(Outgoing::Audio(audio)) => {
                                resend.push(&audio);
                                sender.send(audio_chunk_message(&audio)).await
                            }
                            Some(Outgoing::StopRecording) => {
                                stop_sent = true;
                                sender.send(stop_recording_message()).await
                            }
                            // The client closed the session.
                            None => break ConnectionOutcome::Ended,
                        };
                        if let Err(error) = sent {
                            break ConnectionOutcome::Dropped(error.to_string());
                        }
                    }
                }
            },
        };

        match outcome {
            ConnectionOutcome::Ended => break None,
            ConnectionOutcome::Failed(reason) => break Some(reason),
            ConnectionOutcome::Dropped(why) => {
                reconnects += 1;
                log::warn!(
                    "[gladia-ws] (session id={session_id}) connection dropped ({why}); reconnecting ({reconnects}/{MAX_RECONNECTS}), {} unacknowledged bytes to replay",
                    resend.buffered_bytes
                );
                let resumed = if reconnects <= MAX_RECONNECTS {
                    reconnect(&url, &session_id).await
                } else {
                    None
                };
                match resumed {
                    Some(new_stream) => stream = new_stream,
                    None => break Some("Connection to Gladia was lost.".to_string()),
                }
            }
        }
    };

    log::info!("[gladia-ws] (session id={session_id}) session ended");
    if let Some(events) = events.as_ref() {
        if let Some(reason) = failure {
            let _ = events.send(TranscriptionEvent::Error(reason));
        }
        let _ = events.send(TranscriptionEvent::SessionEnded);
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum TranscriptionEvent {
    /// The current hypothesis for an utterance still being spoken; each one
    /// replaces the previous partial with the same `id`.
    Partial {
        id: Option<String>,
        text: String,
        channel: Option<u32>,
        start: f64,
        end: f64,
        speaker: Option<u32>,
        words: Vec<TranscriptWord>,
    },
    Final {
        id: Option<String>,
        text: String,
        channel: Option<u32>,
        start: f64,
        end: f64,
        speaker: Option<u32>,
        words: Vec<TranscriptWord>,
    },
    /// The session failed (server error, refused option, lost connection).
    /// Carries a human-readable reason so the frontend can surface it. A
    /// normal close emits `SessionEnded` only — never `Error`.
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
    /// The API could not be reached or answered unexpectedly; the message
    /// is user-facing.
    #[error("{0}")]
    ApiUnavailable(String),
}

impl From<GladiaError> for String {
    fn from(error: GladiaError) -> Self {
        error.to_string()
    }
}

fn build_session_request(config: &GladiaConfig) -> SessionRequest {
    let api_vocabulary =
        expand_vocabulary_for_languages(config.custom_vocabulary.clone(), &config.languages);
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
    let code_switching = config.code_switching.then_some(true);
    let language_config = if config.languages.is_empty() {
        code_switching.map(|code_switching| LanguageConfig {
            languages: vec![],
            code_switching: Some(code_switching),
        })
    } else {
        Some(LanguageConfig {
            languages: config.languages.clone(),
            code_switching,
        })
    };
    SessionRequest {
        encoding: config.encoding.clone(),
        sample_rate: config.sample_rate,
        bit_depth: config.bit_depth,
        channels: config.channels,
        endpointing: config.endpointing,
        diarization: config.diarization.then_some(true),
        diarization_config: config
            .diarization
            .then_some(DiarizationConfig { max_speakers: 4 }),
        model: config.model.clone(),
        messages_config: MessagesConfig {
            receive_partial_transcripts: config.receive_partial,
            receive_final_transcripts: config.receive_final,
            receive_acknowledgments: true,
            receive_errors: true,
        },
        language_config,
        realtime_processing,
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

    #[allow(clippy::too_many_arguments)] // One argument per session option.
    pub async fn init_session(
        &self,
        api_key: &str,
        languages: Option<Vec<String>>,
        code_switching: bool,
        custom_vocabulary: Vec<CustomVocabEntry>,
        endpointing: f64,
        model: Option<String>,
        diarization: bool,
        endpoint: &ApiEndpoint,
    ) -> Result<String, GladiaError> {
        let normalized_vocabulary = normalize_vocabulary(custom_vocabulary);
        let resolved_languages = resolve_session_languages(languages.clone());

        // Guard: reuse an existing live session only when config is unchanged.
        {
            let config = self.config.lock().await;
            let session_unchanged = config.custom_vocabulary == normalized_vocabulary
                && config.languages == resolved_languages
                && config.code_switching == code_switching
                && config.endpointing == endpointing
                && config.model == model
                && config.diarization == diarization
                && config.endpoint == *endpoint;
            drop(config);

            if session_unchanged {
                let existing = self.session_id.lock().await;
                if let Some(ref id) = *existing {
                    let alive = self
                        .task_handle
                        .lock()
                        .await
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
            config.model = model;
            config.diarization = diarization;
            config.endpoint = endpoint.clone();
            build_session_request(&config)
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
            .post(endpoint.live_url())
            .header("x-gladia-key", api_key)
            .header(
                "x-gladia-version",
                format!("Gladiaflow/{}", env!("CARGO_PKG_VERSION")),
            )
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            // The body says why (`message`, `validation_errors`): keep it.
            let body = response.text().await.unwrap_or_default();
            log::error!("[gladia] session creation failed: HTTP {status}: {body}");
            let detail = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|value| {
                    value
                        .get("message")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                });
            return Err(GladiaError::WebSocketError(match detail {
                Some(detail) => format!("Failed to create session: {status}: {detail}"),
                None => format!("Failed to create session: {status}"),
            }));
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
    ) -> Result<(), GladiaError> {
        let mut config = self.config.lock().await;
        let changed = config.sample_rate != sample_rate
            || config.channels != channels
            || config.bit_depth != bit_depth;
        config.sample_rate = sample_rate;
        config.channels = channels;
        config.bit_depth = bit_depth;
        drop(config);
        if changed && self.session_id.lock().await.is_some() {
            self.close_session().await?;
        }
        Ok(())
    }

    async fn start_websocket_task(&self) -> Result<(), GladiaError> {
        let ws_url = {
            let url = self.ws_url.lock().await.clone();
            url.ok_or(GladiaError::SessionNotInitialized)?
        };

        // Connect once here so a failure is reported to the caller; later
        // reconnections happen inside the task.
        let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
            .await
            .map_err(|e| GladiaError::WebSocketError(e.to_string()))?;

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Outgoing>();
        *self.ws_tx.lock().await = Some(tx);
        self.flush_pending_audio().await;

        let transcription_tx = self.transcription_tx.lock().await.clone();
        let session_id = self
            .session_id
            .lock()
            .await
            .clone()
            .unwrap_or_else(|| "unknown".to_string());

        log::info!("[gladia-ws] websocket connected (session id={session_id})");
        let resend_limit_bytes = {
            let config = self.config.lock().await;
            audio_bytes_for_seconds(&config, RESEND_AUDIO_SECONDS)
        };

        let handle = tokio::spawn(run_session_socket(
            ws_stream,
            ws_url,
            session_id,
            rx,
            transcription_tx,
            resend_limit_bytes,
        ));
        *self.task_handle.lock().await = Some(handle);

        Ok(())
    }

    /// Check `api_key` against the live API, which is the only API this app
    /// uses (some regional hosts don't serve the pre-recorded routes).
    /// `Ok(false)` means the key was rejected; any other failure is an error
    /// so it isn't misreported as an invalid key.
    pub async fn test_connection(
        &self,
        api_key: &str,
        api_base_url: &str,
    ) -> Result<bool, GladiaError> {
        let client = reqwest::Client::new();
        let response = client
            .get(format!("{api_base_url}/v2/live?limit=1"))
            .header("x-gladia-key", api_key)
            .header(
                "x-gladia-version",
                format!("Gladiaflow/{}", env!("CARGO_PKG_VERSION")),
            )
            .send()
            .await
            .map_err(|error| {
                log::warn!("[gladia] API key check could not reach {api_base_url}: {error}");
                GladiaError::ApiUnavailable(format!("Couldn't reach {api_base_url}."))
            })?;

        let status = response.status();
        log::info!("[gladia] API key check against {api_base_url}: HTTP {status}");
        match status.as_u16() {
            _ if status.is_success() => Ok(true),
            401 | 403 => Ok(false),
            _ => Err(GladiaError::ApiUnavailable(format!(
                "{api_base_url} answered HTTP {}. Check the Gladia API URL.",
                status.as_u16()
            ))),
        }
    }

    /// The live models `api_base_url` offers `api_key`, from
    /// `GET /v2/live/capabilities` (Gladia Enterprise hosts). A host without
    /// that route (404, e.g. the global API) gets `FALLBACK_LIVE_MODELS`.
    pub async fn list_live_models(
        &self,
        api_key: &str,
        api_base_url: &str,
    ) -> Result<LiveModels, GladiaError> {
        let client = reqwest::Client::new();
        let response = client
            .get(format!("{api_base_url}/v2/live/capabilities"))
            .header("x-gladia-key", api_key)
            .header(
                "x-gladia-version",
                format!("Gladiaflow/{}", env!("CARGO_PKG_VERSION")),
            )
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|error| {
                log::warn!("[gladia] live capabilities: couldn't reach {api_base_url}: {error}");
                GladiaError::ApiUnavailable(format!("Couldn't reach {api_base_url}."))
            })?;

        let status = response.status();
        log::info!("[gladia] live capabilities from {api_base_url}: HTTP {status}");
        match status.as_u16() {
            _ if status.is_success() => {
                let capabilities: LiveCapabilities = response.json().await?;
                Ok(LiveModels {
                    models: capabilities
                        .models
                        .into_iter()
                        .map(|capability| capability.model)
                        .collect(),
                    discovered: true,
                })
            }
            404 => Ok(LiveModels {
                models: FALLBACK_LIVE_MODELS
                    .iter()
                    .map(|model| (*model).to_string())
                    .collect(),
                discovered: false,
            }),
            401 | 403 => Err(GladiaError::ApiUnavailable(format!(
                "Invalid API key for {}.",
                api_base_url
                    .trim_start_matches("https://")
                    .trim_start_matches("http://")
            ))),
            code => Err(GladiaError::ApiUnavailable(format!(
                "{api_base_url} answered HTTP {code} when listing models."
            ))),
        }
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
            tx.send(Outgoing::Audio(audio_data.to_vec()))
                .map_err(|e| GladiaError::WebSocketError(e.to_string()))?;
        } else {
            self.enqueue_pending_audio(audio_data.to_vec()).await;
        }
        Ok(())
    }

    async fn enqueue_pending_audio(&self, chunk: Vec<u8>) {
        let pending_limit_bytes = {
            let config = self.config.lock().await;
            audio_bytes_for_seconds(&config, PENDING_AUDIO_SECONDS)
        };
        let mut pending = self.pending_audio.lock().await;
        pending.total_bytes += chunk.len();
        pending.chunks.push_back(chunk);

        while pending.total_bytes > pending_limit_bytes {
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
            if tx.send(Outgoing::Audio(chunk)).is_err() {
                break;
            }
        }
    }

    /// Signal Gladia to stop recording. The server will process remaining
    /// audio buffers, send final transcriptions, and then close the WebSocket.
    pub async fn stop_recording(&self) -> Result<(), GladiaError> {
        let sender = self.ws_tx.lock().await;
        if let Some(tx) = sender.as_ref() {
            tx.send(Outgoing::StopRecording)
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

    /// Serve one HTTP request with `status_line` and return its request line.
    async fn serve_once(listener: TcpListener, status_line: &'static str) -> String {
        serve_once_with_body(listener, status_line, "").await
    }

    /// Serve one HTTP request with `status_line` and a JSON `body`, and
    /// return its request line.
    async fn serve_once_with_body(
        listener: TcpListener,
        status_line: &'static str,
        body: &'static str,
    ) -> String {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 8192];
        let n = socket.read(&mut buf).await.unwrap();
        let request = String::from_utf8_lossy(&buf[..n]).into_owned();
        let response = format!(
            "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(response.as_bytes()).await.unwrap();
        request.lines().next().unwrap_or_default().to_string()
    }

    #[tokio::test]
    async fn list_live_models_reads_the_host_capabilities() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(serve_once_with_body(
            listener,
            "HTTP/1.1 200 OK",
            r#"{"models":[{"model":"trantoria-1","addons":["pii"]},{"model":"solaria-3","addons":[]}]}"#,
        ));

        let models = GladiaClient::new()
            .list_live_models("key", &base)
            .await
            .unwrap();

        assert_eq!(
            models,
            LiveModels {
                models: vec!["trantoria-1".to_string(), "solaria-3".to_string()],
                discovered: true,
            }
        );
        assert_eq!(server.await.unwrap(), "GET /v2/live/capabilities HTTP/1.1");
    }

    #[tokio::test]
    async fn list_live_models_falls_back_when_the_host_has_no_capabilities() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(serve_once(listener, "HTTP/1.1 404 Not Found"));

        let models = GladiaClient::new()
            .list_live_models("key", &base)
            .await
            .unwrap();

        assert_eq!(models.models, vec!["solaria-1".to_string()]);
        assert!(!models.discovered);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn list_live_models_reports_a_rejected_key() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(serve_once(listener, "HTTP/1.1 401 Unauthorized"));

        let error = GladiaClient::new()
            .list_live_models("key", &base)
            .await
            .unwrap_err();

        assert!(
            error
                .to_string()
                .starts_with("Invalid API key for 127.0.0.1:"),
            "{error}"
        );
        server.await.unwrap();
    }

    #[test]
    fn session_request_sends_the_model_only_when_chosen() {
        let body = serde_json::to_value(build_session_request(&GladiaConfig::default())).unwrap();
        assert!(body.get("model").is_none());

        let config = GladiaConfig {
            model: Some("solaria-3".to_string()),
            ..GladiaConfig::default()
        };
        let body = serde_json::to_value(build_session_request(&config)).unwrap();
        assert_eq!(body["model"], "solaria-3");
    }

    #[test]
    fn session_request_preserves_dual_channel_audio_format() {
        let config = GladiaConfig {
            sample_rate: 48_000,
            channels: 2,
            bit_depth: 16,
            diarization: true,
            ..GladiaConfig::default()
        };
        let body = serde_json::to_value(build_session_request(&config)).unwrap();
        assert_eq!(body["sample_rate"], 48_000);
        assert_eq!(body["channels"], 2);
        assert_eq!(body["bit_depth"], 16);
        assert_eq!(body["diarization"], true);
        assert_eq!(body["diarization_config"]["max_speakers"], 4);
    }

    #[test]
    fn regular_live_session_does_not_request_diarization() {
        let body = serde_json::to_value(build_session_request(&GladiaConfig::default())).unwrap();
        assert!(body.get("diarization").is_none());
        assert!(body.get("diarization_config").is_none());
    }

    #[test]
    fn audio_buffer_limits_follow_the_negotiated_format() {
        let mono = GladiaConfig::default();
        let stereo = GladiaConfig {
            sample_rate: 48_000,
            channels: 2,
            bit_depth: 16,
            ..GladiaConfig::default()
        };
        assert_eq!(audio_bytes_for_seconds(&mono, 3), 96_000);
        assert_eq!(audio_bytes_for_seconds(&stereo, 3), 576_000);
    }

    #[tokio::test]
    async fn test_connection_uses_custom_api_base_url() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/proxy", listener.local_addr().unwrap());
        let server = tokio::spawn(serve_once(listener, "HTTP/1.1 200 OK"));

        let ok = GladiaClient::new()
            .test_connection("key", &base)
            .await
            .unwrap();

        assert!(ok);
        assert_eq!(server.await.unwrap(), "GET /proxy/v2/live?limit=1 HTTP/1.1");
    }

    #[tokio::test]
    async fn test_connection_reports_rejected_key_as_invalid() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(serve_once(listener, "HTTP/1.1 401 Unauthorized"));

        let ok = GladiaClient::new()
            .test_connection("key", &base)
            .await
            .unwrap();

        assert!(!ok);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_connection_reports_other_statuses_as_errors() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(serve_once(listener, "HTTP/1.1 404 Not Found"));

        let error = GladiaClient::new()
            .test_connection("key", &base)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("HTTP 404"), "{error}");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn init_session_sends_no_region_to_a_regional_host() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(serve_once(listener, "HTTP/1.1 401 Unauthorized"));
        let endpoint = ApiEndpoint::new(base, "us-west");

        let result = GladiaClient::new()
            .init_session("key", None, false, vec![], 0.1, None, false, &endpoint)
            .await;

        assert!(result.is_err());
        assert_eq!(server.await.unwrap(), "POST /v2/live HTTP/1.1");
    }

    #[test]
    fn api_endpoint_sends_region_only_to_the_global_api() {
        let global = ApiEndpoint::new(DEFAULT_API_BASE_URL.to_string(), "eu-west");
        assert_eq!(
            global.live_url(),
            "https://api.gladia.io/v2/live?region=eu-west"
        );

        let regional = ApiEndpoint::new("https://eu1.api.gladia.io".to_string(), "us-west");
        assert_eq!(regional.region, None);
        assert_eq!(regional.live_url(), "https://eu1.api.gladia.io/v2/live");
    }

    fn config_with_vocabulary(endpoint: ApiEndpoint) -> GladiaConfig {
        GladiaConfig {
            languages: vec!["en".to_string(), "fr".to_string()],
            custom_vocabulary: vec![CustomVocabEntry {
                value: "gladiaflow".to_string(),
                pronunciations: None,
                language: None,
                intensity: 0.4,
            }],
            endpoint,
            ..GladiaConfig::default()
        }
    }

    #[test]
    fn session_request_keeps_vocabulary_per_language_on_any_host() {
        // Regional Enterprise hosts accept `language` on vocabulary entries
        // too (verified on eu1), so every host gets the same vocabulary.
        for base_url in [DEFAULT_API_BASE_URL, "https://eu1.api.gladia.io"] {
            let config = config_with_vocabulary(ApiEndpoint::new(base_url.to_string(), "eu-west"));

            let body = serde_json::to_value(build_session_request(&config)).unwrap();

            let languages: Vec<_> = body["realtime_processing"]["custom_vocabulary_config"]
                ["vocabulary"]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| entry["language"].clone())
                .collect();
            assert_eq!(
                languages,
                vec![serde_json::json!("en"), serde_json::json!("fr")],
                "{base_url}"
            );
            assert_eq!(body["messages_config"]["receive_acknowledgments"], true);
            assert_eq!(body["messages_config"]["receive_errors"], true);
        }
    }

    #[test]
    fn parses_transcripts_acknowledgments_and_errors() {
        assert_eq!(
            parse_server_message(
                r#"{"type":"transcript","data":{"id":"1","is_final":true,"utterance":{"text":"Hi there","channel":1,"start":0.5,"end":1.0,"speaker":1,"words":[{"word":"Hi","start":0.5,"end":0.7,"speaker":0},{"word":"there","start":0.8,"end":1.0,"speaker":1}]}}}"#
            )
            .unwrap(),
            Incoming::Transcript(TranscriptionEvent::Final {
                id: Some("1".to_string()),
                text: "Hi there".to_string(),
                channel: Some(1),
                start: 0.5,
                end: 1.0,
                speaker: Some(1),
                words: vec![
                    TranscriptWord {
                        word: "Hi".to_string(),
                        start: Some(0.5),
                        end: Some(0.7),
                        speaker: Some(0),
                    },
                    TranscriptWord {
                        word: "there".to_string(),
                        start: Some(0.8),
                        end: Some(1.0),
                        speaker: Some(1),
                    },
                ],
            })
        );
        assert_eq!(
            parse_server_message(
                r#"{"type":"transcript","data":{"id":"00_00000002","is_final":false,"utterance":{"text":"H","start":0.2,"end":0.4}}}"#
            )
            .unwrap(),
            Incoming::Transcript(TranscriptionEvent::Partial {
                id: Some("00_00000002".to_string()),
                text: "H".to_string(),
                channel: None,
                start: 0.2,
                end: 0.4,
                speaker: None,
                words: vec![],
            })
        );
        assert_eq!(
            parse_server_message(
                r#"{"type":"transcript","data":{"is_final":false,"utterance":{"text":"H"}}}"#
            )
            .unwrap(),
            Incoming::Transcript(TranscriptionEvent::Partial {
                id: None,
                text: "H".to_string(),
                channel: None,
                start: -1.0,
                end: -1.0,
                speaker: None,
                words: vec![],
            })
        );
        assert_eq!(
            parse_server_message(
                r#"{"type":"audio_chunk","acknowledged":true,"data":{"byte_range":[0,3200]}}"#
            )
            .unwrap(),
            Incoming::Acknowledged(3200)
        );
        assert_eq!(
            parse_server_message(
                r#"{"type":"error","error":{"message":"no inference endpoint available"}}"#
            )
            .unwrap(),
            Incoming::Error("no inference endpoint available".to_string())
        );
        assert_eq!(
            parse_server_message(r#"{"type":"error","error":"bad audio"}"#).unwrap(),
            Incoming::Error("bad audio".to_string())
        );
        assert_eq!(
            parse_server_message(r#"{"type":"speech_start","data":{"time":1.0}}"#).unwrap(),
            Incoming::Ignored
        );
    }

    #[test]
    fn resend_buffer_drops_acknowledged_chunks_only() {
        let mut buffer = ResendBuffer::default();
        buffer.push(&[0; 100]);
        buffer.push(&[1; 100]);
        buffer.push(&[2; 100]);

        buffer.acknowledge(150);
        assert_eq!(
            buffer.chunks.len(),
            2,
            "a partly acknowledged chunk is kept"
        );

        buffer.acknowledge(300);
        assert!(buffer.chunks.is_empty());
        assert_eq!(buffer.buffered_bytes, 0);
    }

    #[test]
    fn close_codes_map_to_outcomes() {
        let frame = |code: u16, reason: &'static str| {
            Some(CloseFrame {
                code: CloseCode::from(code),
                reason: reason.into(),
            })
        };
        assert!(matches!(
            close_outcome(frame(1000, "")),
            ConnectionOutcome::Ended
        ));
        assert!(matches!(
            close_outcome(frame(1011, "rejected: translation")),
            ConnectionOutcome::Failed(reason) if reason.contains("rejected: translation")
        ));
        assert!(matches!(
            close_outcome(frame(4401, "")),
            ConnectionOutcome::Failed(_)
        ));
        assert!(matches!(
            close_outcome(frame(1012, "")),
            ConnectionOutcome::Dropped(_)
        ));
    }

    async fn connected_client(
        url: String,
    ) -> (GladiaClient, broadcast::Receiver<TranscriptionEvent>) {
        let client = GladiaClient::new();
        *client.ws_url.lock().await = Some(url);
        *client.session_id.lock().await = Some("test-session".to_string());
        let (tx, rx) = broadcast::channel(16);
        *client.transcription_tx.lock().await = Some(tx);
        client.start_websocket_task().await.unwrap();
        (client, rx)
    }

    async fn next_event(rx: &mut broadcast::Receiver<TranscriptionEvent>) -> TranscriptionEvent {
        tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("timed out waiting for an event")
            .unwrap()
    }

    async fn next_text(ws: &mut WebSocketStream<tokio::net::TcpStream>) -> String {
        loop {
            match ws.next().await.unwrap().unwrap() {
                Message::Text(text) => return text,
                _ => continue,
            }
        }
    }

    #[tokio::test]
    async fn server_error_message_fails_the_session_with_its_reason() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
            ws.send(Message::Text(
                r#"{"type":"error","error":{"message":"no inference endpoint available"}}"#
                    .to_string(),
            ))
            .await
            .unwrap();
            while ws.next().await.is_some() {}
        });

        let (_client, mut rx) = connected_client(url).await;

        assert_eq!(
            next_event(&mut rx).await,
            TranscriptionEvent::Error("Gladia error: no inference endpoint available".to_string())
        );
        assert_eq!(next_event(&mut rx).await, TranscriptionEvent::SessionEnded);
    }

    #[tokio::test]
    async fn reconnects_after_1012_and_replays_unacknowledged_audio() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            // First connection: take one chunk, then restart (1012).
            let (socket, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
            let first = next_text(&mut ws).await;
            ws.close(Some(CloseFrame {
                code: CloseCode::from(1012),
                reason: "restarting".into(),
            }))
            .await
            .unwrap();

            // Second connection: the unacknowledged chunk comes again first.
            let (socket, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
            let replayed = next_text(&mut ws).await;
            ws.send(Message::Text(
                r#"{"type":"transcript","data":{"id":"1","is_final":true,"utterance":{"text":"hello","start":0,"end":1}}}"#
                    .to_string(),
            ))
            .await
            .unwrap();
            ws.close(None).await.unwrap();
            (first, replayed)
        });

        let (client, mut rx) = connected_client(url).await;
        client.send_audio(b"abcd").await.unwrap();

        assert_eq!(
            next_event(&mut rx).await,
            TranscriptionEvent::Final {
                id: Some("1".to_string()),
                text: "hello".to_string(),
                channel: None,
                start: 0.0,
                end: 1.0,
                speaker: None,
                words: vec![],
            }
        );
        assert_eq!(next_event(&mut rx).await, TranscriptionEvent::SessionEnded);
        let (first, replayed) = server.await.unwrap();
        assert_eq!(first, replayed);
        assert_eq!(Message::Text(first), audio_chunk_message(b"abcd"));
    }
}
