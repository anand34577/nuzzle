//! Microphone capture (-> 16 kHz mono PCM16 chunks) and speaker playback (mono float in), on the devices the user
//! chose (or the Windows default). cpal streams are not `Send`, so they live on one audio thread, which also rebuilds
//! them when a device errors or disappears, when a missing chosen device comes back, and (when following the
//! default) when the default device changes. Microphone and speaker are independent: one failing doesn't stop the other.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, Stream};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc as std_mpsc, Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

use crate::LockExt;

const IN_RATE: u32 = 16_000;
/// Keep the mic closed this long after the last played sample so the speakers don't
/// feed the model its own voice.
const ECHO_TAIL_MS: u64 = 450;
/// How often the audio thread checks that its devices are still the right ones.
const CHECK_EVERY: Duration = Duration::from_secs(2);

pub fn now_ms() -> u64 {
    static T0: OnceLock<Instant> = OnceLock::new();
    T0.get_or_init(Instant::now).elapsed().as_millis() as u64
}

#[derive(Default)]
struct Resampler {
    pos: f64,
    last: f32,
    rate: u32,
}

/// Which devices to use, by name. Empty = follow the Windows default.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Choice {
    pub mic: String,
    pub speaker: String,
}

enum Cmd {
    Use(Choice),
}

/// State shared between the handle and the audio thread's streams.
struct Shared {
    buf: Mutex<VecDeque<f32>>,
    out_rate: AtomicU32,
    level_in: AtomicU32,
    level_out: AtomicU32,
    speaking_until: AtomicU64,
    /// Playback volume in percent.
    volume: AtomicU32,
    /// Problems and recoveries worth telling the user about (drained by the UI loop).
    notes: Mutex<Vec<String>>,
    /// Names of the devices in use right now ("" = none).
    current: Mutex<(String, String)>,
    mic_tx: mpsc::Sender<Vec<u8>>,
    mic_on: Arc<AtomicBool>,
}

pub struct Audio {
    sh: Arc<Shared>,
    rs: Mutex<Resampler>,
    cmd: std_mpsc::Sender<Cmd>,
}

fn load(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Ordering::Relaxed))
}

impl Audio {
    pub fn level_in(&self) -> f32 {
        load(&self.sh.level_in)
    }
    pub fn level_out(&self) -> f32 {
        load(&self.sh.level_out)
    }

    /// Keep the mic gated for at least `ms` more (e.g. while speech is being synthesised).
    pub fn hold(&self, ms: u64) {
        self.sh.speaking_until.fetch_max(now_ms() + ms, Ordering::Relaxed);
    }

    /// Open the echo gate now (the user wants to talk over me).
    pub fn release_hold(&self) {
        self.sh.speaking_until.store(0, Ordering::Relaxed);
    }

    pub fn flush(&self) {
        self.sh.buf.lk().clear();
        *self.rs.lk() = Resampler::default();
    }

    /// Audio is queued or playing right now.
    pub fn is_playing(&self) -> bool {
        !self.sh.buf.lk().is_empty()
    }

    pub fn set_volume(&self, pct: u32) {
        self.sh.volume.store(pct.min(150), Ordering::Relaxed);
    }

    /// Switch devices (applied on the audio thread within moments).
    pub fn use_devices(&self, choice: Choice) {
        let _ = self.cmd.send(Cmd::Use(choice));
    }

    /// Messages about devices going away or coming back, oldest first.
    pub fn take_notes(&self) -> Vec<String> {
        std::mem::take(&mut *self.sh.notes.lk())
    }

    /// (microphone, speaker) names in use right now; empty when there is none.
    pub fn current(&self) -> (String, String) {
        self.sh.current.lk().clone()
    }

    /// Queue mono float samples recorded at `src_rate` Hz for playback (dropped when there is no speaker).
    pub fn push(&self, samples: &[f32], src_rate: f64) {
        let out_rate = self.sh.out_rate.load(Ordering::Relaxed);
        if out_rate == 0 || samples.is_empty() {
            return;
        }
        let mut rs = self.rs.lk();
        if rs.rate != out_rate {
            *rs = Resampler { rate: out_rate, ..Resampler::default() };
        }
        let mut ext = Vec::with_capacity(samples.len() + 1);
        ext.push(rs.last);
        ext.extend_from_slice(samples);
        let ratio = src_rate / f64::from(out_rate);
        let mut out = Vec::new();
        let mut pos = rs.pos;
        while pos < (ext.len() - 1) as f64 {
            let i = pos as usize;
            let f = (pos - i as f64) as f32;
            out.push(ext[i] * (1.0 - f) + ext[i + 1] * f);
            pos += ratio;
        }
        rs.pos = pos - (ext.len() - 1) as f64;
        rs.last = ext[ext.len() - 1];
        self.sh.buf.lk().extend(out);
    }

    /// A short, soft two-note chime to check the speaker.
    pub fn chime(&self) {
        self.push(&chime_samples(24_000.0), 24_000.0);
    }
}

fn chime_samples(rate: f64) -> Vec<f32> {
    let note = |freq: f64, secs: f64| -> Vec<f32> {
        let n = (rate * secs) as usize;
        (0..n)
            .map(|i| {
                let t = i as f64 / rate;
                let env = (t / 0.01).min(1.0) * (1.0 - i as f64 / n as f64).powi(2);
                (0.35 * env * (std::f64::consts::TAU * freq * t).sin()) as f32
            })
            .collect()
    };
    [note(660.0, 0.18), note(880.0, 0.32)].concat()
}

/// The microphones and speakers Windows offers right now, plus which ones are the defaults.
pub fn devices() -> Value {
    fn names<I: Iterator<Item = cpal::Device>>(it: Option<I>) -> Vec<String> {
        let mut v: Vec<String> = it.into_iter().flatten().filter_map(|d| d.name().ok()).collect();
        v.sort();
        v.dedup();
        v
    }
    let host = cpal::default_host();
    json!({
        "mics": names(host.input_devices().ok()),
        "speakers": names(host.output_devices().ok()),
        "default_mic": host.default_input_device().and_then(|d| d.name().ok()),
        "default_speaker": host.default_output_device().and_then(|d| d.name().ok()),
    })
}

/// Start the audio thread. It never fails: missing devices are reported through `take_notes` and retried.
pub fn start(mic_tx: mpsc::Sender<Vec<u8>>, mic_on: Arc<AtomicBool>, choice: Choice, volume: u32) -> Audio {
    let sh = Arc::new(Shared {
        buf: Mutex::new(VecDeque::new()),
        out_rate: AtomicU32::new(0),
        level_in: AtomicU32::new(0),
        level_out: AtomicU32::new(0),
        speaking_until: AtomicU64::new(0),
        volume: AtomicU32::new(volume.min(150)),
        notes: Mutex::new(vec![]),
        current: Mutex::new((String::new(), String::new())),
        mic_tx,
        mic_on,
    });
    let (cmd, rx) = std_mpsc::channel();
    let s2 = sh.clone();
    std::thread::Builder::new()
        .name("audio".into())
        .spawn(move || run(s2, choice, rx))
        .expect("audio thread");
    Audio { sh, rs: Mutex::new(Resampler::default()), cmd }
}

/// One direction (microphone or speaker) and what the thread knows about it.
struct Side {
    input: bool,
    stream: Option<Stream>,
    name: String,
    /// Using the default because the chosen device isn't there.
    fallback: bool,
    /// Set by the stream's error callback.
    failed: Arc<AtomicBool>,
    /// The last problem reported, so it is said once rather than every check.
    problem: Option<String>,
}

impl Side {
    fn what(&self) -> &'static str {
        if self.input { "microphone" } else { "speaker" }
    }

    fn report(&mut self, sh: &Shared, msg: String) {
        if self.problem.as_deref() != Some(msg.as_str()) {
            crate::log::warn(&format!("audio: {msg}"));
            sh.notes.lk().push(msg.clone());
        }
        self.problem = Some(msg);
    }
}

fn run(sh: Arc<Shared>, mut choice: Choice, rx: std_mpsc::Receiver<Cmd>) {
    let host = cpal::default_host();
    let mut mic = Side { input: true, stream: None, name: String::new(), fallback: false, failed: Arc::default(), problem: None };
    let mut spk = Side { input: false, stream: None, name: String::new(), fallback: false, failed: Arc::default(), problem: None };
    loop {
        refresh(&host, &choice.speaker, &mut spk, &sh);
        refresh(&host, &choice.mic, &mut mic, &sh);
        *sh.current.lk() = (
            if mic.stream.is_some() { mic.name.clone() } else { String::new() },
            if spk.stream.is_some() { spk.name.clone() } else { String::new() },
        );
        match rx.recv_timeout(CHECK_EVERY) {
            Ok(Cmd::Use(c)) => {
                if c.mic != choice.mic {
                    mic.stream = None;
                    mic.problem = None;
                }
                if c.speaker != choice.speaker {
                    spk.stream = None;
                    spk.problem = None;
                }
                choice = c;
            }
            Err(std_mpsc::RecvTimeoutError::Timeout) => {}
            Err(std_mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// The device to use for `wanted` (a name, or "" for the default), and whether that is a fallback to the default.
fn pick(host: &cpal::Host, input: bool, wanted: &str) -> (Option<cpal::Device>, bool) {
    if !wanted.is_empty() {
        let found = if input {
            host.input_devices().ok().and_then(|mut it| it.find(|d| d.name().is_ok_and(|n| n == wanted)))
        } else {
            host.output_devices().ok().and_then(|mut it| it.find(|d| d.name().is_ok_and(|n| n == wanted)))
        };
        if found.is_some() {
            return (found, false);
        }
    }
    (if input { host.default_input_device() } else { host.default_output_device() }, !wanted.is_empty())
}

/// Make sure `side` runs on the right device, (re)building its stream when needed.
fn refresh(host: &cpal::Host, wanted: &str, side: &mut Side, sh: &Arc<Shared>) {
    let what = side.what();
    let broken = side.failed.swap(false, Ordering::SeqCst);
    if side.stream.is_some() && !broken {
        // Healthy. Only a stream that follows the default (or stands in for a missing choice) may need to move.
        if !(side.fallback || wanted.is_empty()) {
            return;
        }
        let (dev, fb) = pick(host, side.input, wanted);
        let name = dev.and_then(|d| d.name().ok()).unwrap_or_default();
        if name == side.name && fb == side.fallback {
            return;
        }
    }
    if broken && side.stream.is_some() {
        side.report(sh, format!("The {what} '{}' stopped working; trying to get it back...", side.name));
    }
    side.stream = None;
    if !side.input {
        sh.out_rate.store(0, Ordering::Relaxed);
    }

    let (dev, fb) = pick(host, side.input, wanted);
    let Some(dev) = dev else {
        return side.report(sh, format!("No {what} found. Plug one in and I'll pick it up."));
    };
    let name = dev.name().unwrap_or_else(|_| what.to_string());
    let built = if side.input {
        build_input(&dev, sh, side.failed.clone())
    } else {
        build_output(&dev, sh, side.failed.clone()).map(|(s, rate)| {
            sh.out_rate.store(rate, Ordering::Relaxed);
            s
        })
    };
    match built.and_then(|s| s.play().map(|_| s).map_err(|e| e.to_string())) {
        Ok(s) => {
            let had_problem = side.problem.take().is_some();
            if fb && (!side.fallback || side.name != name) {
                side.report(sh, format!("Your chosen {what} '{wanted}' isn't connected, so I'm using {name} for now."));
            } else if had_problem {
                sh.notes.lk().push(format!("The {what} is working again ({name})."));
            }
            crate::log::info(&format!("audio: {what} = {name}"));
            side.stream = Some(s);
            side.name = name;
            side.fallback = fb;
        }
        Err(e) => {
            if !side.input {
                sh.out_rate.store(0, Ordering::Relaxed);
            }
            side.report(sh, format!("I couldn't use the {what} '{name}' ({e})."));
        }
    }
}

fn build_output(dev: &cpal::Device, sh: &Arc<Shared>, failed: Arc<AtomicBool>) -> Result<(Stream, u32), String> {
    let cfg = dev.default_output_config().map_err(|e| e.to_string())?;
    let rate = cfg.sample_rate().0;
    let ch = cfg.channels() as usize;
    let fmt = cfg.sample_format();
    let cfg: cpal::StreamConfig = cfg.into();

    fn make<T: SizedSample + FromSample<f32>>(
        dev: &cpal::Device,
        cfg: &cpal::StreamConfig,
        ch: usize,
        sh: Arc<Shared>,
        failed: Arc<AtomicBool>,
    ) -> Result<Stream, String> {
        dev.build_output_stream(
            cfg,
            move |data: &mut [T], _| {
                let gain = sh.volume.load(Ordering::Relaxed) as f32 / 100.0;
                let mut q = sh.buf.lk();
                let mut energy = 0.0f32;
                let mut real = false;
                for frame in data.chunks_mut(ch) {
                    let s = match q.pop_front() {
                        Some(s) => {
                            real = true;
                            (s * gain).clamp(-1.0, 1.0)
                        }
                        None => 0.0,
                    };
                    energy += s * s;
                    for o in frame.iter_mut() {
                        *o = T::from_sample(s);
                    }
                }
                if real {
                    sh.speaking_until.store(now_ms() + ECHO_TAIL_MS, Ordering::Relaxed);
                }
                let rms = (energy / (data.len() / ch).max(1) as f32).sqrt();
                let prev = f32::from_bits(sh.level_out.load(Ordering::Relaxed));
                sh.level_out.store((prev * 0.6 + rms * 0.4).to_bits(), Ordering::Relaxed);
            },
            move |e| {
                crate::log::warn(&format!("audio out error: {e}"));
                failed.store(true, Ordering::SeqCst);
            },
            None,
        )
        .map_err(|e| e.to_string())
    }

    let sh = sh.clone();
    let s = match fmt {
        SampleFormat::F32 => make::<f32>(dev, &cfg, ch, sh, failed)?,
        SampleFormat::I16 => make::<i16>(dev, &cfg, ch, sh, failed)?,
        SampleFormat::U16 => make::<u16>(dev, &cfg, ch, sh, failed)?,
        f => return Err(format!("unsupported sample format {f:?}")),
    };
    Ok((s, rate))
}

fn build_input(dev: &cpal::Device, sh: &Arc<Shared>, failed: Arc<AtomicBool>) -> Result<Stream, String> {
    let cfg = dev.default_input_config().map_err(|e| e.to_string())?;
    let rate = cfg.sample_rate().0;
    let ch = cfg.channels() as usize;
    let fmt = cfg.sample_format();
    let cfg: cpal::StreamConfig = cfg.into();

    fn make<T: SizedSample>(
        dev: &cpal::Device,
        cfg: &cpal::StreamConfig,
        rate: u32,
        ch: usize,
        sh: Arc<Shared>,
        failed: Arc<AtomicBool>,
    ) -> Result<Stream, String>
    where
        f32: FromSample<T>,
    {
        // Box-filter downsampler to 16 kHz, collected into ~100 ms chunks.
        let (mut phase, mut sum, mut cnt) = (0u32, 0.0f32, 0u32);
        let mut chunk: Vec<u8> = Vec::with_capacity(3200);
        dev.build_input_stream(
            cfg,
            move |data: &[T], _| {
                let on = sh.mic_on.load(Ordering::Relaxed);
                let mut energy = 0.0f32;
                for frame in data.chunks(ch) {
                    let m = frame.iter().map(|s| s.to_sample::<f32>()).sum::<f32>() / ch as f32;
                    energy += m * m;
                    sum += m;
                    cnt += 1;
                    phase += IN_RATE;
                    if phase >= rate {
                        phase -= rate;
                        let v = (sum / cnt as f32).clamp(-1.0, 1.0);
                        sum = 0.0;
                        cnt = 0;
                        chunk.extend_from_slice(&((v * 32767.0) as i16).to_le_bytes());
                    }
                }
                let rms = if on { (energy / (data.len() / ch).max(1) as f32).sqrt() } else { 0.0 };
                let prev = f32::from_bits(sh.level_in.load(Ordering::Relaxed));
                sh.level_in.store((prev * 0.6 + rms * 0.4).to_bits(), Ordering::Relaxed);

                if chunk.len() >= 3200 {
                    let gate_open = on && now_ms() >= sh.speaking_until.load(Ordering::Relaxed);
                    if gate_open {
                        let _ = sh.mic_tx.try_send(std::mem::take(&mut chunk));
                    } else {
                        chunk.clear();
                    }
                }
            },
            move |e| {
                crate::log::warn(&format!("audio in error: {e}"));
                failed.store(true, Ordering::SeqCst);
            },
            None,
        )
        .map_err(|e| e.to_string())
    }

    let sh = sh.clone();
    match fmt {
        SampleFormat::F32 => make::<f32>(dev, &cfg, rate, ch, sh, failed),
        SampleFormat::I16 => make::<i16>(dev, &cfg, rate, ch, sh, failed),
        SampleFormat::U16 => make::<u16>(dev, &cfg, rate, ch, sh, failed),
        f => Err(format!("unsupported sample format {f:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_listing_has_its_shape_even_without_devices() {
        let v = devices();
        assert!(v["mics"].is_array() && v["speakers"].is_array());
        assert!(v.get("default_mic").is_some() && v.get("default_speaker").is_some());
    }

    #[test]
    fn chime_is_short_and_quiet() {
        let s = chime_samples(24_000.0);
        assert!((10_000..14_000).contains(&s.len()));
        assert!(s.iter().all(|v| v.abs() <= 0.36));
    }
}
