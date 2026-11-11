//! Microphone capture with live audio level, adaptive VAD utterance segmentation,
//! and automatic silence shutoff. Recording is cross-platform (cpal); permission
//! handling is strictly per OS in `perm` so macOS and Linux never share a branch.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

/// Events emitted by the live capture loop while the microphone is open.
pub enum MicChunk {
    /// Rolling snapshot of the current phrase (used for live word-by-word preview on OS STT).
    #[allow(dead_code)]
    Partial { id: u64, samples: Vec<i16>, rate: u32 },
    /// Completed phrase after a natural pause (or flushed when manually stopped).
    Utterance { samples: Vec<i16>, rate: u32 },
    /// Prolonged silence: microphone stopped itself automatically.
    AutoStop { heard_speech: bool },
}

/// A running continuous recording. Dropping it aborts capture immediately.
pub struct Recording {
    /// Send `true` to flush any trailing speech and finish; `false` to abort on drop.
    stop_tx: mpsc::Sender<bool>,
    level: Arc<AtomicU32>,
    alive: Arc<AtomicBool>,
}

impl Recording {
    /// Current smoothed voice energy in `0.0..=1.0` for waveform rendering.
    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

    /// Stop capture cleanly, flushing any unfinished phrase to the chunk channel.
    pub fn finish(self) {
        let _ = self.stop_tx.send(true);
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(false);
    }
}

fn send_mono<T: Copy>(tx: &mpsc::Sender<Vec<i16>>, data: &[T], channels: usize, conv: impl Fn(T) -> i16) {
    let ch = channels.max(1);
    let mut mono = Vec::with_capacity(data.len() / ch + 1);
    for frame in data.chunks(ch) {
        let sum: i32 = frame.iter().map(|s| conv(*s) as i32).sum();
        mono.push((sum / frame.len() as i32) as i16);
    }
    let _ = tx.send(mono);
}

pub fn start(
    emit_partials: bool,
    chunk_tx: tokio::sync::mpsc::UnboundedSender<MicChunk>,
) -> Result<Recording, String> {
    let (stop_tx, stop_rx) = mpsc::channel::<bool>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    let level = Arc::new(AtomicU32::new(0.0f32.to_bits()));
    let alive = Arc::new(AtomicBool::new(true));
    let level_bg = level.clone();
    let alive_bg = alive.clone();

    // cpal::Stream is not Send on every OS, so it lives and dies on its own thread.
    std::thread::spawn(move || {
        let (audio_tx, audio_rx) = mpsc::channel::<Vec<i16>>();
        let built = (|| -> Result<(cpal::Stream, u32), String> {
            let device = cpal::default_host().default_input_device().ok_or("no microphone found")?;
            let cfg = device.default_input_config().map_err(|e| e.to_string())?;
            let rate = cfg.sample_rate().0;
            let ch = cfg.channels() as usize;
            let err_fn = |e| eprintln!("[jink] mic error: {e}");
            let config: cpal::StreamConfig = cfg.clone().into();
            let stream = match cfg.sample_format() {
                SampleFormat::F32 => {
                    let tx = audio_tx.clone();
                    device.build_input_stream(
                        &config,
                        move |d: &[f32], _| send_mono(&tx, d, ch, |s| (s.clamp(-1.0, 1.0) * 32767.0) as i16),
                        err_fn,
                        None,
                    )
                }
                SampleFormat::I16 => {
                    let tx = audio_tx.clone();
                    device.build_input_stream(&config, move |d: &[i16], _| send_mono(&tx, d, ch, |s| s), err_fn, None)
                }
                SampleFormat::U16 => {
                    let tx = audio_tx.clone();
                    device.build_input_stream(
                        &config,
                        move |d: &[u16], _| send_mono(&tx, d, ch, |s| (s as i32 - 32768) as i16),
                        err_fn,
                        None,
                    )
                }
                other => return Err(format!("unsupported sample format {other:?}")),
            }
            .map_err(|e| e.to_string())?;
            stream.play().map_err(|e| e.to_string())?;
            Ok((stream, rate))
        })();

        let (stream, rate) = match built {
            Ok(v) => {
                let _ = ready_tx.send(Ok(()));
                v
            }
            Err(e) => {
                alive_bg.store(false, Ordering::Relaxed);
                let _ = ready_tx.send(Err(e));
                return;
            }
        };

        run_vad_loop(rate, emit_partials, &audio_rx, &stop_rx, &level_bg, &chunk_tx);
        drop(stream);
        level_bg.store(0.0f32.to_bits(), Ordering::Relaxed);
        alive_bg.store(false, Ordering::Relaxed);
    });

    match ready_rx.recv() {
        Ok(Ok(())) => Ok(Recording { stop_tx, level, alive }),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("microphone thread failed".into()),
    }
}

fn run_vad_loop(
    rate: u32,
    emit_partials: bool,
    audio_rx: &mpsc::Receiver<Vec<i16>>,
    stop_rx: &mpsc::Receiver<bool>,
    level: &AtomicU32,
    chunk_tx: &tokio::sync::mpsc::UnboundedSender<MicChunk>,
) {
    let frame_len = (rate as usize / 50).max(1); // 20ms per VAD frame
    let preroll_cap = (rate as usize * 3 / 10).max(frame_len); // 300ms pre-roll
    let partial_step = (rate as usize * 85 / 100).max(frame_len); // ~850ms between live previews
    let max_utterance = rate as usize * 20; // 20s safety cap per phrase

    let mut pending: VecDeque<i16> = VecDeque::new();
    let mut preroll: VecDeque<i16> = VecDeque::with_capacity(preroll_cap + frame_len);
    let mut utterance: Vec<i16> = Vec::new();

    let mut smooth_lvl = 0.0f32;
    let mut noise_floor = 0.006f32;
    let mut in_speech = false;
    let mut onset_voiced = 0u32;
    let mut voiced_frames = 0u32;
    let mut silence_frames = 0u32;
    let mut last_partial_len = 0usize;
    let mut utterance_id = 1u64;
    let mut heard_speech = false;

    loop {
        if let Ok(flush) = stop_rx.try_recv() {
            if flush && in_speech && voiced_frames >= 5 && !utterance.is_empty() {
                let _ = chunk_tx.send(MicChunk::Utterance {
                    samples: utterance,
                    rate,
                });
            }
            break;
        }

        match audio_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(samples) => pending.extend(samples),
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        while pending.len() >= frame_len {
            let mut sum_sq = 0.0f64;
            let mut frame = Vec::with_capacity(frame_len);
            for _ in 0..frame_len {
                let s = pending.pop_front().unwrap_or(0);
                let norm = s as f64 / 32768.0;
                sum_sq += norm * norm;
                frame.push(s);
            }
            let rms = (sum_sq / frame_len as f64).sqrt() as f32;

            // Perceptual level for live waveform bars
            let vis = (rms * 8.5).sqrt().clamp(0.0, 1.0);
            smooth_lvl = if vis > smooth_lvl {
                smooth_lvl * 0.4 + vis * 0.6
            } else {
                smooth_lvl * 0.72 + vis * 0.28
            };
            level.store(smooth_lvl.to_bits(), Ordering::Relaxed);

            // Adaptive noise floor with hysteresis between speech onset and sustain
            let enter_thresh = (noise_floor * 2.8).max(0.012);
            let sustain_thresh = (noise_floor * 2.0).max(0.008);

            if !in_speech {
                if rms < noise_floor * 2.2 {
                    noise_floor = (noise_floor * 0.96 + rms * 0.04).clamp(0.002, 0.03);
                }
                for &s in &frame {
                    if preroll.len() >= preroll_cap {
                        preroll.pop_front();
                    }
                    preroll.push_back(s);
                }

                if rms >= enter_thresh {
                    onset_voiced += 1;
                    silence_frames = 0;
                    if onset_voiced >= 3 {
                        // 60ms of solid voice: start phrase with the 300ms pre-roll intact
                        in_speech = true;
                        utterance.clear();
                        utterance.extend(preroll.iter().copied());
                        voiced_frames = onset_voiced;
                        onset_voiced = 0;
                        last_partial_len = 0;
                    }
                } else {
                    onset_voiced = 0;
                    silence_frames += 1;
                    // Auto-stop: ~3.5s after speaking a phrase (0.76s commit + 2.8s idle),
                    // or 8.0s if no speech happened at all.
                    let limit = if heard_speech { 140 } else { 400 };
                    if silence_frames >= limit {
                        let _ = chunk_tx.send(MicChunk::AutoStop { heard_speech });
                        return;
                    }
                }
            } else {
                utterance.extend_from_slice(&frame);
                if rms >= sustain_thresh {
                    voiced_frames += 1;
                    silence_frames = 0;
                } else {
                    silence_frames += 1;
                }

                if emit_partials
                    && voiced_frames >= 14
                    && utterance.len().saturating_sub(last_partial_len) >= partial_step
                {
                    last_partial_len = utterance.len();
                    let _ = chunk_tx.send(MicChunk::Partial {
                        id: utterance_id,
                        samples: utterance.clone(),
                        rate,
                    });
                }

                // 760ms pause (38 * 20ms) ends the current phrase and keeps listening for the next
                if silence_frames >= 38 || utterance.len() >= max_utterance {
                    if voiced_frames >= 8 {
                        heard_speech = true;
                        let finished = std::mem::take(&mut utterance);
                        if chunk_tx
                            .send(MicChunk::Utterance {
                                samples: finished,
                                rate,
                            })
                            .is_err()
                        {
                            return;
                        }
                        utterance_id += 1;
                    } else {
                        utterance.clear();
                    }
                    preroll.clear();
                    in_speech = false;
                    voiced_frames = 0;
                    silence_frames = 0;
                    last_partial_len = 0;
                }
            }
        }
    }
}

/// 16-bit PCM mono WAV.
pub fn to_wav(samples: &[i16], rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut w = Vec::with_capacity(44 + data_len as usize);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data_len).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&1u16.to_le_bytes()); // mono
    w.extend_from_slice(&rate.to_le_bytes());
    w.extend_from_slice(&(rate * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        w.extend_from_slice(&s.to_le_bytes());
    }
    w
}

#[derive(PartialEq, Eq)]
pub enum Access {
    Granted,
    /// macOS has not asked yet: starting the capture triggers the system prompt.
    Undecided,
    Denied,
}

/// Microphone permission, one implementation per OS.
pub mod perm {
    #![allow(unexpected_cfgs)]
    pub use super::Access;

    #[cfg(target_os = "macos")]
    pub fn check() -> Access {
        use objc::runtime::Object;
        use objc::{class, msg_send, sel, sel_impl};
        #[link(name = "AVFoundation", kind = "framework")]
        extern "C" {}
        unsafe {
            let media: *mut Object = msg_send![class!(NSString), stringWithUTF8String: b"soun\0".as_ptr()];
            let status: i64 = msg_send![class!(AVCaptureDevice), authorizationStatusForMediaType: media];
            match status {
                0 => Access::Undecided,
                3 => Access::Granted,
                _ => Access::Denied, // restricted / denied
            }
        }
    }

    /// Open System Settings → Privacy & Security → Microphone.
    #[cfg(target_os = "macos")]
    pub fn open_settings() {
        let _ = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
            .spawn();
    }

    /// Linux has no per-app microphone permission prompt.
    #[cfg(not(target_os = "macos"))]
    pub fn check() -> Access {
        Access::Granted
    }

    #[cfg(not(target_os = "macos"))]
    pub fn open_settings() {}
}
