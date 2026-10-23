use crate::audio::{AudioCapture, AudioDeviceSelection};
use crate::system_audio::{SystemAudioCapture, SYSTEM_AUDIO_SAMPLE_RATE};
use crossbeam_channel::{unbounded, Receiver};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub const MEETING_SAMPLE_RATE: u32 = SYSTEM_AUDIO_SAMPLE_RATE;
const FRAME_MILLIS: u64 = 20;
const SAMPLES_PER_FRAME: usize = (MEETING_SAMPLE_RATE as usize * FRAME_MILLIS as usize) / 1_000;
const FIRST_CHUNK_TIMEOUT: Duration = Duration::from_secs(3);
const SOURCE_STALL_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_RESTARTS_PER_SOURCE: usize = 3;
const RESTART_DELAYS: [Duration; MAX_RESTARTS_PER_SOURCE] = [
    Duration::from_millis(250),
    Duration::from_secs(1),
    Duration::from_secs(2),
];
type MeetingCaptureReceivers = (Receiver<Vec<u8>>, Receiver<String>, String);

pub struct MeetingAudioCapture {
    microphone: Arc<AudioCapture>,
    system: Arc<SystemAudioCapture>,
    worker: Mutex<Option<JoinHandle<()>>>,
    running: Arc<AtomicBool>,
}

impl MeetingAudioCapture {
    pub fn new(microphone: Arc<AudioCapture>) -> Self {
        Self {
            microphone,
            system: Arc::new(SystemAudioCapture::new()),
            worker: Mutex::new(None),
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn start(
        &self,
        selection: AudioDeviceSelection,
    ) -> Result<MeetingCaptureReceivers, String> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Err("Meeting audio capture is already running".to_string());
        }

        let (error_tx, error_rx) = unbounded();
        let (microphone_error_tx, microphone_error_rx) = unbounded();
        let (microphone_rx, microphone_name) = match self
            .microphone
            .start_capture(selection.clone(), Some(microphone_error_tx))
        {
            Ok(result) => result,
            Err(error) => {
                self.running.store(false, Ordering::SeqCst);
                return Err(error.to_string());
            }
        };
        let microphone_rate = self.microphone.get_sample_rate();
        let (system_rx, system_error_rx) = match self.system.start() {
            Ok(result) => result,
            Err(error) => {
                self.microphone.stop_capture().ok();
                self.running.store(false, Ordering::SeqCst);
                return Err(error);
            }
        };

        let (output_tx, output_rx) = unbounded();
        let running = self.running.clone();
        let microphone = self.microphone.clone();
        let system = self.system.clone();
        let worker = thread::Builder::new()
            .name("meeting-audio".into())
            .spawn(move || {
                let ticker = crossbeam_channel::tick(Duration::from_millis(FRAME_MILLIS));
                let timeline_origin = Instant::now();
                let mut emitted_samples = 0usize;
                let mut microphone_samples =
                    TimelineBuffer::new(microphone_rate, MEETING_SAMPLE_RATE);
                let mut system_samples =
                    TimelineBuffer::new(SYSTEM_AUDIO_SAMPLE_RATE, MEETING_SAMPLE_RATE);
                let mut microphone_rx = microphone_rx;
                let mut microphone_error_rx = microphone_error_rx;
                let mut system_rx = system_rx;
                let mut system_error_rx = system_error_rx;
                let mut microphone_last_chunk = timeline_origin;
                let mut system_last_chunk = timeline_origin;
                let mut microphone_restarts = 0usize;
                let mut system_restarts = 0usize;
                let mut microphone_exhausted_reported = false;
                let mut system_exhausted_reported = false;
                let mut system_nonzero_seen = false;
                let mut system_zero_since = None;
                let mut microphone_retry_after = timeline_origin;
                let mut system_retry_after = timeline_origin;

                while running.load(Ordering::SeqCst) {
                    if ticker.recv().is_err() {
                        break;
                    }

                    loop {
                        match microphone_rx.try_recv() {
                            Ok(chunk) => {
                                microphone_last_chunk = Instant::now();
                                microphone_samples.push_pcm_bytes(
                                    &chunk.data,
                                    chunk.captured_at,
                                    emitted_samples,
                                );
                            }
                            Err(crossbeam_channel::TryRecvError::Empty) => break,
                            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                                microphone_rx = crossbeam_channel::never();
                                microphone_last_chunk = timeline_origin;
                                break;
                            }
                        }
                    }

                    loop {
                        match system_rx.try_recv() {
                            Ok(chunk) => {
                                system_last_chunk = Instant::now();
                                if chunk.samples.iter().any(|sample| *sample != 0) {
                                    system_nonzero_seen = true;
                                    system_zero_since = None;
                                } else if system_nonzero_seen {
                                    system_zero_since.get_or_insert_with(Instant::now);
                                }
                                system_samples.set_input_rate(chunk.sample_rate);
                                system_samples.push_samples(
                                    &chunk.samples,
                                    chunk.captured_at,
                                    emitted_samples,
                                );
                            }
                            Err(crossbeam_channel::TryRecvError::Empty) => break,
                            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                                system_rx = crossbeam_channel::never();
                                system_last_chunk = timeline_origin;
                                break;
                            }
                        }
                    }

                    match microphone_error_rx.try_recv() {
                        Ok(_) => microphone_last_chunk = timeline_origin,
                        Err(crossbeam_channel::TryRecvError::Disconnected) => {
                            microphone_error_rx = crossbeam_channel::never()
                        }
                        Err(crossbeam_channel::TryRecvError::Empty) => {}
                    }
                    match system_error_rx.try_recv() {
                        Ok(_) => system_last_chunk = timeline_origin,
                        Err(crossbeam_channel::TryRecvError::Disconnected) => {
                            system_error_rx = crossbeam_channel::never()
                        }
                        Err(crossbeam_channel::TryRecvError::Empty) => {}
                    }

                    let now = Instant::now();
                    let microphone_stalled = now.duration_since(microphone_last_chunk)
                        >= if microphone_restarts == 0 {
                            FIRST_CHUNK_TIMEOUT
                        } else {
                            SOURCE_STALL_TIMEOUT
                        };
                    if microphone_stalled
                        && microphone_restarts < MAX_RESTARTS_PER_SOURCE
                        && now >= microphone_retry_after
                    {
                        microphone_retry_after = now + RESTART_DELAYS[microphone_restarts];
                        microphone_restarts += 1;
                        if running.load(Ordering::SeqCst) {
                            microphone.stop_capture().ok();
                            let (next_error_tx, next_error_rx) = unbounded();
                            match microphone
                                .start_capture(selection.clone(), Some(next_error_tx))
                            {
                                Ok((next_rx, _)) => {
                                    microphone_rx = next_rx;
                                    microphone_error_rx = next_error_rx;
                                    microphone_samples
                                        .set_input_rate(microphone.get_sample_rate());
                                    microphone_samples.reset_timeline();
                                    microphone_last_chunk = Instant::now();
                                    microphone_exhausted_reported = false;
                                }
                                Err(error)
                                    if microphone_restarts == MAX_RESTARTS_PER_SOURCE =>
                                {
                                    let _ = error_tx
                                        .send(format!("Microphone recovery failed: {error}"));
                                }
                                Err(_) => {}
                            }
                        }
                    } else if microphone_stalled && !microphone_exhausted_reported {
                        microphone_exhausted_reported = true;
                        let _ = error_tx
                            .send("Microphone recovery budget was exhausted".to_string());
                    }

                    let system_stalled = now.duration_since(system_last_chunk)
                        >= if system_restarts == 0 {
                            FIRST_CHUNK_TIMEOUT
                        } else {
                            SOURCE_STALL_TIMEOUT
                        }
                        || system_zero_since.is_some_and(|started| {
                            now.duration_since(started) >= Duration::from_secs(30)
                        });
                    if system_stalled
                        && system_restarts < MAX_RESTARTS_PER_SOURCE
                        && now >= system_retry_after
                    {
                        system_retry_after = now + RESTART_DELAYS[system_restarts];
                        system_restarts += 1;
                        if running.load(Ordering::SeqCst) {
                            system.stop().ok();
                            match system.start() {
                                Ok((next_rx, next_error_rx)) => {
                                    system_rx = next_rx;
                                    system_error_rx = next_error_rx;
                                    system_samples.reset_timeline();
                                    system_last_chunk = Instant::now();
                                    system_zero_since = None;
                                    system_exhausted_reported = false;
                                }
                                Err(error) if system_restarts == MAX_RESTARTS_PER_SOURCE => {
                                    let _ = error_tx
                                        .send(format!("System audio recovery failed: {error}"));
                                }
                                Err(_) => {}
                            }
                        }
                    } else if system_stalled && !system_exhausted_reported {
                        system_exhausted_reported = true;
                        let _ = error_tx.send(
                            "System audio recovery budget was exhausted; continuing with the owner channel"
                                .to_string(),
                        );
                    }

                    microphone_samples.bound_latency();
                    system_samples.bound_latency();
                    let frame = interleave_frame(
                        microphone_samples.samples_mut(),
                        system_samples.samples_mut(),
                        SAMPLES_PER_FRAME,
                    );
                    emitted_samples += SAMPLES_PER_FRAME;
                    if output_tx.send(frame).is_err() {
                        break;
                    }
                }
            })
            .map_err(|error| {
                self.microphone.stop_capture().ok();
                self.system.stop().ok();
                self.running.store(false, Ordering::SeqCst);
                error.to_string()
            })?;
        *self.worker.lock().map_err(|error| error.to_string())? = Some(worker);
        Ok((output_rx, error_rx, microphone_name))
    }

    pub fn stop(&self) -> Result<(), String> {
        self.running.store(false, Ordering::SeqCst);
        let microphone_result = self.microphone.stop_capture().map_err(|e| e.to_string());
        let system_result = self.system.stop();
        if let Some(worker) = self.worker.lock().map_err(|e| e.to_string())?.take() {
            worker
                .join()
                .map_err(|_| "Meeting audio worker panicked".to_string())?;
        }
        microphone_result?;
        system_result
    }
}

struct TimelineBuffer {
    samples: VecDeque<i16>,
    converter: RateConverter,
    input_rate: u32,
    output_rate: u32,
    anchor_capture: Option<Instant>,
    anchor_output_sample: usize,
}

impl TimelineBuffer {
    fn new(input_rate: u32, output_rate: u32) -> Self {
        Self {
            samples: VecDeque::new(),
            converter: RateConverter::new(input_rate, output_rate),
            input_rate,
            output_rate,
            anchor_capture: None,
            anchor_output_sample: 0,
        }
    }

    fn set_input_rate(&mut self, input_rate: u32) {
        if self.input_rate != input_rate {
            self.input_rate = input_rate;
            self.converter = RateConverter::new(input_rate, self.output_rate);
            self.anchor_capture = None;
        }
    }

    fn reset_timeline(&mut self) {
        self.samples.clear();
        self.converter = RateConverter::new(self.input_rate, self.output_rate);
        self.anchor_capture = None;
        self.anchor_output_sample = 0;
    }

    fn push_pcm_bytes(&mut self, bytes: &[u8], captured_at: Instant, emitted_samples: usize) {
        let samples = bytes
            .chunks_exact(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        self.push_samples(&samples, captured_at, emitted_samples);
    }

    fn push_samples(&mut self, samples: &[i16], captured_at: Instant, emitted_samples: usize) {
        let mut converted = VecDeque::new();
        self.converter.push_samples(samples, &mut converted);
        let buffered_end = emitted_samples.saturating_add(self.samples.len());
        let anchor_capture = match self.anchor_capture {
            Some(anchor) => anchor,
            None => {
                self.anchor_capture = Some(captured_at);
                self.anchor_output_sample = buffered_end;
                captured_at
            }
        };
        let desired_start = self.anchor_output_sample.saturating_add(
            (captured_at
                .saturating_duration_since(anchor_capture)
                .as_secs_f64()
                * self.output_rate as f64)
                .round() as usize,
        );
        let reanchor_threshold = SAMPLES_PER_FRAME * 2;
        let maximum_adjustment = SAMPLES_PER_FRAME / 4;
        if desired_start > buffered_end {
            let gap = desired_start - buffered_end;
            if gap > reanchor_threshold {
                self.anchor_capture = Some(captured_at);
                self.anchor_output_sample = buffered_end;
            } else {
                self.samples
                    .extend(std::iter::repeat_n(0, gap.min(maximum_adjustment)));
            }
        } else {
            let overlap = buffered_end - desired_start;
            if overlap > reanchor_threshold {
                self.anchor_capture = Some(captured_at);
                self.anchor_output_sample = buffered_end;
            } else {
                converted.drain(..overlap.min(maximum_adjustment).min(converted.len()));
            }
        }
        self.samples.extend(converted);
    }

    fn bound_latency(&mut self) {
        let maximum = SAMPLES_PER_FRAME * 10;
        if self.samples.len() > maximum {
            self.samples.drain(..self.samples.len() - maximum);
            self.anchor_capture = None;
        }
    }

    fn samples_mut(&mut self) -> &mut VecDeque<i16> {
        &mut self.samples
    }
}

struct RateConverter {
    input_rate: u32,
    output_rate: u32,
    accumulator: u32,
}

impl RateConverter {
    fn new(input_rate: u32, output_rate: u32) -> Self {
        Self {
            input_rate,
            output_rate,
            accumulator: 0,
        }
    }

    fn push_samples(&mut self, samples: &[i16], output: &mut VecDeque<i16>) {
        for &sample in samples {
            self.accumulator = self.accumulator.saturating_add(self.output_rate);
            while self.accumulator >= self.input_rate {
                output.push_back(sample);
                self.accumulator -= self.input_rate;
            }
        }
    }
}

fn interleave_frame(
    microphone: &mut VecDeque<i16>,
    system: &mut VecDeque<i16>,
    samples: usize,
) -> Vec<u8> {
    let mut output = Vec::with_capacity(samples * 4);
    for _ in 0..samples {
        output.extend_from_slice(&microphone.pop_front().unwrap_or_default().to_le_bytes());
        output.extend_from_slice(&system.pop_front().unwrap_or_default().to_le_bytes());
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interleaves_owner_then_system_and_pads_silence() {
        let mut microphone = VecDeque::from([1, 2]);
        let mut system = VecDeque::from([10]);
        let bytes = interleave_frame(&mut microphone, &mut system, 2);
        let samples: Vec<i16> = bytes
            .chunks_exact(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        assert_eq!(samples, vec![1, 10, 2, 0]);
    }

    #[test]
    fn rate_converter_preserves_streaming_state() {
        let mut converter = RateConverter::new(24_000, 48_000);
        let mut output = VecDeque::new();
        converter.push_samples(&[1], &mut output);
        converter.push_samples(&[2], &mut output);
        assert_eq!(output.into_iter().collect::<Vec<_>>(), vec![1, 1, 2, 2]);
    }

    #[test]
    fn drift_backlog_is_bounded() {
        let mut buffer = TimelineBuffer::new(48_000, 48_000);
        buffer.samples = VecDeque::from(vec![7; SAMPLES_PER_FRAME * 60]);
        buffer.bound_latency();
        assert_eq!(buffer.samples.len(), SAMPLES_PER_FRAME * 10);
        assert_eq!(buffer.anchor_capture, None);
    }

    #[test]
    fn large_timestamp_gap_reanchors_without_inserting_silence() {
        let origin = Instant::now();
        let mut buffer = TimelineBuffer::new(48_000, 48_000);
        buffer.push_samples(&[1], origin, 0);
        buffer.push_samples(&[2], origin + Duration::from_secs(1), 0);
        assert_eq!(buffer.samples.into_iter().collect::<Vec<_>>(), vec![1, 2]);
    }

    #[test]
    fn small_timestamp_drift_has_a_strict_correction_cap() {
        let origin = Instant::now();
        let mut buffer = TimelineBuffer::new(48_000, 48_000);
        buffer.push_samples(&[1], origin, 0);
        buffer.push_samples(&[2], origin + Duration::from_millis(20), 0);
        assert_eq!(buffer.samples.len(), SAMPLES_PER_FRAME / 4 + 2);
        assert_eq!(buffer.samples.back(), Some(&2));
    }
}
