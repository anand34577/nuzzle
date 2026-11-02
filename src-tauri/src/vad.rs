//! Voice activity detection: finds where an utterance starts and ends in the 100 ms chunks (16 kHz mono PCM16) the
//! microphone produces.
//!
//! Plain energy gating mistakes fans, rumble and desk thumps for speech, so this adds: a high-pass filter (drops
//! rumble and DC), a noise floor that adapts to the room, hysteresis (it takes more to start than to keep going, so
//! a quiet word at the end of a sentence isn't cut off), a zero-crossing check (hiss is much "busier" than a voice),
//! and rejection of blips that are too short to be words.

use std::collections::VecDeque;

/// Chunks of silence that end an utterance (7 x 100 ms: long enough for a breath, short enough to feel responsive).
const HANGOVER: u32 = 7;
/// Chunks kept from before the first loud one, so the start of a word isn't clipped.
const PREROLL: usize = 3;
/// Fewest loud chunks that count as speech rather than a blip.
const MIN_SPEECH: u32 = 3;
/// Loud chunks in a row before we say "hearing you" (and the character reacts).
const ANNOUNCE_AFTER: u32 = 2;
/// Longest single utterance: 25 s of 16 kHz PCM16.
const MAX_BYTES: usize = 16_000 * 2 * 25;

#[derive(Debug, PartialEq)]
pub enum Vad {
    /// Nothing worth reporting.
    Idle,
    /// Speech has clearly started.
    Started,
    /// An utterance is complete.
    Ended(Vec<u8>),
    /// Something loud, but too short or too noisy to be speech.
    Dismissed,
}

pub struct Endpointer {
    floor: f32,
    pre: VecDeque<Vec<u8>>,
    utt: Vec<u8>,
    speech: u32,
    quiet: u32,
    active: bool,
    announced: bool,
    /// Two cascaded one-pole high-pass stages (input, output) each.
    hp: [(f32, f32); 2],
}

/// Root-mean-square level of PCM16 bytes, 0..1.
#[cfg(test)]
fn rms(chunk: &[u8]) -> f32 {
    let n = chunk.len() / 2;
    if n == 0 {
        return 0.0;
    }
    let e: f32 = chunk
        .chunks_exact(2)
        .map(|b| {
            let v = i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0;
            v * v
        })
        .sum();
    (e / n as f32).sqrt()
}

impl Default for Endpointer {
    fn default() -> Self {
        Self::new()
    }
}

impl Endpointer {
    pub fn new() -> Self {
        Self { floor: 0.01, pre: VecDeque::new(), utt: vec![], speech: 0, quiet: 0, active: false, announced: false, hp: [(0.0, 0.0); 2] }
    }

    /// Is an utterance in progress?
    #[cfg(test)]
    pub fn active(&self) -> bool {
        self.active
    }

    /// The learned background level (for tests and diagnostics).
    #[cfg(test)]
    pub fn floor(&self) -> f32 {
        self.floor
    }

    /// RMS after a two-stage high-pass (~100 Hz, so rumble and thumps fade while voices pass), and the fraction of
    /// sign changes between samples.
    fn analyze(&mut self, chunk: &[u8]) -> (f32, f32) {
        let (mut e, mut n, mut cross) = (0.0f32, 0u32, 0u32);
        let mut prev = 0.0f32;
        for b in chunk.chunks_exact(2) {
            let mut v = i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0;
            for (px, py) in &mut self.hp {
                let y = v - *px + 0.96 * *py;
                (*px, *py) = (v, y);
                v = y;
            }
            e += v * v;
            if n > 0 && (v >= 0.0) != (prev >= 0.0) {
                cross += 1;
            }
            prev = v;
            n += 1;
        }
        if n == 0 {
            return (0.0, 0.0);
        }
        ((e / n as f32).sqrt(), cross as f32 / n as f32)
    }

    pub fn push(&mut self, chunk: Vec<u8>) -> Vad {
        let (level, zcr) = self.analyze(&chunk);
        let start_at = (self.floor * 3.0).max(0.015);
        let keep_at = (self.floor * 1.8).max(0.010);
        // Voice sits mostly below ~4 kHz; white-ish hiss crosses zero far more often than speech does.
        let speechlike = zcr < 0.35;
        let loud = if self.active { level > keep_at } else { level > start_at && speechlike };

        if !self.active {
            if loud {
                self.active = true;
                self.announced = false;
                self.utt = self.pre.drain(..).flatten().collect();
                self.utt.extend_from_slice(&chunk);
                self.speech = 1;
                self.quiet = 0;
            } else {
                // Only quiet moments teach us what "quiet" is here.
                self.floor = (self.floor * 0.95 + level * 0.05).clamp(0.002, 0.08);
                self.pre.push_back(chunk);
                if self.pre.len() > PREROLL {
                    self.pre.pop_front();
                }
            }
            return Vad::Idle;
        }

        self.utt.extend_from_slice(&chunk);
        if loud {
            self.speech += 1;
            self.quiet = 0;
        } else {
            self.quiet += 1;
        }
        if self.quiet >= HANGOVER || self.utt.len() > MAX_BYTES {
            return self.finish(MIN_SPEECH);
        }
        if !self.announced && self.speech >= ANNOUNCE_AFTER {
            self.announced = true;
            return Vad::Started;
        }
        Vad::Idle
    }

    /// Push-to-talk released: whatever was said is complete (even a short "yes").
    pub fn flush(&mut self) -> Vad {
        if self.active {
            self.finish(1)
        } else {
            Vad::Idle
        }
    }

    fn finish(&mut self, min_speech: u32) -> Vad {
        self.active = false;
        let (speech, announced) = (self.speech, self.announced);
        let pcm = std::mem::take(&mut self.utt);
        self.speech = 0;
        self.quiet = 0;
        self.announced = false;
        self.pre.clear();
        if speech >= min_speech {
            Vad::Ended(pcm)
        } else if announced || speech > 0 {
            Vad::Dismissed
        } else {
            Vad::Idle
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 100 ms of a sine at `freq` Hz with peak `amp` (0..1), as PCM16 bytes.
    fn tone(freq: f32, amp: f32) -> Vec<u8> {
        (0..1600)
            .flat_map(|i| (((i as f32 / 16_000.0 * freq * std::f32::consts::TAU).sin() * amp * 32767.0) as i16).to_le_bytes())
            .collect()
    }

    /// Deterministic hiss (a simple LCG), full-band, so it crosses zero constantly.
    fn hiss(amp: f32, seed: &mut u32) -> Vec<u8> {
        (0..1600)
            .flat_map(|_| {
                *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let v = ((*seed >> 16) as f32 / 65535.0 - 0.5) * 2.0 * amp;
                ((v * 32767.0) as i16).to_le_bytes()
            })
            .collect()
    }

    fn quiet() -> Vec<u8> {
        tone(120.0, 0.004)
    }

    #[test]
    fn silence_stays_idle_and_teaches_the_floor() {
        let mut v = Endpointer::new();
        for _ in 0..40 {
            assert_eq!(v.push(quiet()), Vad::Idle);
        }
        assert!(v.floor() < 0.01 && !v.active());
    }

    #[test]
    fn a_spoken_phrase_is_announced_then_returned_with_a_little_lead_in() {
        let mut v = Endpointer::new();
        for _ in 0..5 {
            v.push(quiet());
        }
        let mut events = vec![];
        for _ in 0..8 {
            events.push(v.push(tone(220.0, 0.3)));
        }
        assert_eq!(events[0], Vad::Idle, "one loud chunk alone isn't announced");
        assert_eq!(events[1], Vad::Started);
        let mut ended = None;
        for _ in 0..HANGOVER + 1 {
            if let Vad::Ended(p) = v.push(quiet()) {
                ended = Some(p);
            }
        }
        let pcm = ended.expect("utterance ends after the hangover");
        // 3 lead-in chunks + 8 spoken + the quiet tail
        assert!(pcm.len() >= (PREROLL + 8) * 3200, "got {} bytes", pcm.len());
        assert!(!v.active());
    }

    #[test]
    fn a_short_blip_is_dismissed_not_transcribed() {
        let mut v = Endpointer::new();
        for _ in 0..5 {
            v.push(quiet());
        }
        v.push(tone(200.0, 0.4)); // a single 100 ms thump
        let mut last = Vad::Idle;
        for _ in 0..HANGOVER + 1 {
            let e = v.push(quiet());
            if e != Vad::Idle {
                last = e;
            }
        }
        assert_eq!(last, Vad::Dismissed);
    }

    #[test]
    fn steady_hiss_is_not_speech_even_when_loud() {
        let mut v = Endpointer::new();
        let mut seed = 7;
        for _ in 0..30 {
            assert_eq!(v.push(hiss(0.5, &mut seed)), Vad::Idle);
        }
        assert!(!v.active());
    }

    #[test]
    fn low_rumble_is_filtered_out() {
        let mut v = Endpointer::new();
        for _ in 0..30 {
            assert_eq!(v.push(tone(25.0, 0.2)), Vad::Idle, "25 Hz rumble (a passing truck) must not trigger");
        }
    }

    #[test]
    fn a_quiet_word_at_the_end_does_not_cut_the_sentence() {
        let mut v = Endpointer::new();
        for _ in 0..5 {
            v.push(quiet());
        }
        for _ in 0..6 {
            v.push(tone(200.0, 0.3));
        }
        // trailing words at ~1/5 of the volume are above the keep-going level
        for _ in 0..4 {
            assert!(!matches!(v.push(tone(200.0, 0.06)), Vad::Ended(_)));
        }
        assert!(v.active());
    }

    #[test]
    fn push_to_talk_release_ends_even_a_short_word() {
        let mut v = Endpointer::new();
        for _ in 0..3 {
            v.push(quiet());
        }
        v.push(tone(200.0, 0.4));
        assert!(matches!(v.flush(), Vad::Ended(_)));
        assert_eq!(v.flush(), Vad::Idle);
    }

    #[test]
    fn very_long_speech_is_cut_at_the_limit() {
        let mut v = Endpointer::new();
        let mut ended = false;
        for _ in 0..300 {
            if matches!(v.push(tone(200.0, 0.3)), Vad::Ended(_)) {
                ended = true;
                break;
            }
        }
        assert!(ended);
    }

    #[test]
    fn rms_of_silence_and_noise() {
        assert_eq!(rms(&[0u8; 64]), 0.0);
        let loud: Vec<u8> = (0..32).flat_map(|_| 16_000i16.to_le_bytes()).collect();
        assert!(rms(&loud) > 0.4);
    }
}
