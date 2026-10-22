use crossbeam_channel::{unbounded, Receiver, Sender};
use serde::Serialize;
use std::time::Instant;

pub const SYSTEM_AUDIO_SAMPLE_RATE: u32 = 48_000;

#[derive(Debug)]
pub struct SystemAudioChunk {
    pub samples: Vec<i16>,
    pub sample_rate: u32,
    pub captured_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemAudioBackend {
    CoreAudioProcessTap,
    ScreenCaptureKit,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemAudioPermissionStatus {
    Authorized,
    Denied,
    NotDetermined,
    Unsupported,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemAudioCapability {
    pub backend: SystemAudioBackend,
    pub permission: SystemAudioPermissionStatus,
    pub requires_restart: bool,
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use screencapturekit::prelude::*;
    use std::process::Command;
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::sync::Mutex;
    use std::time::Duration;

    const PERMISSION_UNKNOWN: u8 = 0;
    const PERMISSION_AUTHORIZED: u8 = 1;
    const PERMISSION_DENIED: u8 = 2;
    static PROCESS_TAP_PERMISSION: AtomicU8 = AtomicU8::new(PERMISSION_UNKNOWN);

    struct AudioHandler {
        tx: Sender<SystemAudioChunk>,
        error_tx: Sender<String>,
    }

    impl SCStreamOutputTrait for AudioHandler {
        fn did_output_sample_buffer(
            &self,
            sample: CMSampleBuffer,
            output_type: SCStreamOutputType,
        ) {
            if output_type != SCStreamOutputType::Audio {
                return;
            }
            let buffers = match sample.audio_buffer_list() {
                Ok(buffers) => buffers,
                Err(status) => {
                    let _ = self
                        .error_tx
                        .send(format!("Could not read system audio buffer ({status})"));
                    return;
                }
            };

            let mut samples = Vec::new();
            for buffer in &buffers {
                samples.extend(buffer.data().chunks_exact(4).map(|bytes| {
                    let value = f32::from_le_bytes(bytes.try_into().unwrap_or([0; 4]));
                    (value.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
                }));
            }
            if !samples.is_empty() {
                let duration =
                    Duration::from_secs_f64(samples.len() as f64 / SYSTEM_AUDIO_SAMPLE_RATE as f64);
                let captured_at = Instant::now()
                    .checked_sub(duration)
                    .unwrap_or_else(Instant::now);
                let _ = self.tx.send(SystemAudioChunk {
                    samples,
                    sample_rate: SYSTEM_AUDIO_SAMPLE_RATE,
                    captured_at,
                });
            }
        }
    }

    enum ActiveCapture {
        ProcessTap { _stream: cpal::Stream },
        ScreenCaptureKit(SCStream),
    }

    pub struct SystemAudioCapture {
        stream: Mutex<Option<ActiveCapture>>,
    }

    impl SystemAudioCapture {
        pub fn new() -> Self {
            Self {
                stream: Mutex::new(None),
            }
        }

        pub fn start(&self) -> Result<(Receiver<SystemAudioChunk>, Receiver<String>), String> {
            let mut active = self.stream.lock().map_err(|error| error.to_string())?;
            if active.is_some() {
                return Err("System audio capture is already running".to_string());
            }

            let (rx, error_rx, capture) = match selected_backend() {
                SystemAudioBackend::CoreAudioProcessTap => start_process_tap()?,
                SystemAudioBackend::ScreenCaptureKit => start_screen_capture_kit()?,
                SystemAudioBackend::Unsupported => {
                    return Err("Meeting recording requires macOS 13 or later".to_string())
                }
            };
            *active = Some(capture);
            Ok((rx, error_rx))
        }

        pub fn stop(&self) -> Result<(), String> {
            let mut active = self.stream.lock().map_err(|error| error.to_string())?;
            if let Some(ActiveCapture::ScreenCaptureKit(stream)) = active.take() {
                stream
                    .stop_capture()
                    .map_err(|error| format!("Could not stop system audio capture: {error}"))?;
            }
            Ok(())
        }
    }

    fn start_screen_capture_kit(
    ) -> Result<(Receiver<SystemAudioChunk>, Receiver<String>, ActiveCapture), String> {
        let content = SCShareableContent::get()
            .map_err(|error| format!("Screen Recording permission is required: {error}"))?;
        let display = content
            .displays()
            .into_iter()
            .next()
            .ok_or_else(|| "No display is available for system audio capture".to_string())?;
        let filter = SCContentFilter::create()
            .with_display(&display)
            .with_excluding_windows(&[])
            .build()
            .map_err(|error| error.to_string())?;
        let configuration = SCStreamConfiguration::new()
            .with_width(2)
            .with_height(2)
            .with_captures_audio(true)
            .with_excludes_current_process_audio(true)
            .with_sample_rate(SYSTEM_AUDIO_SAMPLE_RATE as i32)
            .with_channel_count(1);
        let (tx, rx) = unbounded();
        let (error_tx, error_rx) = unbounded();
        let mut stream =
            SCStream::new(&filter, &configuration).map_err(|error| error.to_string())?;
        stream
            .add_output_handler(
                AudioHandler {
                    tx,
                    error_tx: error_tx.clone(),
                },
                SCStreamOutputType::Audio,
            )
            .map_err(|error| error.to_string())?;
        stream
            .start_capture()
            .map_err(|error| format!("Could not start system audio capture: {error}"))?;
        Ok((rx, error_rx, ActiveCapture::ScreenCaptureKit(stream)))
    }

    fn start_process_tap(
    ) -> Result<(Receiver<SystemAudioChunk>, Receiver<String>, ActiveCapture), String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "No output device is available for system audio capture".to_string())?;
        let supported = device
            .default_output_config()
            .map_err(|error| format!("Could not inspect the output device: {error}"))?;
        let sample_rate = supported.sample_rate();
        let channels = supported.channels() as usize;
        let config = supported.config();
        let (tx, rx) = unbounded();
        let (error_tx, error_rx) = unbounded();

        macro_rules! build_stream {
            ($sample:ty, $convert:expr) => {{
                let chunk_tx = tx.clone();
                let stream_error_tx = error_tx.clone();
                device.build_input_stream(
                    config.clone(),
                    move |data: &[$sample], info: &cpal::InputCallbackInfo| {
                        let mut samples = Vec::with_capacity(data.len() / channels.max(1));
                        for frame in data.chunks(channels.max(1)) {
                            let sum: f32 = frame.iter().map(|sample| $convert(*sample)).sum();
                            let mono = (sum / frame.len().max(1) as f32).clamp(-1.0, 1.0);
                            samples.push((mono * i16::MAX as f32) as i16);
                        }
                        if samples.is_empty() {
                            return;
                        }
                        let latency = info
                            .timestamp()
                            .callback
                            .saturating_duration_since(info.timestamp().capture);
                        let captured_at = Instant::now()
                            .checked_sub(latency)
                            .unwrap_or_else(Instant::now);
                        let _ = chunk_tx.send(SystemAudioChunk {
                            samples,
                            sample_rate,
                            captured_at,
                        });
                    },
                    move |error| {
                        let _ = stream_error_tx
                            .send(format!("CoreAudio process-tap stream failed: {error}"));
                    },
                    None,
                )
            }};
        }

        let stream_result = match supported.sample_format() {
            cpal::SampleFormat::F32 => build_stream!(f32, |sample: f32| sample),
            cpal::SampleFormat::I16 => {
                build_stream!(i16, |sample: i16| sample as f32 / i16::MAX as f32)
            }
            cpal::SampleFormat::U16 => {
                build_stream!(u16, |sample: u16| sample as f32 / u16::MAX as f32 * 2.0
                    - 1.0)
            }
            format => {
                return Err(format!(
                    "Unsupported system-audio sample format: {format:?}"
                ))
            }
        };
        let stream = stream_result.map_err(|error| {
            mark_process_tap_failure(&error.to_string());
            format!("Could not create the CoreAudio process tap: {error}")
        })?;
        stream.play().map_err(|error| {
            mark_process_tap_failure(&error.to_string());
            format!("Could not start CoreAudio process tap: {error}")
        })?;
        PROCESS_TAP_PERMISSION.store(PERMISSION_AUTHORIZED, Ordering::Release);
        Ok((rx, error_rx, ActiveCapture::ProcessTap { _stream: stream }))
    }

    fn mark_process_tap_failure(message: &str) {
        let message = message.to_ascii_lowercase();
        if message.contains("permission")
            || message.contains("not permitted")
            || message.contains("1885696621")
        {
            PROCESS_TAP_PERMISSION.store(PERMISSION_DENIED, Ordering::Release);
        }
    }

    fn parse_macos_version(value: &str) -> Option<(u32, u32)> {
        let mut parts = value.trim().split('.');
        Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
    }

    fn macos_version() -> Option<(u32, u32)> {
        let output = Command::new("/usr/bin/sw_vers")
            .arg("-productVersion")
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        parse_macos_version(std::str::from_utf8(&output.stdout).ok()?)
    }

    fn selected_backend() -> SystemAudioBackend {
        match macos_version() {
            Some((major, minor)) if major > 14 || major == 14 && minor >= 4 => {
                SystemAudioBackend::CoreAudioProcessTap
            }
            Some((major, _)) if major >= 13 => SystemAudioBackend::ScreenCaptureKit,
            _ => SystemAudioBackend::Unsupported,
        }
    }

    pub fn capability() -> SystemAudioCapability {
        let backend = selected_backend();
        let permission = match backend {
            SystemAudioBackend::CoreAudioProcessTap => {
                match PROCESS_TAP_PERMISSION.load(Ordering::Acquire) {
                    PERMISSION_AUTHORIZED => SystemAudioPermissionStatus::Authorized,
                    PERMISSION_DENIED => SystemAudioPermissionStatus::Denied,
                    _ => SystemAudioPermissionStatus::NotDetermined,
                }
            }
            SystemAudioBackend::ScreenCaptureKit => {
                if core_graphics::access::ScreenCaptureAccess.preflight() {
                    SystemAudioPermissionStatus::Authorized
                } else {
                    SystemAudioPermissionStatus::NotDetermined
                }
            }
            SystemAudioBackend::Unsupported => SystemAudioPermissionStatus::Unsupported,
        };
        SystemAudioCapability {
            backend,
            permission,
            requires_restart: false,
        }
    }

    pub fn request_permission() -> Result<SystemAudioCapability, String> {
        match selected_backend() {
            SystemAudioBackend::CoreAudioProcessTap => {
                let capture = SystemAudioCapture::new();
                let (audio, _) = capture.start()?;
                let _ = audio.recv_timeout(Duration::from_secs(1));
                capture.stop()?;
                Ok(capability())
            }
            SystemAudioBackend::ScreenCaptureKit => {
                let granted = core_graphics::access::ScreenCaptureAccess.request();
                Ok(SystemAudioCapability {
                    backend: SystemAudioBackend::ScreenCaptureKit,
                    permission: if granted {
                        SystemAudioPermissionStatus::Authorized
                    } else {
                        SystemAudioPermissionStatus::Denied
                    },
                    requires_restart: granted,
                })
            }
            SystemAudioBackend::Unsupported => Ok(capability()),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::parse_macos_version;

        #[test]
        fn parses_macos_product_versions() {
            assert_eq!(parse_macos_version("14.4.1\n"), Some((14, 4)));
            assert_eq!(parse_macos_version("26.0"), Some((26, 0)));
            assert_eq!(parse_macos_version("invalid"), None);
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::*;

    pub struct SystemAudioCapture;

    impl SystemAudioCapture {
        pub fn new() -> Self {
            Self
        }

        pub fn start(&self) -> Result<(Receiver<SystemAudioChunk>, Receiver<String>), String> {
            Err("Meeting recording is only available on macOS".to_string())
        }

        pub fn stop(&self) -> Result<(), String> {
            Ok(())
        }
    }

    pub fn capability() -> SystemAudioCapability {
        SystemAudioCapability {
            backend: SystemAudioBackend::Unsupported,
            permission: SystemAudioPermissionStatus::Unsupported,
            requires_restart: false,
        }
    }

    pub fn request_permission() -> Result<SystemAudioCapability, String> {
        Ok(capability())
    }
}

pub use platform::{capability, request_permission, SystemAudioCapture};
