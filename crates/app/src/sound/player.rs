//! Non-blocking playback of synthesised cues through the default audio device.
//!
//! The device is opened lazily (only once sound is enabled) on a dedicated
//! background thread, so neither construction nor `play` ever blocks the GUI
//! thread for long. If no device can be opened the player stays silent.

use std::collections::HashMap;
use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use rodio::buffer::SamplesBuffer;
use rodio::mixer::Mixer;
use rodio::source::Done;

use super::synth::{Cue, SAMPLE_RATE, render_cue};

/// Minimum spacing between two key-press ticks.
const KEYPRESS_INTERVAL: Duration = Duration::from_millis(30);
/// Minimum spacing between two plays of the same non-key-press cue.
const CUE_INTERVAL: Duration = Duration::from_millis(80);
/// Maximum number of cues sounding at once; extras are dropped.
const MAX_OVERLAP: usize = 4;
/// How long `SoundPlayer::new` waits for the device before continuing silently
/// (the device may still become available later).
const OPEN_WAIT: Duration = Duration::from_millis(250);
/// Default volume for new settings.
const DEFAULT_VOLUME: f32 = 0.4;

/// User-facing sound preferences.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SoundSettings {
    /// Master switch; sound is off by default.
    pub enabled: bool,
    /// Master volume, clamped to `0..=1` when applied.
    pub volume: f32,
    /// Whether typing produces key-press ticks (separately toggleable).
    pub keypress: bool,
}

impl Default for SoundSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            volume: DEFAULT_VOLUME,
            keypress: false,
        }
    }
}

impl SoundSettings {
    /// A copy with `volume` clamped to `0..=1` (non-finite becomes 0).
    pub fn sanitized(self) -> Self {
        let volume = if self.volume.is_finite() {
            self.volume.clamp(0.0, 1.0)
        } else {
            0.0
        };
        Self { volume, ..self }
    }
}

/// Pure rate-limit and overlap policy for cue playback.
#[derive(Debug, Default)]
pub(crate) struct Limiter {
    last_played: HashMap<Cue, Instant>,
}

impl Limiter {
    /// Decide whether `cue` may start at `now` while `active` cues are sounding.
    /// Records the play when allowed.
    pub(crate) fn allow(&mut self, cue: Cue, now: Instant, active: usize) -> bool {
        if active >= MAX_OVERLAP {
            return false;
        }
        let interval = match cue {
            Cue::KeyPress => KEYPRESS_INTERVAL,
            _ => CUE_INTERVAL,
        };
        if let Some(prev) = self.last_played.get(&cue)
            && now.saturating_duration_since(*prev) < interval
        {
            return false;
        }
        self.last_played.insert(cue, now);
        true
    }
}

/// State of the audio output device.
enum Output {
    /// Not opened (sound disabled).
    Idle,
    /// Background thread is opening the device.
    Opening {
        ready: Receiver<Option<Mixer>>,
        keep_alive: Sender<()>,
    },
    /// Device open; dropping `keep_alive` closes it.
    Ready {
        mixer: Mixer,
        _keep_alive: Sender<()>,
    },
    /// No usable device; stay silent.
    Unavailable,
}

impl Output {
    /// Start opening the default device on a background thread which owns the
    /// stream and keeps it alive until the returned handle's sender is dropped.
    fn open() -> Self {
        let (ready_tx, ready) = mpsc::channel();
        let (keep_alive, stop_rx) = mpsc::channel::<()>();
        let spawned = thread::Builder::new()
            .name("sound-output".into())
            .spawn(move || run_output_thread(&ready_tx, &stop_rx));
        match spawned {
            Ok(_) => Output::Opening { ready, keep_alive },
            Err(_) => Output::Unavailable,
        }
    }

    /// Advance `Opening` to `Ready`/`Unavailable` if the thread has reported.
    fn poll(self, wait: Option<Duration>) -> Self {
        let Output::Opening { ready, keep_alive } = self else {
            return self;
        };
        let result = match wait {
            Some(timeout) => ready.recv_timeout(timeout).map_err(|e| match e {
                mpsc::RecvTimeoutError::Timeout => TryRecvError::Empty,
                mpsc::RecvTimeoutError::Disconnected => TryRecvError::Disconnected,
            }),
            None => ready.try_recv(),
        };
        match result {
            Ok(Some(mixer)) => Output::Ready {
                mixer,
                _keep_alive: keep_alive,
            },
            Ok(None) | Err(TryRecvError::Disconnected) => Output::Unavailable,
            Err(TryRecvError::Empty) => Output::Opening { ready, keep_alive },
        }
    }
}

/// Body of the output thread: open the device, hand back its mixer, then hold
/// the stream open until the player drops its keep-alive sender.
fn run_output_thread(ready_tx: &Sender<Option<Mixer>>, stop_rx: &Receiver<()>) {
    match rodio::DeviceSinkBuilder::open_default_sink() {
        Ok(mut sink) => {
            sink.log_on_drop(false);
            if ready_tx.send(Some(sink.mixer().clone())).is_ok() {
                // Blocks until the keep-alive sender is dropped.
                let _ = stop_rx.recv();
            }
        }
        Err(_) => {
            let _ = ready_tx.send(None);
        }
    }
}

/// Plays procedurally synthesised cues; silent when disabled or no device.
pub struct SoundPlayer {
    settings: SoundSettings,
    cache: HashMap<Cue, Arc<[f32]>>,
    cached_volume: Option<f32>,
    limiter: Limiter,
    output: Output,
    active: Arc<AtomicUsize>,
}

impl std::fmt::Debug for SoundPlayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SoundPlayer")
            .field("settings", &self.settings)
            .field("available", &self.available())
            .finish_non_exhaustive()
    }
}

impl SoundPlayer {
    /// Create a player. Never fails: if no audio device can be opened the
    /// player is silent and [`available`](Self::available) is false. The device
    /// is only opened when `settings.enabled` is true.
    pub fn new(settings: SoundSettings) -> Self {
        let mut player = Self {
            settings: SoundSettings::default(),
            cache: HashMap::new(),
            cached_volume: None,
            limiter: Limiter::default(),
            output: Output::Idle,
            active: Arc::new(AtomicUsize::new(0)),
        };
        player.set_settings(settings);
        player.poll_output(Some(OPEN_WAIT));
        player
    }

    /// Apply new settings: re-renders cues on volume change, opens the device
    /// when enabled and releases it when disabled. Never blocks.
    pub fn set_settings(&mut self, settings: SoundSettings) {
        self.settings = settings.sanitized();
        if !self.settings.enabled {
            self.output = Output::Idle;
            return;
        }
        if matches!(self.output, Output::Idle) {
            self.output = Output::open();
        }
        if self.cached_volume != Some(self.settings.volume) {
            self.render_cache();
        }
    }

    /// Whether an audio device is open and cues can actually be heard.
    pub fn available(&self) -> bool {
        matches!(self.output, Output::Ready { .. })
    }

    /// Play `cue` without blocking. No-op when disabled, when the device is
    /// unavailable, for key presses unless `keypress` is enabled, and when rate
    /// limits (key press ≤ 1 per 30 ms, other cues ≤ 1 per 80 ms each, ≤ 4
    /// overlapping sounds) would be exceeded.
    pub fn play(&mut self, cue: Cue) {
        if !self.settings.enabled || (cue == Cue::KeyPress && !self.settings.keypress) {
            return;
        }
        self.poll_output(None);
        let Output::Ready { mixer, .. } = &self.output else {
            return;
        };
        let Some(samples) = self.cache.get(&cue) else {
            return;
        };
        let active = self.active.load(Ordering::Relaxed);
        if !self.limiter.allow(cue, Instant::now(), active) {
            return;
        }
        let Some(rate) = NonZero::new(SAMPLE_RATE) else {
            return;
        };
        let buffer = SamplesBuffer::new(NonZero::<u16>::MIN, rate, samples.to_vec());
        self.active.fetch_add(1, Ordering::Relaxed);
        mixer.add(Done::new(buffer, Arc::clone(&self.active)));
    }

    fn poll_output(&mut self, wait: Option<Duration>) {
        let output = std::mem::replace(&mut self.output, Output::Idle);
        self.output = output.poll(wait);
    }

    fn render_cache(&mut self) {
        let volume = self.settings.volume;
        self.cache = Cue::ALL
            .iter()
            .map(|&cue| (cue, Arc::from(render_cue(cue, volume))))
            .collect();
        self.cached_volume = Some(volume);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn default_settings_are_off() {
        let s = SoundSettings::default();
        assert!(!s.enabled);
        assert!(!s.keypress);
        assert!((s.volume - 0.4).abs() < f32::EPSILON);
    }

    #[test]
    fn sanitized_clamps_volume() {
        let clamp = |v| {
            SoundSettings {
                volume: v,
                ..SoundSettings::default()
            }
            .sanitized()
            .volume
        };
        assert_eq!(clamp(2.0), 1.0);
        assert_eq!(clamp(-0.5), 0.0);
        assert_eq!(clamp(f32::NAN), 0.0);
        assert_eq!(clamp(0.25), 0.25);
    }

    #[test]
    fn keypress_limited_to_one_per_30ms() {
        let mut l = Limiter::default();
        let t0 = Instant::now();
        assert!(l.allow(Cue::KeyPress, t0, 0));
        assert!(!l.allow(Cue::KeyPress, t0 + ms(10), 0));
        assert!(!l.allow(Cue::KeyPress, t0 + ms(29), 0));
        assert!(l.allow(Cue::KeyPress, t0 + ms(30), 0));
    }

    #[test]
    fn other_cues_limited_per_cue_to_80ms() {
        let mut l = Limiter::default();
        let t0 = Instant::now();
        assert!(l.allow(Cue::Bell, t0, 0));
        assert!(!l.allow(Cue::Bell, t0 + ms(50), 0));
        assert!(l.allow(Cue::PanelOpen, t0 + ms(50), 1));
        assert!(l.allow(Cue::KeyPress, t0 + ms(50), 1));
        assert!(!l.allow(Cue::Bell, t0 + ms(79), 0));
        assert!(l.allow(Cue::Bell, t0 + ms(80), 0));
    }

    #[test]
    fn overlap_capped_at_four() {
        let mut l = Limiter::default();
        let t0 = Instant::now();
        assert!(l.allow(Cue::Bell, t0, 3));
        assert!(!l.allow(Cue::Error, t0, 4));
        assert!(!l.allow(Cue::KeyPress, t0, 5));
        // A dropped play does not start the cue's cooldown.
        assert!(l.allow(Cue::Error, t0 + ms(1), 0));
    }

    #[test]
    fn earlier_instant_does_not_panic() {
        let mut l = Limiter::default();
        let t0 = Instant::now() + ms(100);
        assert!(l.allow(Cue::Bell, t0, 0));
        assert!(!l.allow(Cue::Bell, t0 - ms(50), 0));
    }

    #[test]
    fn disabled_player_is_a_silent_noop() {
        let mut p = SoundPlayer::new(SoundSettings::default());
        assert!(!p.available());
        for cue in Cue::ALL {
            p.play(cue);
        }
    }

    #[test]
    fn enabled_player_never_panics_even_without_device() {
        let settings = SoundSettings {
            enabled: true,
            volume: 0.0,
            keypress: true,
        };
        let mut p = SoundPlayer::new(settings);
        for cue in Cue::ALL {
            p.play(cue);
        }
        p.set_settings(SoundSettings {
            volume: 0.1,
            ..settings
        });
        p.play(Cue::KeyPress);
        p.set_settings(SoundSettings::default());
        assert!(!p.available());
    }
}
