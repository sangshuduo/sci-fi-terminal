//! Procedural synthesis of the terminal's sound cues.
//!
//! Clean-room: every cue is generated here from original parameters (short
//! sine / soft-square / filtered-noise voices shaped by envelopes). No recorded
//! or third-party audio is used. Everything in this module is pure and
//! deterministic, so it is fully unit-testable without an audio device.

use std::f32::consts::TAU;

/// Output sample rate of every rendered cue, in Hz (mono).
pub const SAMPLE_RATE: u32 = 44_100;

/// Seed for the deterministic noise generator.
const NOISE_SEED: u32 = 0x5EED_C0DE;

/// A short UI sound effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cue {
    /// Very short, soft filtered-noise tick for typing.
    KeyPress,
    /// Two-tone chime for the terminal bell.
    Bell,
    /// Short rising sweep when a panel opens.
    PanelOpen,
    /// Short falling sweep when a panel closes.
    PanelClose,
    /// Soft ascending three-note figure when a session starts.
    SessionStart,
    /// Soft descending three-note figure when a session exits.
    SessionExit,
    /// Low double buzz for errors.
    Error,
}

impl Cue {
    /// Every cue, in declaration order.
    pub const ALL: [Cue; 7] = [
        Cue::KeyPress,
        Cue::Bell,
        Cue::PanelOpen,
        Cue::PanelClose,
        Cue::SessionStart,
        Cue::SessionExit,
        Cue::Error,
    ];
}

/// Oscillator shape of a voice.
#[derive(Debug, Clone, Copy)]
enum Wave {
    /// Pure sine.
    Sine,
    /// Sine with a quiet octave overtone (chime-like).
    Chime,
    /// Soft-clipped sine: a rounded square wave with `drive` controlling edge hardness.
    SoftSquare { drive: f32 },
    /// White noise band-passed between `f0` (high-pass) and `f1` (low-pass).
    Noise,
}

/// One enveloped sound event inside a cue.
#[derive(Debug, Clone, Copy)]
struct Voice {
    start_ms: f32,
    dur_ms: f32,
    /// Start frequency (Hz); for `Wave::Noise` the high-pass corner.
    f0: f32,
    /// End frequency (Hz, exponential glide); for `Wave::Noise` the low-pass corner.
    f1: f32,
    wave: Wave,
    attack_ms: f32,
    release_ms: f32,
    /// Exponential decay rate per second applied after the attack (0 = flat).
    decay: f32,
    gain: f32,
}

impl Voice {
    /// A steady tone voice with typical soft envelope.
    const fn tone(start_ms: f32, dur_ms: f32, freq: f32, wave: Wave, gain: f32) -> Self {
        Self {
            start_ms,
            dur_ms,
            f0: freq,
            f1: freq,
            wave,
            attack_ms: 6.0,
            release_ms: 30.0,
            decay: 8.0,
            gain,
        }
    }

    /// A frequency sweep voice.
    const fn sweep(dur_ms: f32, from: f32, to: f32) -> Self {
        Self {
            start_ms: 0.0,
            dur_ms,
            f0: from,
            f1: to,
            wave: Wave::Sine,
            attack_ms: 12.0,
            release_ms: 40.0,
            decay: 0.0,
            gain: 1.0,
        }
    }
}

/// Recipe for a cue: total length, target peak (before volume) and voices.
struct Recipe {
    total_ms: f32,
    peak: f32,
    voices: &'static [Voice],
}

const KEYPRESS: Recipe = Recipe {
    total_ms: 20.0,
    peak: 0.22,
    voices: &[Voice {
        start_ms: 0.0,
        dur_ms: 20.0,
        f0: 1_800.0,
        f1: 5_200.0,
        wave: Wave::Noise,
        attack_ms: 1.0,
        release_ms: 8.0,
        decay: 160.0,
        gain: 1.0,
    }],
};

const BELL: Recipe = Recipe {
    total_ms: 250.0,
    peak: 0.45,
    voices: &[
        Voice::tone(0.0, 150.0, 1_046.0, Wave::Chime, 1.0),
        Voice::tone(70.0, 180.0, 1_568.0, Wave::Chime, 0.8),
    ],
};

const PANEL_OPEN: Recipe = Recipe {
    total_ms: 120.0,
    peak: 0.3,
    voices: &[Voice::sweep(120.0, 380.0, 960.0)],
};

const PANEL_CLOSE: Recipe = Recipe {
    total_ms: 120.0,
    peak: 0.3,
    voices: &[Voice::sweep(120.0, 960.0, 380.0)],
};

const SESSION_START: Recipe = Recipe {
    total_ms: 300.0,
    peak: 0.4,
    voices: &[
        Voice::tone(0.0, 130.0, 494.0, Wave::Sine, 1.0),
        Voice::tone(85.0, 130.0, 622.0, Wave::Sine, 0.9),
        Voice::tone(170.0, 130.0, 740.0, Wave::Sine, 0.85),
    ],
};

const SESSION_EXIT: Recipe = Recipe {
    total_ms: 300.0,
    peak: 0.4,
    voices: &[
        Voice::tone(0.0, 130.0, 698.0, Wave::Sine, 0.9),
        Voice::tone(85.0, 130.0, 554.0, Wave::Sine, 0.9),
        Voice::tone(170.0, 130.0, 415.0, Wave::Sine, 1.0),
    ],
};

const ERROR: Recipe = Recipe {
    total_ms: 200.0,
    peak: 0.45,
    voices: &[
        Voice::tone(0.0, 80.0, 138.0, Wave::SoftSquare { drive: 3.0 }, 1.0),
        Voice::tone(115.0, 85.0, 131.0, Wave::SoftSquare { drive: 3.0 }, 1.0),
    ],
};

fn recipe(cue: Cue) -> &'static Recipe {
    match cue {
        Cue::KeyPress => &KEYPRESS,
        Cue::Bell => &BELL,
        Cue::PanelOpen => &PANEL_OPEN,
        Cue::PanelClose => &PANEL_CLOSE,
        Cue::SessionStart => &SESSION_START,
        Cue::SessionExit => &SESSION_EXIT,
        Cue::Error => &ERROR,
    }
}

/// Render a cue to mono `f32` samples in `[-1, 1]` at [`SAMPLE_RATE`].
///
/// `volume` is clamped to `0..=1` (non-finite values count as 0). Output is
/// deterministic, at most 400 ms long, peaks at no more than `0.6 * volume`
/// and starts and ends at silence so playback never clicks.
pub fn render_cue(cue: Cue, volume: f32) -> Vec<f32> {
    let volume = if volume.is_finite() {
        volume.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let recipe = recipe(cue);
    let mut buf = vec![0.0; ms_to_samples(recipe.total_ms)];
    let mut rng = XorShift32::new(NOISE_SEED);
    for voice in recipe.voices {
        render_voice(&mut buf, voice, &mut rng);
    }
    normalize(&mut buf, recipe.peak.min(0.6) * volume);
    buf
}

fn ms_to_samples(ms: f32) -> usize {
    (ms * SAMPLE_RATE as f32 / 1000.0).round().max(0.0) as usize
}

/// Scale so the absolute peak equals `target` (silence stays silent).
fn normalize(buf: &mut [f32], target: f32) {
    let peak = buf.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
    let scale = if peak > f32::EPSILON {
        target / peak
    } else {
        0.0
    };
    buf.iter_mut().for_each(|s| *s *= scale);
}

/// Mix one voice into `buf`, clipped to the buffer bounds.
fn render_voice(buf: &mut [f32], voice: &Voice, rng: &mut XorShift32) {
    let start = ms_to_samples(voice.start_ms);
    let len = ms_to_samples(voice.dur_ms).min(buf.len().saturating_sub(start));
    if len < 2 {
        return;
    }
    let env = Envelope::new(voice, len);
    let mut osc = Oscillator::new(voice);
    for (i, out) in buf[start..start + len].iter_mut().enumerate() {
        let progress = i as f32 / (len - 1) as f32;
        *out += voice.gain * env.at(i) * osc.next(progress, rng);
    }
}

/// Attack/release envelope that is exactly zero at both ends, with an optional
/// exponential decay after the attack.
struct Envelope {
    len: usize,
    attack: f32,
    release: f32,
    decay_per_sample: f32,
}

impl Envelope {
    fn new(voice: &Voice, len: usize) -> Self {
        Self {
            len,
            attack: (ms_to_samples(voice.attack_ms) as f32).max(1.0),
            release: (ms_to_samples(voice.release_ms) as f32).max(1.0),
            decay_per_sample: voice.decay / SAMPLE_RATE as f32,
        }
    }

    fn at(&self, i: usize) -> f32 {
        let from_start = i as f32;
        let to_end = (self.len - 1 - i) as f32;
        let rise = smoothstep(from_start / self.attack);
        let fall = smoothstep(to_end / self.release);
        let body = (-(from_start - self.attack).max(0.0) * self.decay_per_sample).exp();
        rise * fall * body
    }
}

/// Hermite smoothstep on `x` clamped to `0..=1` (click-free ramps).
fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Phase-continuous oscillator (or filtered-noise source) for one voice.
struct Oscillator {
    wave: Wave,
    f0: f32,
    f1: f32,
    phase: f32,
    low: f32,
    high_lp: f32,
}

impl Oscillator {
    fn new(voice: &Voice) -> Self {
        Self {
            wave: voice.wave,
            f0: voice.f0.max(1.0),
            f1: voice.f1.max(1.0),
            phase: 0.0,
            low: 0.0,
            high_lp: 0.0,
        }
    }

    /// Next sample; `progress` runs 0..=1 across the voice (drives glides).
    fn next(&mut self, progress: f32, rng: &mut XorShift32) -> f32 {
        if let Wave::Noise = self.wave {
            return self.next_noise(rng);
        }
        let freq = self.f0 * (self.f1 / self.f0).powf(progress);
        let s = (TAU * self.phase).sin();
        self.phase = (self.phase + freq / SAMPLE_RATE as f32).fract();
        match self.wave {
            Wave::Sine | Wave::Noise => s,
            Wave::Chime => s + 0.25 * (2.0 * TAU * self.phase).sin(),
            Wave::SoftSquare { drive } => (drive * s).tanh(),
        }
    }

    /// White noise band-passed by two one-pole low-pass filters.
    fn next_noise(&mut self, rng: &mut XorShift32) -> f32 {
        let white = rng.next_f32();
        self.low += one_pole(self.f1) * (white - self.low);
        self.high_lp += one_pole(self.f0) * (self.low - self.high_lp);
        self.low - self.high_lp
    }
}

/// One-pole low-pass coefficient for a cutoff frequency.
fn one_pole(cutoff: f32) -> f32 {
    1.0 - (-TAU * cutoff / SAMPLE_RATE as f32).exp()
}

/// Tiny deterministic xorshift PRNG for seeded noise.
struct XorShift32(u32);

impl XorShift32 {
    fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }

    /// Uniform sample in `[-1, 1)`.
    fn next_f32(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / (1u32 << 23) as f32 - 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-5;

    fn duration_ms(samples: &[f32]) -> f32 {
        samples.len() as f32 * 1000.0 / SAMPLE_RATE as f32
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0_f32, |m, s| m.max(s.abs()))
    }

    #[test]
    fn every_cue_is_well_formed() {
        for volume in [0.1, 0.4, 1.0] {
            for cue in Cue::ALL {
                let s = render_cue(cue, volume);
                assert!(!s.is_empty(), "{cue:?} empty");
                assert!(duration_ms(&s) <= 400.0, "{cue:?} too long");
                assert!(s.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
                assert!(peak(&s) <= 0.6 * volume + EPS, "{cue:?} too loud");
                assert!(peak(&s) > 0.0, "{cue:?} silent at {volume}");
                assert!(s[0].abs() < 0.01, "{cue:?} clicks at start");
                assert!(s[s.len() - 1].abs() < 0.01, "{cue:?} clicks at end");
            }
        }
    }

    #[test]
    fn cue_lengths_match_design() {
        let ms = |c| duration_ms(&render_cue(c, 1.0));
        assert!((15.0..=25.0).contains(&ms(Cue::KeyPress)));
        assert!((200.0..=300.0).contains(&ms(Cue::Bell)));
        assert!((100.0..=150.0).contains(&ms(Cue::PanelOpen)));
        assert!((100.0..=150.0).contains(&ms(Cue::PanelClose)));
        assert!((250.0..=350.0).contains(&ms(Cue::SessionStart)));
        assert!((250.0..=350.0).contains(&ms(Cue::SessionExit)));
        assert!((150.0..=250.0).contains(&ms(Cue::Error)));
    }

    #[test]
    fn zero_volume_is_silent() {
        for cue in Cue::ALL {
            assert!(render_cue(cue, 0.0).iter().all(|&x| x == 0.0));
        }
    }

    #[test]
    fn out_of_range_volume_is_clamped() {
        for cue in Cue::ALL {
            assert_eq!(render_cue(cue, 7.0), render_cue(cue, 1.0));
            assert!(render_cue(cue, -1.0).iter().all(|&x| x == 0.0));
            assert!(render_cue(cue, f32::NAN).iter().all(|&x| x == 0.0));
        }
    }

    #[test]
    fn rendering_is_deterministic() {
        for cue in Cue::ALL {
            assert_eq!(render_cue(cue, 0.7), render_cue(cue, 0.7));
        }
    }

    #[test]
    fn cues_are_distinct() {
        let renders: Vec<_> = Cue::ALL.iter().map(|&c| render_cue(c, 1.0)).collect();
        for (i, a) in renders.iter().enumerate() {
            for b in &renders[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn xorshift_stays_in_range() {
        let mut rng = XorShift32::new(0);
        for _ in 0..10_000 {
            let v = rng.next_f32();
            assert!((-1.0..1.0).contains(&v));
        }
    }
}
