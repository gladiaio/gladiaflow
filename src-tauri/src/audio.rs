use arc_swap::ArcSwapOption;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{bounded, unbounded, Receiver, RecvTimeoutError, Sender};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum AudioDeviceSelection {
    #[default]
    Automatic,
    SystemDefault,
    Specific {
        device_id: String,
        device_name: String,
    },
}

impl AudioDeviceSelection {
    pub fn from_ipc(selection: Option<Self>, legacy_name: Option<String>) -> Self {
        selection.unwrap_or_else(|| {
            legacy_name
                .filter(|name| !name.is_empty())
                .map(|device_name| Self::Specific {
                    device_id: String::new(),
                    device_name,
                })
                .unwrap_or(Self::SystemDefault)
        })
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: String,
    pub transport: String,
    pub is_default: bool,
    pub is_built_in: bool,
}

pub struct AudioCapture {
    sample_rate: Arc<AtomicU32>,
    commands: Sender<CaptureCommand>,
    command_lock: Mutex<()>,
    next_generation: AtomicU64,
}

struct SessionRoute {
    tx: Sender<AudioChunk>,
    error_tx: Option<Sender<String>>,
    generation: u64,
    played_at: Mutex<Instant>,
    first_callback_seen: AtomicBool,
}

struct PreparedCapture {
    stream: cpal::Stream,
    selection: AudioDeviceSelection,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    resolved_device_id: Option<String>,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    resolved_device_name: String,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    input_sample_rate: u32,
    output_sample_rate: u32,
    route: Arc<ArcSwapOption<SessionRoute>>,
    invalidated: Arc<AtomicBool>,
    active: bool,
}

enum CaptureCommand {
    Prepare(AudioDeviceSelection, Sender<Result<u32, String>>),
    Start(
        AudioDeviceSelection,
        Arc<SessionRoute>,
        Sender<Result<(u32, String), String>>,
    ),
    Stop(Sender<Result<(), String>>),
}

pub const GLADIA_SAMPLE_RATES: [u32; 5] = [8_000, 16_000, 32_000, 44_100, 48_000];
const FALLBACK_SAMPLE_RATE: u32 = 16_000;
const DEFAULT_DEVICE_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const OBSERVED_RATE_TOLERANCE: f64 = 0.08;
const OBSERVED_RATE_CONFIRMATIONS: u8 = 2;
const COMMON_INPUT_SAMPLE_RATES: [u32; 12] = [
    8_000, 12_000, 16_000, 22_050, 24_000, 32_000, 44_100, 48_000, 88_200, 96_000, 176_400, 192_000,
];

pub fn is_gladia_compatible_sample_rate(rate: u32) -> bool {
    GLADIA_SAMPLE_RATES.contains(&rate)
}

pub fn effective_output_sample_rate(native_rate: u32) -> u32 {
    if is_gladia_compatible_sample_rate(native_rate) {
        native_rate
    } else {
        FALLBACK_SAMPLE_RATE
    }
}

fn log_sample_rate_decision(native_rate: u32, output_rate: u32) {
    if is_gladia_compatible_sample_rate(native_rate) {
        log::info!(
            "audio: detected native sample rate {native_rate} Hz (Gladia-compatible) → passthrough, no resampling"
        );
    } else {
        log::info!(
            "audio: detected native sample rate {native_rate} Hz (not Gladia-compatible) → resampling to {output_rate} Hz"
        );
    }
}

pub fn list_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    match host.input_devices() {
        Ok(devices) => devices
            .filter_map(|d| {
                d.description()
                    .ok()
                    .map(|description| description.name().to_string())
            })
            .collect(),
        Err(_) => vec![],
    }
}

pub fn list_input_device_info() -> Vec<AudioDeviceInfo> {
    let host = cpal::default_host();
    let default_id = host
        .default_input_device()
        .and_then(|device| device.id().ok())
        .map(|id| id.to_string());
    host.input_devices()
        .map(|devices| {
            devices
                .filter_map(|device| {
                    let description = device.description().ok()?;
                    let name = description.name().to_string();
                    let id = device
                        .id()
                        .ok()
                        .map(|id| id.to_string())
                        .unwrap_or_default();
                    let transport = device_transport(&device, &name);
                    Some(AudioDeviceInfo {
                        is_default: default_id.as_deref() == Some(id.as_str()),
                        is_built_in: transport == "built_in",
                        id,
                        name,
                        transport: transport.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn device_transport(device: &cpal::Device, name: &str) -> &'static str {
    #[cfg(target_os = "macos")]
    {
        use coreaudio::audio_unit::macos_helpers::{
            get_device_id_from_name, get_device_transport_type,
        };
        if let Some(id) = get_device_id_from_name(name, true) {
            if let Ok(value) = get_device_transport_type(id) {
                return match value {
                    0x626c7565 | 0x626c6561 => "bluetooth", // 'blue' / 'blea'
                    0x626c746e => "built_in",               // 'bltn'
                    _ => "other",
                };
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = name;
    match device
        .description()
        .map(|description| description.interface_type())
        .unwrap_or_default()
    {
        cpal::InterfaceType::Bluetooth => "bluetooth",
        cpal::InterfaceType::BuiltIn => "built_in",
        cpal::InterfaceType::Usb => "usb",
        cpal::InterfaceType::Virtual => "virtual",
        _ => "unknown",
    }
}

fn resolve_device(
    host: &cpal::Host,
    selection: &AudioDeviceSelection,
) -> Result<cpal::Device, String> {
    let default = || {
        host.default_input_device()
            .ok_or_else(|| "No microphone input device available".to_string())
    };
    match selection {
        AudioDeviceSelection::SystemDefault => default(),
        AudioDeviceSelection::Specific {
            device_id,
            device_name,
        } => {
            let devices = host.input_devices().map_err(|error| error.to_string())?;
            Ok(devices
                .filter_map(|device| {
                    let id_matches = !device_id.is_empty()
                        && device
                            .id()
                            .ok()
                            .is_some_and(|id| id.to_string() == *device_id);
                    let name_matches = device_has_name(&device, device_name);
                    (id_matches || name_matches).then_some(device)
                })
                .next()
                .or_else(|| host.default_input_device())
                .ok_or_else(|| "No microphone input device available".to_string())?)
        }
        AudioDeviceSelection::Automatic => {
            let default_device = default()?;
            let default_name = default_device
                .description()
                .map(|description| description.name().to_string())
                .unwrap_or_default();
            let default_transport = device_transport(&default_device, &default_name);
            if default_transport != "bluetooth" {
                return Ok(default_device);
            }
            let built_in = host
                .input_devices()
                .map_err(|error| error.to_string())?
                .find(|device| {
                    let name = device
                        .description()
                        .map(|description| description.name().to_string())
                        .unwrap_or_default();
                    device_transport(device, &name) == "built_in"
                });
            if automatic_prefers_built_in(default_transport, built_in.is_some()) {
                let device = built_in.expect("built-in device checked above");
                Ok(device)
            } else {
                Ok(default_device)
            }
        }
    }
}

fn automatic_prefers_built_in(default_transport: &str, has_built_in: bool) -> bool {
    default_transport == "bluetooth" && has_built_in
}

fn device_has_name(device: &cpal::Device, expected_name: &str) -> bool {
    device
        .description()
        .map(|description| description.name() == expected_name)
        .unwrap_or(false)
}

pub struct AudioChunk {
    pub data: Vec<u8>,
}

// Keep software gain modest: aggressive fixed gain clips speech and hurts ASR.
const MAX_SOFTWARE_PREAMP_GAIN: f32 = 1.25;
const TARGET_RMS: f32 = 0.12;
const MIN_RMS_FOR_GAIN: f32 = 0.015;

struct AudioResampler {
    step: f64,
    cursor: f64,
    passthrough: bool,
    input_buffer: Vec<f32>,
}

struct CallbackRateValidator {
    previous_capture: Option<cpal::StreamInstant>,
    previous_frames: usize,
    candidate: Option<u32>,
    confirmations: u8,
    resolved: bool,
}

impl CallbackRateValidator {
    fn new() -> Self {
        Self {
            previous_capture: None,
            previous_frames: 0,
            candidate: None,
            confirmations: 0,
            resolved: false,
        }
    }

    fn observe(&mut self, capture: cpal::StreamInstant, frames: usize) -> Option<u32> {
        if self.resolved {
            return None;
        }

        let observed = self
            .previous_capture
            .and_then(|previous| capture.duration_since(&previous))
            .and_then(|elapsed| estimate_common_sample_rate(self.previous_frames, elapsed));
        self.previous_capture = Some(capture);
        self.previous_frames = frames;

        let observed = observed?;
        if self.candidate == Some(observed) {
            self.confirmations = self.confirmations.saturating_add(1);
        } else {
            self.candidate = Some(observed);
            self.confirmations = 1;
        }

        if self.confirmations >= OBSERVED_RATE_CONFIRMATIONS {
            self.resolved = true;
            Some(observed)
        } else {
            None
        }
    }
}

fn estimate_common_sample_rate(frames: usize, elapsed: Duration) -> Option<u32> {
    if frames == 0 || elapsed.is_zero() {
        return None;
    }
    let measured = frames as f64 / elapsed.as_secs_f64();
    let nearest = COMMON_INPUT_SAMPLE_RATES
        .iter()
        .copied()
        .min_by(|left, right| {
            (measured - *left as f64)
                .abs()
                .total_cmp(&(measured - *right as f64).abs())
        })?;
    let relative_error = (measured - nearest as f64).abs() / nearest as f64;
    (relative_error <= OBSERVED_RATE_TOLERANCE).then_some(nearest)
}

impl AudioResampler {
    fn new(input_sample_rate: u32, output_sample_rate: u32) -> Self {
        Self {
            step: input_sample_rate as f64 / output_sample_rate as f64,
            cursor: 0.0,
            passthrough: input_sample_rate == output_sample_rate,
            input_buffer: Vec::new(),
        }
    }

    fn process(&mut self, input: &[f32]) -> Vec<f32> {
        if self.passthrough {
            return input.to_vec();
        }

        self.input_buffer.extend_from_slice(input);
        let len = self.input_buffer.len();
        let mut out = Vec::new();

        if self.step > 1.0 {
            // Downsampling. Averaging the input samples spanning each output
            // step acts as a box low-pass filter, suppressing the high-frequency
            // energy that would otherwise alias when we decimate (e.g. 48k -> 16k).
            // A naive pick/interpolate without this filter aliases and degrades ASR.
            while self.cursor + self.step < len as f64 {
                let start = self.cursor.floor() as usize;
                let end = (self.cursor + self.step).floor() as usize;
                let end = end.min(len - 1);

                let mut sum = 0.0f32;
                let mut count = 0u32;
                for &s in &self.input_buffer[start..=end] {
                    sum += s;
                    count += 1;
                }
                if count > 0 {
                    out.push(sum / count as f32);
                }
                self.cursor += self.step;
            }
        } else {
            // Upsampling or near-unity: linear interpolation, no aliasing risk.
            while self.cursor + 1.0 < len as f64 {
                let i0 = self.cursor.floor() as usize;
                let i1 = i0 + 1;
                let frac = (self.cursor - i0 as f64) as f32;

                let s0 = self.input_buffer[i0];
                let s1 = self.input_buffer[i1];
                out.push(s0 + (s1 - s0) * frac);
                self.cursor += self.step;
            }
        }

        let consumed = (self.cursor.floor() as usize).min(self.input_buffer.len());
        if consumed > 0 {
            self.input_buffer.drain(..consumed);
            self.cursor -= consumed as f64;
        }

        out
    }
}

fn pcm_f32_to_le_bytes(samples: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        let clamped = s.clamp(-1.0, 1.0);
        let i16_sample = (clamped * 32767.0) as i16;
        out.extend_from_slice(&i16_sample.to_le_bytes());
    }
    out
}

fn apply_software_preamp(samples: &mut [f32]) {
    if samples.is_empty() {
        return;
    }

    let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
    if rms < MIN_RMS_FOR_GAIN {
        // Avoid boosting room hiss when the mic is effectively silent.
        return;
    }

    let gain = (TARGET_RMS / rms).clamp(1.0, MAX_SOFTWARE_PREAMP_GAIN);
    if (gain - 1.0).abs() < 0.01 {
        return;
    }

    for sample in samples.iter_mut() {
        *sample = (*sample * gain).clamp(-1.0, 1.0);
    }
}

fn f32_to_mono(samples: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return samples.to_vec();
    }
    let mut energy = vec![0.0f64; channels];
    let mut frames = 0usize;
    for frame in samples.chunks(channels) {
        if frame.len() < channels {
            break;
        }
        frames += 1;
        for (ch, sample) in frame.iter().enumerate() {
            let s = *sample as f64;
            energy[ch] += s * s;
        }
    }
    if frames == 0 {
        return Vec::new();
    }

    // Keep the channel with the highest RMS (usually the real mic channel).
    let dominant = energy
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(idx, _)| idx)
        .unwrap_or(0);

    samples
        .chunks(channels)
        .filter_map(|frame| frame.get(dominant).copied())
        .collect()
}

fn i16_to_mono(samples: &[i16], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return samples
            .iter()
            .map(|&s| s as f32 / i16::MAX as f32)
            .collect();
    }
    let mut energy = vec![0.0f64; channels];
    let mut frames = 0usize;
    for frame in samples.chunks(channels) {
        if frame.len() < channels {
            break;
        }
        frames += 1;
        for (ch, sample) in frame.iter().enumerate() {
            let s = (*sample as f64) / i16::MAX as f64;
            energy[ch] += s * s;
        }
    }
    if frames == 0 {
        return Vec::new();
    }

    let dominant = energy
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(idx, _)| idx)
        .unwrap_or(0);

    samples
        .chunks(channels)
        .filter_map(|frame| frame.get(dominant).copied())
        .map(|s| s as f32 / i16::MAX as f32)
        .collect()
}

fn u16_to_mono(samples: &[u16], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return samples
            .iter()
            .map(|&s| (s as f32 / u16::MAX as f32) * 2.0 - 1.0)
            .collect();
    }
    let mut energy = vec![0.0f64; channels];
    let mut frames = 0usize;
    for frame in samples.chunks(channels) {
        if frame.len() < channels {
            break;
        }
        frames += 1;
        for (ch, sample) in frame.iter().enumerate() {
            let s = (*sample as f64 / u16::MAX as f64) * 2.0 - 1.0;
            energy[ch] += s * s;
        }
    }
    if frames == 0 {
        return Vec::new();
    }

    let dominant = energy
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(idx, _)| idx)
        .unwrap_or(0);

    samples
        .chunks(channels)
        .filter_map(|frame| frame.get(dominant).copied())
        .map(|s| (s as f32 / u16::MAX as f32) * 2.0 - 1.0)
        .collect()
}

fn run_audio_callback_safely<F>(f: F)
where
    F: FnOnce(),
{
    if let Err(payload) = catch_unwind(AssertUnwindSafe(f)) {
        let reason = if let Some(msg) = payload.downcast_ref::<&str>() {
            *msg
        } else if let Some(msg) = payload.downcast_ref::<String>() {
            msg.as_str()
        } else {
            "non-string panic payload"
        };
        log::error!("audio callback panicked: {reason}");
    }
}

fn capture_worker(commands: Receiver<CaptureCommand>, sample_rate: Arc<AtomicU32>) {
    let mut prepared: Option<PreparedCapture> = None;
    loop {
        let command = match commands.recv_timeout(DEFAULT_DEVICE_REFRESH_INTERVAL) {
            Ok(command) => command,
            Err(RecvTimeoutError::Timeout) => {
                refresh_default_capture(&mut prepared, &sample_rate);
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };
        match command {
            CaptureCommand::Prepare(device_name, reply) => {
                let result = ensure_prepared(&mut prepared, device_name, &sample_rate);
                let _ = reply.send(result);
            }
            CaptureCommand::Start(device_name, route, reply) => {
                let started = Instant::now();
                let result =
                    ensure_prepared(&mut prepared, device_name, &sample_rate).and_then(|rate| {
                        let capture = prepared.as_mut().expect("prepared capture missing");
                        if capture.route.load().is_some() {
                            return Err("Already running".to_string());
                        }
                        capture.route.store(Some(route));
                        if let Some(active) = capture.route.load_full() {
                            *active
                                .played_at
                                .lock()
                                .expect("audio latency clock poisoned") = Instant::now();
                        }
                        if let Err(error) = capture.stream.play() {
                            capture.route.store(None);
                            prepared = None;
                            return Err(format!("Failed to start microphone stream: {error}"));
                        }
                        capture.active = true;
                        log::info!(
                            "audio latency: start-command-to-play={}ms",
                            started.elapsed().as_millis()
                        );
                        Ok((rate, capture.resolved_device_name.clone()))
                    });
                let _ = reply.send(result);
            }
            CaptureCommand::Stop(reply) => {
                let result = if let Some(capture) = prepared.as_mut() {
                    let pause = if capture.active {
                        capture
                            .stream
                            .pause()
                            .map_err(|error| format!("Failed to stop microphone stream: {error}"))
                    } else {
                        Ok(())
                    };
                    capture.active = false;
                    // Removing the sole persistent route closes the session channel
                    // as soon as any in-flight callback returns.
                    capture.route.store(None);
                    if pause.is_err() {
                        prepared = None;
                    }
                    pause
                } else {
                    Ok(())
                };
                let _ = reply.send(result);
            }
        }
    }
}

fn refresh_default_capture(prepared: &mut Option<PreparedCapture>, sample_rate: &AtomicU32) {
    let Some(capture) = prepared.as_ref() else {
        return;
    };
    if matches!(capture.selection, AudioDeviceSelection::Specific { .. })
        || capture.active
        || capture.route.load().is_some()
    {
        return;
    }

    let current = match selection_snapshot(&capture.selection) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            log::warn!("audio: could not refresh system default microphone: {error}");
            return;
        }
    };
    let changed = default_capture_changed(
        capture.resolved_device_id.as_deref(),
        capture.input_sample_rate,
        current.id.as_deref(),
        current.sample_rate,
        capture.invalidated.load(Ordering::Acquire),
    );
    if !changed {
        return;
    }

    let old_description = format!(
        "{} ({:?}, {} Hz)",
        capture.resolved_device_name, capture.resolved_device_id, capture.input_sample_rate
    );
    match build_prepared_capture(capture.selection.clone()) {
        Ok(replacement) => {
            log::info!(
                "audio: refreshed system default microphone from {old_description} to {} ({:?}, {} Hz)",
                replacement.resolved_device_name,
                replacement.resolved_device_id,
                replacement.input_sample_rate
            );
            sample_rate.store(replacement.output_sample_rate, Ordering::Release);
            *prepared = Some(replacement);
        }
        Err(error) => {
            log::warn!(
                "audio: failed to prepare changed system default microphone; retaining previous paused stream: {error}"
            );
        }
    }
}

fn default_capture_changed(
    prepared_id: Option<&str>,
    prepared_rate: u32,
    current_id: Option<&str>,
    current_rate: u32,
    invalidated: bool,
) -> bool {
    invalidated || prepared_id != current_id || prepared_rate != current_rate
}

struct DefaultInputSnapshot {
    id: Option<String>,
    sample_rate: u32,
}

fn selection_snapshot(selection: &AudioDeviceSelection) -> Result<DefaultInputSnapshot, String> {
    let host = cpal::default_host();
    let device = resolve_device(&host, selection)?;
    let sample_rate = device
        .default_input_config()
        .map_err(|error| format!("Failed to read microphone config: {error}"))?
        .sample_rate();
    Ok(DefaultInputSnapshot {
        id: device.id().ok().map(|id| id.to_string()),
        sample_rate,
    })
}

fn ensure_prepared(
    prepared: &mut Option<PreparedCapture>,
    selection: AudioDeviceSelection,
    sample_rate: &AtomicU32,
) -> Result<u32, String> {
    if let Some(capture) = prepared.as_ref() {
        if capture.selection == selection && !capture.invalidated.load(Ordering::Acquire) {
            return Ok(capture.output_sample_rate);
        }
        if capture.route.load().is_some() {
            return Err("Cannot change microphone while recording".to_string());
        }
    }
    let capture = build_prepared_capture(selection)?;
    let rate = capture.output_sample_rate;
    sample_rate.store(rate, Ordering::Release);
    *prepared = Some(capture);
    Ok(rate)
}

fn build_prepared_capture(selection: AudioDeviceSelection) -> Result<PreparedCapture, String> {
    let host = cpal::default_host();
    let device = resolve_device(&host, &selection)?;
    let resolved_device_id = device.id().ok().map(|id| id.to_string());
    let resolved_device_name = device
        .description()
        .map(|description| description.name().to_string())
        .unwrap_or_else(|_| "Unknown microphone".to_string());
    let input_config = device
        .default_input_config()
        .map_err(|error| format!("Failed to read microphone config: {error}"))?;
    let stream_config: cpal::StreamConfig = input_config.config();
    let channels = stream_config.channels as usize;
    if channels == 0 {
        return Err("Invalid microphone configuration (0 channels)".to_string());
    }
    let input_rate = stream_config.sample_rate;
    let output_rate = effective_output_sample_rate(input_rate);
    log::info!(
        "audio: resolved microphone selection={selection:?}, name={resolved_device_name:?}, id={resolved_device_id:?}, configured_rate={input_rate} Hz"
    );
    log_sample_rate_decision(input_rate, output_rate);

    let route = Arc::new(ArcSwapOption::<SessionRoute>::empty());
    let invalidated = Arc::new(AtomicBool::new(false));
    let error_route = route.clone();
    let error_invalidated = invalidated.clone();
    let error_callback = move |error| {
        error_invalidated.store(true, Ordering::Release);
        let message = format!("Microphone stream error: {error}");
        log::error!("{message}");
        if let Some(active) = error_route.load_full() {
            if let Some(tx) = &active.error_tx {
                let _ = tx.send(message);
            }
        }
    };

    macro_rules! build_stream {
        ($sample:ty, $to_mono:ident) => {{
            let callback_route = route.clone();
            let mut generation = 0u64;
            let mut resampler = AudioResampler::new(input_rate, output_rate);
            #[cfg(target_os = "macos")]
            let mut validated_input_rate = input_rate;
            #[cfg(target_os = "macos")]
            let mut rate_validator = CallbackRateValidator::new();
            device.build_input_stream(
                &stream_config,
                move |data: &[$sample], _info: &cpal::InputCallbackInfo| {
                    run_audio_callback_safely(|| {
                        let Some(active) = callback_route.load_full() else {
                            return;
                        };
                        if generation != active.generation {
                            generation = active.generation;
                            resampler = AudioResampler::new(input_rate, output_rate);
                            #[cfg(target_os = "macos")]
                            {
                                validated_input_rate = input_rate;
                                rate_validator = CallbackRateValidator::new();
                            }
                        }
                        if !active.first_callback_seen.swap(true, Ordering::Relaxed) {
                            log::info!(
                                "audio latency: play-to-first-callback={}ms",
                                active
                                    .played_at
                                    .lock()
                                    .map(|instant| instant.elapsed().as_millis())
                                    .unwrap_or_default()
                            );
                        }
                        let mono = $to_mono(data, channels);
                        #[cfg(target_os = "macos")]
                        if let Some(observed_rate) =
                            rate_validator.observe(_info.timestamp().capture, mono.len())
                        {
                            if observed_rate != validated_input_rate {
                                log::warn!(
                                    "audio: callback rate mismatch: configured={} Hz, observed={} Hz; resampling observed input to {} Hz",
                                    input_rate,
                                    observed_rate,
                                    output_rate
                                );
                                validated_input_rate = observed_rate;
                                resampler =
                                    AudioResampler::new(validated_input_rate, output_rate);
                            } else {
                                log::info!(
                                    "audio: callback rate validated at {observed_rate} Hz"
                                );
                            }
                        }
                        let mut samples = resampler.process(&mono);
                        apply_software_preamp(&mut samples);
                        let bytes = pcm_f32_to_le_bytes(&samples);
                        if !bytes.is_empty() {
                            let _ = active.tx.send(AudioChunk { data: bytes });
                        }
                    });
                },
                error_callback,
                None,
            )
        }};
    }

    let stream = match input_config.sample_format() {
        cpal::SampleFormat::F32 => build_stream!(f32, f32_to_mono),
        cpal::SampleFormat::I16 => build_stream!(i16, i16_to_mono),
        cpal::SampleFormat::U16 => build_stream!(u16, u16_to_mono),
        format => return Err(format!("Unsupported microphone sample format: {format:?}")),
    }
    .map_err(|error| format!("Failed to open microphone stream: {error}"))?;

    // Exercise native start/pause once during preparation. This validates that
    // the cached stream can start and leaves the microphone closed while idle.
    stream
        .play()
        .map_err(|error| format!("Failed to pre-warm microphone stream: {error}"))?;
    stream
        .pause()
        .map_err(|error| format!("Failed to pause pre-warmed microphone stream: {error}"))?;
    log::info!("audio: prepared paused microphone stream for {selection:?}");
    Ok(PreparedCapture {
        stream,
        selection,
        resolved_device_id,
        resolved_device_name,
        input_sample_rate: input_rate,
        output_sample_rate: output_rate,
        route,
        invalidated,
        active: false,
    })
}

impl AudioCapture {
    pub fn new() -> Self {
        let (commands, command_rx) = unbounded();
        let sample_rate = Arc::new(AtomicU32::new(FALLBACK_SAMPLE_RATE));
        let worker_rate = sample_rate.clone();
        thread::Builder::new()
            .name("audio-capture".into())
            .spawn(move || capture_worker(command_rx, worker_rate))
            .expect("failed to create audio capture worker");
        Self {
            sample_rate,
            commands,
            command_lock: Mutex::new(()),
            next_generation: AtomicU64::new(1),
        }
    }

    pub fn get_default_input_device(&self) -> Option<cpal::Device> {
        let host = cpal::default_host();
        host.default_input_device()
    }

    pub fn get_input_config(
        &self,
        device: &cpal::Device,
    ) -> Result<cpal::SupportedStreamConfigRange, String> {
        device
            .supported_input_configs()
            .map_err(|e| e.to_string())?
            .find(|c| c.max_sample_rate() >= 8_000)
            .ok_or("No suitable input config found".to_string())
    }

    pub fn get_sample_rate(&self) -> u32 {
        self.sample_rate.load(Ordering::SeqCst)
    }

    /// Resolve and pre-open the selected microphone. Concurrent calls are
    /// serialized and an unchanged selection reuses the existing paused stream.
    pub fn prepare_capture(&self, selection: AudioDeviceSelection) -> anyhow::Result<u32> {
        let _guard = self
            .command_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("Audio capture state is unavailable"))?;
        self.request(|reply| CaptureCommand::Prepare(selection, reply))
    }

    pub fn start_capture(
        &self,
        selection: AudioDeviceSelection,
        error_tx: Option<Sender<String>>,
    ) -> anyhow::Result<(Receiver<AudioChunk>, String)> {
        let _guard = self
            .command_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("Audio capture state is unavailable"))?;
        let (tx, rx) = unbounded::<AudioChunk>();
        let route = Arc::new(SessionRoute {
            tx,
            error_tx,
            generation: self.next_generation.fetch_add(1, Ordering::Relaxed),
            played_at: Mutex::new(Instant::now()),
            first_callback_seen: AtomicBool::new(false),
        });
        let (_rate, resolved_device_name) =
            self.request(|reply| CaptureCommand::Start(selection, route, reply))?;
        Ok((rx, resolved_device_name))
    }

    pub fn stop_capture(&self) -> anyhow::Result<()> {
        let _guard = self
            .command_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("Audio capture state is unavailable"))?;
        self.request(CaptureCommand::Stop)
    }

    fn request<T>(
        &self,
        command: impl FnOnce(Sender<Result<T, String>>) -> CaptureCommand,
    ) -> anyhow::Result<T> {
        let (reply_tx, reply_rx) = bounded(1);
        self.commands
            .send(command(reply_tx))
            .map_err(|_| anyhow::anyhow!("Audio capture worker stopped"))?;
        reply_rx
            .recv()
            .map_err(|_| anyhow::anyhow!("Audio capture worker stopped"))?
            .map_err(anyhow::Error::msg)
    }
}

impl Default for AudioCapture {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream_instant_nanos(nanos: u64) -> cpal::StreamInstant {
        cpal::StreamInstant::new(
            (nanos / 1_000_000_000) as i64,
            (nanos % 1_000_000_000) as u32,
        )
    }

    /// Feed `input` through the resampler in small chunks (mimicking the
    /// streaming audio callback) and collect all output samples.
    fn run_streaming(input_rate: u32, output_rate: u32, input: &[f32]) -> Vec<f32> {
        let mut resampler = AudioResampler::new(input_rate, output_rate);
        let mut out = Vec::new();
        for chunk in input.chunks(512) {
            out.extend(resampler.process(chunk));
        }
        out
    }

    fn ramp(n: usize) -> Vec<f32> {
        (0..n).map(|i| (i % 100) as f32 / 100.0).collect()
    }

    #[test]
    fn passthrough_at_matching_rate_is_identity() {
        let input = ramp(16_000);
        let out = run_streaming(16_000, 16_000, &input);
        assert_eq!(out.len(), input.len());
        assert_eq!(out, input);
    }

    #[test]
    fn passthrough_at_48k_preserves_length() {
        let input = ramp(48_000);
        let out = run_streaming(48_000, 48_000, &input);
        assert_eq!(out.len(), input.len());
        assert_eq!(out, input);
    }

    #[test]
    fn effective_rate_passthrough_for_gladia_compatible_rates() {
        assert_eq!(effective_output_sample_rate(8_000), 8_000);
        assert_eq!(effective_output_sample_rate(16_000), 16_000);
        assert_eq!(effective_output_sample_rate(32_000), 32_000);
        assert_eq!(effective_output_sample_rate(44_100), 44_100);
        assert_eq!(effective_output_sample_rate(48_000), 48_000);
    }

    #[test]
    fn effective_rate_resamples_incompatible_rates_to_16k() {
        assert_eq!(effective_output_sample_rate(96_000), 16_000);
        assert_eq!(effective_output_sample_rate(22_050), 16_000);
    }

    #[test]
    fn automatic_only_substitutes_bluetooth_defaults() {
        assert!(automatic_prefers_built_in("bluetooth", true));
        assert!(!automatic_prefers_built_in("bluetooth", false));
        assert!(!automatic_prefers_built_in("built_in", true));
        assert!(!automatic_prefers_built_in("usb", true));
        assert!(!automatic_prefers_built_in("unknown", true));
    }

    #[test]
    fn legacy_ipc_selection_remains_compatible() {
        assert_eq!(
            AudioDeviceSelection::from_ipc(None, None),
            AudioDeviceSelection::SystemDefault
        );
        assert_eq!(
            AudioDeviceSelection::from_ipc(None, Some("External mic".into())),
            AudioDeviceSelection::Specific {
                device_id: String::new(),
                device_name: "External mic".into(),
            }
        );
    }

    #[test]
    fn is_gladia_compatible_sample_rate_checks_supported_set() {
        assert!(is_gladia_compatible_sample_rate(48_000));
        assert!(!is_gladia_compatible_sample_rate(96_000));
    }

    #[test]
    fn estimates_common_rate_from_callback_cadence() {
        assert_eq!(
            estimate_common_sample_rate(441, Duration::from_millis(10)),
            Some(44_100)
        );
        assert_eq!(
            estimate_common_sample_rate(160, Duration::from_millis(10)),
            Some(16_000)
        );
    }

    #[test]
    fn rejects_callback_cadence_far_from_common_rates() {
        assert_eq!(
            estimate_common_sample_rate(270, Duration::from_millis(10)),
            None
        );
        assert_eq!(
            estimate_common_sample_rate(0, Duration::from_millis(10)),
            None
        );
        assert_eq!(estimate_common_sample_rate(441, Duration::ZERO), None);
    }

    #[test]
    fn callback_rate_requires_consistent_confirmations() {
        let mut validator = CallbackRateValidator::new();
        assert_eq!(validator.observe(stream_instant_nanos(0), 441), None);
        assert_eq!(
            validator.observe(stream_instant_nanos(10_000_000), 441),
            None
        );
        assert_eq!(
            validator.observe(stream_instant_nanos(20_000_000), 441),
            Some(44_100)
        );
        assert_eq!(
            validator.observe(stream_instant_nanos(30_000_000), 441),
            None
        );
    }

    #[test]
    fn callback_rate_tolerates_normal_timestamp_jitter() {
        let mut validator = CallbackRateValidator::new();
        assert_eq!(validator.observe(stream_instant_nanos(0), 441), None);
        assert_eq!(
            validator.observe(stream_instant_nanos(9_800_000), 441),
            None
        );
        assert_eq!(
            validator.observe(stream_instant_nanos(20_000_000), 441),
            Some(44_100)
        );
    }

    #[test]
    fn default_capture_refreshes_for_identity_rate_or_invalidation() {
        assert!(!default_capture_changed(
            Some("device-a"),
            44_100,
            Some("device-a"),
            44_100,
            false
        ));
        assert!(default_capture_changed(
            Some("device-a"),
            44_100,
            Some("device-b"),
            44_100,
            false
        ));
        assert!(default_capture_changed(
            Some("device-a"),
            16_000,
            Some("device-a"),
            44_100,
            false
        ));
        assert!(default_capture_changed(
            Some("device-a"),
            44_100,
            Some("device-a"),
            44_100,
            true
        ));
    }

    #[test]
    fn downsample_48k_to_16k_thirds_the_length() {
        let input = ramp(48_000); // 1 second at 48 kHz
        let out = run_streaming(48_000, 16_000, &input);
        let expected = 16_000;
        // Allow a small boundary tolerance from streaming chunk edges.
        assert!(
            (out.len() as i64 - expected as i64).abs() <= 4,
            "expected ~{expected} samples, got {}",
            out.len()
        );
    }

    #[test]
    fn downsample_44100_to_16k_matches_ratio() {
        let input = ramp(44_100); // 1 second at 44.1 kHz
        let out = run_streaming(44_100, 16_000, &input);
        let expected = (44_100u64 * 16_000 / 44_100) as i64; // == 16_000
        assert!(
            (out.len() as i64 - expected).abs() <= 4,
            "expected ~{expected} samples, got {}",
            out.len()
        );
    }

    #[test]
    fn upsample_8k_to_16k_doubles_the_length() {
        let input = ramp(8_000); // 1 second at 8 kHz
        let out = run_streaming(8_000, 16_000, &input);
        let expected = 16_000;
        assert!(
            (out.len() as i64 - expected as i64).abs() <= 4,
            "expected ~{expected} samples, got {}",
            out.len()
        );
    }

    #[test]
    fn downsampled_output_stays_within_range() {
        // A full-scale sine should never produce out-of-range samples after the
        // averaging low-pass + decimation.
        let input: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.1).sin()).collect();
        let out = run_streaming(48_000, 16_000, &input);
        assert!(out.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
    }
}
