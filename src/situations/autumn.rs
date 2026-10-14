//! 秋の夜長: autumn-field insects and a quiet, continuously undulating wind.
//!
//! Each insect is a fixed-pool singer with a species, a pitch, a stereo position and three levels of
//! modulation. The FASTEST level is the wing-stroke pulse of its species (tens of hertz, a few
//! per cent faster at the top of the swell). The SHORT period is the repeat of its phrase (a bell ring, a trill, three notes, a
//! rattle, a tap). The LONG period is a "bout": the insect starts singing, sings for 4 to 12
//! seconds and stops, its loudness and calling rate following [`bout_envelope`]. The same pure
//! function drives the firefly the picture draws for that bout (one firefly per bout, at the
//! insect's own stereo position, living exactly as long as the bout), so sound and light agree.
//! Everything is a function of the world clock and per-insect world PRNGs: no wall clock and no
//! allocation after preparation.

use std::{
    f32::consts::{FRAC_PI_2, PI, TAU},
    time::Duration,
};

use fundsp::{
    audionode::AudioNode,
    biquad::Resonator,
    filter::{DCBlock, Lowpole, Pinkpass},
    noise::Noise,
    prelude32::{U1, U2, U3},
};

use crate::{
    audio::transport::{LogicalPixels, ParticleVisual, VisualWriter},
    controls::Normalized,
};

use super::{
    KnobDefinition, SituationDefinition, VisualShape, VisualStyle, Voice, VoiceFrame,
    VoiceLatencyFrames, VoicePreparation, VoicePreparationError,
    synthesis::{
        KnobRamp, RestartableReverb, WorldRandom, dry_latency, prepare_node, soft_ceiling,
    },
};

// Provisional initial values: the user retunes them by listening (P36). The coefficients below
// were derived from the SPEC bands and checked by offline analysis only.
static KNOBS: [KnobDefinition; 2] = [
    KnobDefinition {
        id: "insects",
        label: "虫の量",
        initial: match Normalized::new(0.55) {
            Some(value) => value,
            None => panic!("initial out of range"),
        },
    },
    KnobDefinition {
        id: "wind",
        label: "風の量",
        initial: match Normalized::new(0.50) {
            Some(value) => value,
            None => panic!("initial out of range"),
        },
    },
];

/// The fixed number of insect singers (the `grain_capacity` of the registration).
pub(super) const MAX_INSECTS: usize = 24;
/// The most insects that may be inside a bout at once (so at most this many fireflies glow).
const MAX_SINGING: usize = 16;
/// How many insects may be inside a bout at once: none at exactly 0, then one more for every
/// sixteenth of the knob (9 at the default 0.55, all 16 at the top).
fn singing_cap(knob: f32) -> usize {
    (knob * MAX_SINGING as f32).ceil() as usize
}
/// A bout lasts between these many seconds (SPEC: 4 to 12).
pub(crate) const BOUT_MIN_SECONDS: f64 = 4.0;
pub(crate) const BOUT_MAX_SECONDS: f64 = 12.0;
/// A firefly is drawn for at least this long; a bout is never shorter, so it lives exactly as
/// long as its bout.
pub(crate) const FIREFLY_MIN_SECONDS: f64 = 4.0;
/// Where a firefly is lit, as a share of the window's height from the top: anywhere across it.
const FIREFLY_Y: (f32, f32) = (0.10, 0.90);
/// A phrase is only started while the bout envelope is at least this high.
const PHRASE_MIN_ENVELOPE: f32 = 0.08;
/// The share of a bout's length at which its swell first reaches `PHRASE_MIN_ENVELOPE` (and,
/// mirrored, the share left when it falls below it again): the audible part of a bout is
/// `AUDIBLE_FROM..1 - AUDIBLE_FROM` of its length.
const AUDIBLE_FROM: f64 = 0.125;
/// Every bout sings at least this many phrases (the first starts as soon as it is audible).
const MIN_PHRASES_PER_BOUT: u32 = 2;
// Loudness trim: the raw voice is quiet; this brings its default RMS level with the other
// situations (see the loudness test), well under the soft ceiling's knee in ordinary play.
const OUTPUT_TRIM: f32 = 4.0;
// The night field: sparse early reflections off the ground and the trees, and a long, dark tail.
const REFLECTION_MS: [f32; 8] = [23.0, 37.0, 52.0, 68.0, 87.0, 109.0, 127.0, 141.0];
const REFLECTION_PAN: [f32; 8] = [0.15, 0.85, 0.35, 0.65, 0.10, 0.90, 0.40, 0.60];
/// Level of the first reflection; each is 0.82 times the one before.
const REFLECTION_LEVEL: f32 = 0.30;
/// The tail: 60 dB decay time, delay-size stretch, the cutoff of the one-pole that darkens it and
/// its level against the reflections.
const TAIL_SECONDS: f64 = 1.6;
const TAIL_SIZE: f64 = 1.5;
const TAIL_DARK_HZ: f32 = 2_500.0;
const TAIL_LEVEL: f32 = 2.2;
/// The night air: a very quiet pink bed (low-passed), its level in the mix (the output trim comes
/// after) and the slow swell of that level.
const BED_LEVEL: f32 = 0.0013;
const BED_CUTOFF_HZ: f32 = 6_000.0;
const BED_SWELL_SECONDS: f64 = 41.0;
const VISUAL: VisualStyle = VisualStyle {
    color: [0.16, 0.20, 0.45],
    shape: VisualShape::SlowGlow,
    // A starry night: the scene lays a dense star field behind the fireflies.
    stars: true,
};

pub(super) fn definition() -> SituationDefinition {
    SituationDefinition {
        id: "autumn",
        label: "秋の夜長",
        knobs: &KNOBS,
        visual: VISUAL,
        grain_capacity: MAX_INSECTS,
        prepare: |config| Ok(Box::new(AutumnVoice::new(config)?)),
    }
}

/// The long-period swell of one insect: `u` is the bout's age over its length. It rises from 0,
/// holds 1 around the middle and falls back to exactly 0 at the end, smoothly (zero slope at both
/// ends and where it reaches 1). The audio gain of the bout and the brightness of its firefly are
/// both this value, so they agree.
pub(crate) fn bout_envelope(u: f32) -> f32 {
    if !u.is_finite() || u <= 0.0 || u >= 1.0 {
        return 0.0;
    }
    let bell = (PI * u).sin();
    // 1.25 x sin^2 reaches 1 over the middle 30% of the bout: a rise and a fall of about a third
    // each (1.4 s of a 4 s bout, 4 s of a 12 s one), with a short plateau between.
    let lift = (1.25 * bell * bell).min(1.0);
    lift * lift * (3.0 - 2.0 * lift)
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn splitmix(seed: u64) -> u64 {
    let mut value = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Species {
    /// 鈴虫: a clear bell ring.
    Suzumushi,
    /// コオロギ: a few short pulses, "コロコロ".
    Cricket,
    /// マツムシ: three notes, "チン・チロ・リン".
    Matsumushi,
    /// クツワムシ: short rattling volleys, "ガチャガチャ"; rarer and quieter.
    Kutsuwamushi,
    /// カネタタキ: a small high tap, "チン", at a steady interval.
    Kanetataki,
    /// カンタン: a long, clear, soft trill, "ルルルル".
    Kantan,
    /// ツヅレサセコオロギ: a fast, fine, shallow trill, "リリリリ", then a rest.
    Tsuzure,
    /// アオマツムシ: strong, long notes, "リーリーリー"; a little louder than the rest.
    Aomatsumushi,
    /// クサヒバリ: a high, quiet trill that starts with a short rising chirp, "フィリリリ".
    Kusahibari,
    /// ウマオイ: a rough, long "スイーー" and then a short "チョン".
    Umaoi,
    /// ケラ: a low, long, steady drone, "ジーーー"; far and quiet, a background.
    Kera,
}

/// Singers in enabling order: the insects knob switches them on one after another, so a low
/// setting already has several kinds.
const SPECIES: [Species; MAX_INSECTS] = [
    Species::Suzumushi,
    Species::Cricket,
    Species::Matsumushi,
    Species::Kanetataki,
    Species::Suzumushi,
    Species::Kantan,
    Species::Cricket,
    Species::Aomatsumushi,
    Species::Matsumushi,
    Species::Kutsuwamushi,
    Species::Tsuzure,
    Species::Umaoi,
    Species::Kusahibari,
    Species::Kera,
    Species::Suzumushi,
    Species::Cricket,
    Species::Matsumushi,
    Species::Kanetataki,
    Species::Suzumushi,
    Species::Aomatsumushi,
    Species::Cricket,
    Species::Matsumushi,
    Species::Suzumushi,
    Species::Kantan,
];

// Cricket: pulse spacing and length. Matsumushi: note starts, lengths, pitch ratios and levels.
const CRICKET_SPACING: f32 = 0.085;
const CRICKET_PULSE: f32 = 0.050;
const MATSUMUSHI_STARTS: [f32; 3] = [0.0, 0.27, 0.55];
const MATSUMUSHI_LENGTHS: [f32; 3] = [0.09, 0.12, 0.24];
const MATSUMUSHI_RATIOS: [f32; 3] = [1.06, 0.93, 1.0];
const MATSUMUSHI_LEVELS: [f32; 3] = [0.9, 0.7, 1.0];
// Kutsuwamushi: volley spacing and length (the pulses inside it are the rattle's grains), and
// the second partial and the noise of its rough voice.
const KUTSUWAMUSHI_SPACING: f32 = 0.34;
const KUTSUWAMUSHI_VOLLEY: f32 = 0.20;
const KUTSUWAMUSHI_PARTIAL: f32 = 1.08;
const ROUGH_HZ: f32 = 120.0;
const ROUGH_DEPTH: f32 = 0.3;
// Each matsumushi note's pulse speed and depth relative to the insect's own.
const MATSUMUSHI_PULSE_RATES: [f32; 3] = [1.08, 0.92, 1.0];
const MATSUMUSHI_PULSE_DEPTHS: [f32; 3] = [0.85, 1.15, 1.0];
// Kusahibari: the rising chirp that starts each phrase (its length and the pitch it starts at).
const KUSAHIBARI_CHIRP: f32 = 0.10;
const KUSAHIBARI_FROM: f32 = 0.92;
/// Seconds over which the trill's pulse depth fades in after the chirp.
const KUSAHIBARI_FADE_IN: f32 = 0.03;
// Umaoi: the pause between the long "スイー" and the short "チョン", and the length and pitch of
// the "チョン"; the long part glides up a little.
const UMAOI_PAUSE: f32 = 0.14;
const UMAOI_CHON: f32 = 0.07;
const UMAOI_CHON_RATIO: f32 = 1.10;
const UMAOI_GLIDE: (f32, f32) = (0.97, 1.03);
// Kanetataki: how long one tick rings (each tick draws its own).
const KANETATAKI_RING_MIN: f32 = 0.08;
const KANETATAKI_RING_MAX: f32 = 0.11;

impl Species {
    /// The SPEC's frequency band of the species, in Hz (checked by the spectrum tests).
    #[cfg(test)]
    const fn band(self) -> (f32, f32) {
        match self {
            Self::Suzumushi => (4_000.0, 5_500.0),
            Self::Cricket => (3_000.0, 4_500.0),
            Self::Matsumushi => (3_000.0, 4_000.0),
            Self::Kutsuwamushi => (2_000.0, 3_000.0),
            Self::Kanetataki => (6_000.0, 8_000.0),
            Self::Kantan => (2_200.0, 3_200.0),
            Self::Tsuzure => (3_500.0, 4_500.0),
            Self::Aomatsumushi => (4_500.0, 5_800.0),
            Self::Kusahibari => (5_500.0, 7_000.0),
            Self::Umaoi => (2_500.0, 4_000.0),
            Self::Kera => (1_500.0, 2_200.0),
        }
    }

    /// The pitch the individuals of a species cluster around, the middle of the band's safe part
    /// (so the per-call jitter, the drift, matsumushi's note ratios and kutsuwamushi's partial stay
    /// inside the band). Each insect sits a few to a few tens of hertz off it (its detune).
    const fn pitch_center(self) -> f32 {
        match self {
            Self::Suzumushi => 4_750.0,
            Self::Cricket => 3_750.0,
            Self::Matsumushi => 3_475.0,
            Self::Kutsuwamushi => 2_300.0,
            Self::Kanetataki => 7_000.0,
            Self::Kantan => 2_700.0,
            Self::Tsuzure => 4_000.0,
            Self::Aomatsumushi => 5_150.0,
            Self::Kusahibari => 6_250.0,
            Self::Umaoi => 3_100.0,
            Self::Kera => 1_850.0,
        }
    }

    /// Seconds of silence between a phrase's end and the next at the peak of a bout.
    const fn gap_range(self) -> (f32, f32) {
        match self {
            Self::Suzumushi => (1.10, 1.80),
            Self::Cricket => (2.2, 3.6),
            Self::Matsumushi => (2.4, 3.6),
            Self::Kutsuwamushi => (3.8, 5.0),
            Self::Kanetataki => (0.80, 1.20),
            Self::Kantan => (1.4, 2.4),
            Self::Tsuzure => (1.8, 3.2),
            Self::Aomatsumushi => (0.4, 0.9),
            Self::Kusahibari => (1.5, 2.6),
            Self::Umaoi => (2.6, 4.2),
            Self::Kera => (2.0, 3.5),
        }
    }

    /// Seconds a bout of this species lasts, within the SPEC's 4 to 12. The slower callers sing
    /// longer bouts, so each bout holds several of their phrases.
    const fn bout_range(self) -> (f64, f64) {
        match self {
            Self::Suzumushi | Self::Kanetataki => (BOUT_MIN_SECONDS, BOUT_MAX_SECONDS),
            Self::Cricket | Self::Matsumushi => (6.0, BOUT_MAX_SECONDS),
            Self::Kutsuwamushi => (8.0, BOUT_MAX_SECONDS),
            Self::Aomatsumushi | Self::Kusahibari => (6.0, BOUT_MAX_SECONDS),
            Self::Kantan | Self::Tsuzure | Self::Umaoi => (8.0, BOUT_MAX_SECONDS),
            Self::Kera => (10.0, BOUT_MAX_SECONDS),
        }
    }

    /// Raw phrase amplitude (before the output trim; the calling insects' sum is quiet). The
    /// differences are the point: kutsuwamushi and kanetataki are the small, quiet ones.
    const fn level(self) -> f32 {
        match self {
            Self::Suzumushi => 0.0589,
            Self::Cricket => 0.1106,
            Self::Matsumushi => 0.0786,
            Self::Kutsuwamushi => 0.0614,
            Self::Kanetataki => 0.0405,
            Self::Kantan => 0.0323,
            Self::Tsuzure => 0.0312,
            Self::Aomatsumushi => 0.0659,
            Self::Kusahibari => 0.0299,
            Self::Umaoi => 0.043,
            Self::Kera => 0.0338,
        }
    }

    /// How long this species rests between bouts, relative to the others (kutsuwamushi is rarer).
    const fn rest_scale(self) -> f64 {
        match self {
            Self::Kutsuwamushi => 1.8,
            Self::Kera => 1.3,
            _ => 1.0,
        }
    }

    /// The species' wing-stroke pulse: the range of its rate (Hz) and of its depth. `None` for
    /// the kanetataki, which has no fast pulse.
    const fn pulse(self) -> Option<((f32, f32), (f32, f32))> {
        match self {
            Self::Suzumushi => Some(((50.0, 90.0), (0.30, 0.50))),
            Self::Cricket => Some(((30.0, 50.0), (0.82, 0.95))),
            // Three notes, each with its own speed and depth (see the per-note scales).
            Self::Matsumushi => Some(((27.5, 31.5), (0.40, 0.55))),
            Self::Kutsuwamushi => Some(((15.0, 30.0), (0.80, 0.95))),
            Self::Kanetataki => None,
            Self::Kantan => Some(((37.0, 56.0), (0.45, 0.65))),
            Self::Tsuzure => Some(((64.0, 86.0), (0.25, 0.40))),
            Self::Aomatsumushi => Some(((42.0, 53.0), (0.40, 0.60))),
            Self::Kusahibari => Some(((73.0, 96.0), (0.30, 0.50))),
            Self::Umaoi => Some(((30.0, 45.0), (0.60, 0.80))),
            Self::Kera => Some(((32.0, 43.0), (0.45, 0.65))),
        }
    }

    /// The rattling voice: a second partial just above the first and noise in the loudness.
    const fn rough(self) -> bool {
        matches!(self, Self::Kutsuwamushi | Self::Umaoi)
    }

    /// A steady caller keeps its interval whatever its bout's loudness (the tap).
    const fn steady(self) -> bool {
        matches!(self, Self::Kanetataki)
    }
}

/// Detune of an individual from its species' pitch: a few to a few tens of hertz either way.
const DETUNE_MIN_HZ: f32 = 3.0;
const DETUNE_MAX_HZ: f32 = 30.0;
/// The per-call variation of each phrase: loudness (dB), pitch (share) and pulse interval (share).
const CALL_GAIN_DB: f32 = 1.5;
const CALL_PITCH_HZ: f32 = 4.0;
const CALL_PULSE: f64 = 0.05;
/// The slow, temperature-like drift of an insect's pitch (share).
const PITCH_DRIFT_HZ: f32 = 4.0;

/// Distance step of each pool slot: a designed mix of near and far, so every prefix of the pool
/// (every setting of the insects knob) sounds about as loud, whatever the seed. Chosen by
/// annealing the per-voice mean power of every prefix (to within about two decibels), with the
/// SPEC's claims fixed: the louder アオマツムシ (slots 7 and 19) near, and the quiet
/// クサヒバリ and ケラ (12 and 13) far or mid.
const DISTANCE_TABLE: [usize; MAX_INSECTS] = [
    0, 2, 2, 0, 0, 0, 2, 0, 1, 2, 0, 0, 2, 2, 0, 2, 0, 1, 0, 0, 0, 0, 3, 1,
];

/// Where insect `index` sits for `seed`: the table's entry. Only the first two suzumushi (slots 0
/// and 4, always enabled together) trade places with the seed, so which of them is near changes
/// from run to run while the loudness of every prefix of the pool does not.
fn distance_for(index: usize, seed: u64) -> usize {
    let slot = index % MAX_INSECTS;
    let swap = splitmix(seed ^ 0xd157_a9ce) & 1 == 1;
    let slot = match (slot, swap) {
        (0, true) => 4,
        (4, true) => 0,
        _ => slot,
    };
    DISTANCE_TABLE[slot]
}

// Distance. Each insect sits at one of four distances; the farther, the quieter (inverse
// distance), duller (air absorbs the highs: a one-pole low-pass, none for the nearest), later
// (sound travels 343 m/s) and the more of it reaches the room (reverb send relative to the
// direct sound).
const DISTANCE_METERS: [f32; 4] = [2.0, 3.5, 6.0, 10.0];
const DISTANCE_CUTOFF_HZ: [f32; 4] = [f32::INFINITY, 11_000.0, 7_000.0, 4_500.0];
const DISTANCE_WET: [f32; 4] = [0.12, 0.25, 0.45, 0.75];
const SOUND_SPEED: f32 = 343.0;
/// Overall trim of the insects (dry and send) into the mix, settled by the loudness test.
const INSECT_TRIM: f32 = 0.43;

/// A fixed-size delay line whose contents are never cleared: after `invalidate` nothing older
/// than the new writes can be read (a read before enough writes returns 0), so it is O(1) at
/// any length and safe in the callback.
struct DelayLine {
    samples: Box<[f32]>,
    cursor: usize,
    filled: usize,
}

impl DelayLine {
    fn new(length: usize) -> Self {
        Self {
            samples: vec![0.0; length.max(1)].into_boxed_slice(),
            cursor: 0,
            filled: 0,
        }
    }

    fn push(&mut self, value: f32) {
        self.samples[self.cursor] = value;
        self.cursor += 1;
        if self.cursor == self.samples.len() {
            self.cursor = 0;
        }
        self.filled = (self.filled + 1).min(self.samples.len());
    }

    /// The value pushed `delay` pushes before the latest (0 is the latest); 0 while not yet
    /// written since the last `invalidate`.
    fn tap(&self, delay: usize) -> f32 {
        if delay >= self.filled {
            return 0.0;
        }
        let length = self.samples.len();
        self.samples[(self.cursor + length - 1 - delay) % length]
    }

    fn invalidate(&mut self) {
        self.cursor = 0;
        self.filled = 0;
    }
}

/// One phrase being sung: what was drawn when it started.
#[derive(Clone, Copy)]
struct Phrase {
    age: f64,
    length: f64,
    pitch: f32,
    level: f32,
    count: u32,
    tempo: f32,
    /// A bell's ring length.
    ring: f32,
}

impl Phrase {
    const fn idle() -> Self {
        Self {
            age: 0.0,
            length: 0.0,
            pitch: 4_000.0,
            level: 0.0,
            count: 1,
            tempo: 1.0,
            ring: 0.5,
        }
    }

    fn sounding(&self) -> bool {
        self.age < self.length
    }

    fn cancel(&mut self) {
        self.age = self.length;
    }
}

/// A short smooth pulse: a quick rise and a smooth fall to zero.
fn tap(t: f32, length: f32, attack: f32) -> f32 {
    if !(0.0..length).contains(&t) {
        return 0.0;
    }
    let fall = 1.0 - t / length;
    smooth(t / attack) * fall * fall
}

/// A raised-cosine pulse.
fn hann(t: f32, length: f32) -> f32 {
    if !(0.0..length).contains(&t) {
        return 0.0;
    }
    let s = (PI * t / length).sin();
    s * s
}

/// The amplitude and the pitch ratio of a species' phrase at its age.
fn phrase_sound(species: Species, phrase: &Phrase) -> (f32, f32) {
    let t = phrase.age as f32;
    match species {
        Species::Suzumushi => {
            // A ring: quick onset, a held body, a smooth fall.
            let x = t / phrase.ring;
            if x >= 1.0 {
                (0.0, 1.0)
            } else {
                (smooth(t / 0.012) * smooth((1.0 - x) / 0.75), 1.0)
            }
        }
        Species::Cricket => {
            let spacing = CRICKET_SPACING * phrase.tempo;
            let index = (t / spacing) as u32;
            if index >= phrase.count {
                return (0.0, 1.0);
            }
            let local = t - index as f32 * spacing;
            (
                hann(local, CRICKET_PULSE) * (1.0 - 0.07 * index as f32),
                1.0,
            )
        }
        Species::Matsumushi => {
            for note in 0..3 {
                let local = t - MATSUMUSHI_STARTS[note] * phrase.tempo;
                let amplitude = tap(local, MATSUMUSHI_LENGTHS[note], 0.004);
                if amplitude > 0.0 {
                    return (amplitude * MATSUMUSHI_LEVELS[note], MATSUMUSHI_RATIOS[note]);
                }
            }
            (0.0, 1.0)
        }
        Species::Kutsuwamushi => {
            let spacing = KUTSUWAMUSHI_SPACING * phrase.tempo;
            let volley = (t / spacing) as u32;
            if volley >= phrase.count {
                return (0.0, 1.0);
            }
            let local = t - volley as f32 * spacing;
            // One rattling volley: a quick start, a held body (the pulses are its grains) and a
            // short fall.
            if local >= KUTSUWAMUSHI_VOLLEY {
                return (0.0, 1.0);
            }
            (
                smooth(local / 0.006) * smooth((KUTSUWAMUSHI_VOLLEY - local) / 0.045),
                1.0,
            )
        }
        Species::Kanetataki => (tap(t, phrase.ring, 0.0015), 1.0),
        // The long trills: a slow rise and a slow fall (kantan, kera), a quick start and a short
        // fall (tsuzure, aomatsumushi).
        Species::Kantan => (long_note(t, phrase.length as f32, 0.45, 0.60), 1.0),
        Species::Kera => (long_note(t, phrase.length as f32, 0.35, 0.55), 1.0),
        Species::Tsuzure => (long_note(t, phrase.length as f32, 0.03, 0.10), 1.0),
        Species::Aomatsumushi => (long_note(t, phrase.length as f32, 0.04, 0.15), 1.0),
        Species::Kusahibari => (
            long_note(t, phrase.length as f32, 0.015, 0.15),
            KUSAHIBARI_FROM + (1.0 - KUSAHIBARI_FROM) * smooth(t / KUSAHIBARI_CHIRP),
        ),
        Species::Umaoi => {
            let chon = t - (phrase.ring + UMAOI_PAUSE);
            if t < phrase.ring {
                (
                    long_note(t, phrase.ring, 0.05, 0.15),
                    UMAOI_GLIDE.0 + (UMAOI_GLIDE.1 - UMAOI_GLIDE.0) * smooth(t / phrase.ring),
                )
            } else if chon >= 0.0 {
                (0.9 * tap(chon, UMAOI_CHON, 0.004), UMAOI_CHON_RATIO)
            } else {
                (0.0, 1.0)
            }
        }
    }
}

/// A held note of `length` seconds: a smooth rise over `rise` and a smooth fall over `fall`.
fn long_note(t: f32, length: f32, rise: f32, fall: f32) -> f32 {
    if !(0.0..length).contains(&t) {
        return 0.0;
    }
    smooth(t / rise) * smooth((length - t) / fall)
}

/// The pulse's rate follows the swell (warmer, faster) by up to this share either way, and
/// wanders slowly by this share.
const PULSE_WARMTH: f64 = 0.08;
const PULSE_WANDER: f64 = 0.03;
/// The deepest a pulse may be (never fully off).
const MAX_DEPTH: f32 = 0.97;
/// Share of a pulse period spent rising: a wing stroke bites fast and dies away a little slower.
const PULSE_ATTACK: f32 = 0.28;
/// A suzumushi's pitch rises by this share (0.4%) at the top of each stroke.
const SUZUMUSHI_WOBBLE: f32 = 0.004;

/// One wing-stroke pulse over its period (`p` in 0..1): 0 at both ends, 1 at the top, a quick
/// smooth rise and a slightly longer smooth fall, so there is no step anywhere.
fn pulse_shape(p: f32) -> f32 {
    if p < PULSE_ATTACK {
        smooth(p / PULSE_ATTACK)
    } else {
        let q = smooth((1.0 - p) / (1.0 - PULSE_ATTACK));
        q * q.sqrt()
    }
}

/// The pitch factor of a species at the top of its stroke: only the suzumushi's bell wobbles.
fn pitch_wobble(species: Species, shape: f32) -> f32 {
    if species == Species::Suzumushi {
        1.0 + SUZUMUSHI_WOBBLE * shape
    } else {
        1.0
    }
}

/// Which grain of the phrase a moment belongs to and how that grain scales the pulse: its rate
/// and depth. Each cricket chirp and kutsuwamushi volley is a grain, so its strokes start afresh;
/// each of a matsumushi's three notes also has a speed and depth of its own.
fn stroke_context(species: Species, t: f32, tempo: f32, ring: f32) -> (u32, f32, f32) {
    match species {
        Species::Cricket => ((t / (CRICKET_SPACING * tempo)) as u32, 1.0, 1.0),
        Species::Kutsuwamushi => ((t / (KUTSUWAMUSHI_SPACING * tempo)) as u32, 1.0, 1.0),
        Species::Matsumushi => {
            let note = (0..3)
                .rev()
                .find(|note| t >= MATSUMUSHI_STARTS[*note] * tempo)
                .unwrap_or(0);
            (
                note as u32,
                MATSUMUSHI_PULSE_RATES[note],
                MATSUMUSHI_PULSE_DEPTHS[note],
            )
        }
        // The chirp glides without a pulse; the trill after it pulses.
        // The trill's depth fades in just after the chirp (one grain throughout: the pulse's
        // phase is never restarted, so the gain is 1 at the handover and never steps).
        Species::Kusahibari => (0, 1.0, smooth((t - KUSAHIBARI_CHIRP) / KUSAHIBARI_FADE_IN)),
        // The long part pulses roughly; the short "チョン" after it is a lighter tap.
        Species::Umaoi if t < ring + UMAOI_PAUSE => (0, 1.0, 1.0),
        Species::Umaoi => (1, 1.0, 0.4),
        _ => (0, 1.0, 1.0),
    }
}

/// Everything an insect needs to know about this sample.
struct Moment {
    delta: f64,
    time: f64,
    /// The insects knob, already ramped.
    knob: f32,
    pool: usize,
    max_singing: usize,
    contributing: bool,
}

struct Insect {
    species: Species,
    index: usize,
    random: WorldRandom,
    /// Normalised left-right position: the pan of the sound and `x` of every event it emits.
    x: f32,
    pan: [f32; 2],
    pitch: f32,
    loudness: f32,
    brightness: f32,
    rest_bias: f64,
    /// This insect's own pause between phrases at the peak of a bout.
    gap_base: f32,
    drift_period: f64,
    drift_phase: f64,
    /// The bout in progress (`bout_length` 0 while resting).
    bout_age: f64,
    bout_length: f64,
    rest_remaining: f64,
    /// [`bout_envelope`] of the bout in progress, 0 outside one.
    envelope: f32,
    gap_remaining: f64,
    /// How many phrases the bout in progress has started.
    phrases_in_bout: u32,
    phrase: Phrase,
    phase: f32,
    partial_phase: f32,
    /// The wing-stroke pulse: this insect's own rate (Hz, before the swell and wander) and depth,
    /// the rate in use now, the phase (cycles, f64), the grain it belongs to, and its last gain.
    pulse_base: f64,
    pulse_depth: f32,
    pulse_rate: f64,
    pulse_phase: f64,
    pulse_note: u32,
    /// Rough voice (kutsuwamushi): the one-pole coefficient and unit-spread gain for the rate.
    rough_pole: f32,
    rough_gain: f32,
    /// A slow wander of the pulse rate: its period (s) and phase.
    wander_period: f64,
    wander_phase: f64,
    /// The noise of a rough voice (kutsuwamushi): generator and one-pole state.
    rng: u64,
    rough: f32,
    /// Per-call variation: its own stream (never the bout/phrase stream) and the pulse-interval
    /// factor of the phrase being sung (the loudness factor is in the phrase's level).
    call_random: WorldRandom,
    call_pulse: f64,
    /// Distance step (0 nearest) and what it sets for the current rate: gain, low-pass state and
    /// coefficient (1 = none), the arrival delay in samples and the reverb send per direct sample.
    distance_step: usize,
    distance_gain: f32,
    air: f32,
    air_coef: f32,
    delay: DelayLine,
    delay_samples: usize,
    send: f32,
    /// Consecutive silent samples so far, and how many make the path settled (the delay line's
    /// length plus a margin for the air's filter).
    quiet: usize,
    settle: usize,
}

impl Insect {
    fn new(index: usize, seed: u64) -> Self {
        let mut random = WorldRandom::new(splitmix(
            seed ^ (index as u64 + 1).wrapping_mul(0x2545_f491_4f6c_dd1d),
        ));
        let species = SPECIES[index % MAX_INSECTS];
        // The draw that used to place the pitch now places the detune (same stream position, so
        // every later draw, the pan included, is unchanged): the sign and a size of 3 to 30 Hz.
        let detune_draw = random.unit() as f32;
        let detune = (detune_draw - 0.5).signum()
            * (DETUNE_MIN_HZ + (DETUNE_MAX_HZ - DETUNE_MIN_HZ) * (2.0 * (detune_draw - 0.5).abs()));
        let pitch = species.pitch_center() + detune;
        // The golden ratio spreads neighbours across the stereo field; a little jitter keeps the
        // places from being a pattern.
        let spread = (index as f32 * 0.618_034 + 0.12 * random.unit() as f32).fract();
        let x = 0.08 + 0.84 * spread;
        let angle = x * FRAC_PI_2;
        let loudness = 0.88 + 0.12 * random.unit() as f32;
        let brightness = random.unit() as f32;
        let rest_bias = 0.8 + 0.4 * random.unit();
        let (gap_low, gap_high) = species.gap_range();
        let gap_base = gap_low + (gap_high - gap_low) * random.unit() as f32;
        let drift_period = 17.0 + 28.0 * random.unit();
        let drift_phase = std::f64::consts::TAU * random.unit();
        let rest_remaining = 0.5 + 9.0 * random.unit();
        // Its wing-stroke pulse (rate and depth within the species' range) and the slow wander of
        // that rate, from its own generator, so the pulse never disturbs the draws above or the
        // bouts after.
        let mut pulse_random = WorldRandom::new(splitmix(seed ^ 0x9015_e5ed ^ index as u64));
        let (pulse_base, pulse_depth) = match species.pulse() {
            Some(((rate_low, rate_high), (depth_low, depth_high))) => (
                f64::from(rate_low + (rate_high - rate_low) * pulse_random.unit() as f32),
                depth_low + (depth_high - depth_low) * pulse_random.unit() as f32,
            ),
            None => (0.0, 0.0),
        };
        let wander_period = 9.0 + 14.0 * pulse_random.unit();
        let wander_phase = TAU64 * pulse_random.unit();
        // Its own stream for where it sits and for the small variation of each call.
        let distance_step = distance_for(index, seed);
        let call_random = WorldRandom::new(splitmix(seed ^ 0xca11_0000 ^ index as u64));
        let mut insect = Self {
            species,
            index,
            random,
            x,
            pan: [angle.cos(), angle.sin()],
            pitch,
            loudness,
            brightness,
            rest_bias,
            gap_base,
            drift_period,
            drift_phase,
            bout_age: 0.0,
            bout_length: 0.0,
            rest_remaining,
            envelope: 0.0,
            gap_remaining: 0.0,
            phrases_in_bout: 0,
            phrase: Phrase::idle(),
            phase: 0.0,
            partial_phase: 0.0,
            pulse_base,
            pulse_depth,
            pulse_rate: pulse_base,
            pulse_phase: 0.0,
            pulse_note: 0,
            rough_pole: 0.0,
            rough_gain: 1.0,
            wander_period,
            wander_phase,
            rng: splitmix(seed ^ 0x5eed ^ index as u64) | 1,
            rough: 0.0,
            call_random,
            call_pulse: 1.0,
            distance_step,
            distance_gain: 1.0,
            air: 0.0,
            air_coef: 1.0,
            delay: DelayLine::new(1),
            delay_samples: 0,
            send: 0.0,
            quiet: 0,
            settle: 0,
        };
        insect.set_rate(48_000);
        insect
    }

    fn singing(&self) -> bool {
        self.bout_length > 0.0
    }

    /// The slow drift of this insect's periods, in -1..1, a function of the world time.
    fn drift(&self, time: f64) -> f64 {
        (std::f64::consts::TAU * time / self.drift_period + self.drift_phase).sin()
    }

    /// The knob switches insects on one after another: insect `i` may sing while the knob is
    /// above `i / pool`.
    fn enabled(&self, moment: &Moment) -> bool {
        moment.knob * moment.pool as f32 > self.index as f32
    }

    /// The pulse rate now: the insect's own, up to 8% faster at the top of the bout's swell and
    /// 8% slower at its ends (warmer, faster), and wandering slowly by 3% with the world time.
    fn update_pulse(&mut self, time: f64) {
        let warmth = 1.0 + PULSE_WARMTH * (2.0 * f64::from(self.envelope) - 1.0);
        let wander =
            1.0 + PULSE_WANDER * (TAU64 * time / self.wander_period + self.wander_phase).sin();
        self.pulse_rate = self.pulse_base * warmth * wander;
    }

    /// Advance the world by one sample. Returns +1 if a bout started, -1 if one ended. A bout in
    /// progress always sings to its end (its firefly matches a voice that is really there), so
    /// only a NEW bout waits for the knob to enable the insect.
    fn advance(&mut self, moment: &Moment, singing: usize, events: &mut VisualWriter<'_>) -> i32 {
        let enabled = self.enabled(moment);
        let mut change = 0;
        if self.singing() {
            self.bout_age += moment.delta;
            if self.bout_age >= self.bout_length {
                self.end_bout(moment);
                change = -1;
            }
        } else {
            // A disabled insect keeps the rest it drew, so it comes back at its own time.
            if enabled {
                self.rest_remaining -= moment.delta;
            }
            if self.rest_remaining <= 0.0 {
                if moment.contributing && singing < moment.max_singing {
                    self.start_bout(moment, events);
                    change = 1;
                } else {
                    // Full or hidden: try again a little later, not in a crowd.
                    self.rest_remaining = 0.4 + 1.2 * self.random.unit();
                }
            }
        }
        self.envelope = if self.singing() {
            bout_envelope((self.bout_age / self.bout_length) as f32)
        } else {
            0.0
        };
        if self.phrase.sounding() {
            self.update_pulse(moment.time);
            self.phrase.age += moment.delta;
        } else if self.singing() {
            let speed = if self.species.steady() {
                1.0
            } else {
                // The long swell also shapes how often the insect calls.
                0.35 + 0.65 * f64::from(self.envelope)
            };
            self.gap_remaining -= moment.delta * speed;
            // At exactly zero the knob stops new calls: a bout in progress sings no further
            // phrase (the one sounding finishes) but lives on, with its firefly, to its end.
            if self.gap_remaining <= 0.0
                && moment.contributing
                && moment.knob > 0.0
                && self.envelope >= PHRASE_MIN_ENVELOPE
            {
                self.start_phrase(moment);
            }
        }
        change
    }

    fn start_bout(&mut self, moment: &Moment, events: &mut VisualWriter<'_>) {
        let (shortest, longest) = self.species.bout_range();
        let length =
            shortest + (longest - shortest) * 0.5 * (self.random.unit() + self.random.unit());
        self.bout_age = 0.0;
        self.bout_length = length;
        self.envelope = 0.0;
        self.phrase.cancel();
        // The first phrase starts as soon as the swell is audible, so every bout sings.
        self.gap_remaining = 0.0;
        self.phrases_in_bout = 0;
        let y = FIREFLY_Y.0 + (FIREFLY_Y.1 - FIREFLY_Y.0) * self.random.unit() as f32;
        // One firefly per bout: same left-right position as the insect's pan, alive exactly as
        // long as the bout; the renderer shapes its brightness with `bout_envelope`.
        events.emit(ParticleVisual {
            x: Normalized::new(self.x).expect("bounded insect position"),
            y: Normalized::new(y).expect("bounded firefly height"),
            strength: Normalized::new(
                (0.55 + 0.30 * self.brightness + 0.15 * moment.knob).min(1.0),
            )
            .expect("bounded firefly strength"),
            radius: LogicalPixels::new(4.0 + 3.0 * self.brightness).expect("finite firefly radius"),
            shape: VisualShape::Firefly,
            large: true,
            lifetime: Duration::from_secs_f64(length.max(FIREFLY_MIN_SECONDS)),
        });
    }

    fn end_bout(&mut self, moment: &Moment) {
        self.bout_length = 0.0;
        self.bout_age = 0.0;
        // More insects means shorter rests between bouts, so more of them sing.
        let scale = 1.9 - 1.4 * f64::from(moment.knob);
        self.rest_remaining =
            (6.0 + 6.0 * self.random.unit()) * self.rest_bias * self.species.rest_scale() * scale;
    }

    fn start_phrase(&mut self, moment: &Moment) {
        let drift = self.drift(moment.time);
        // Small absolute offsets (at most 4 Hz each way for the call and for the slow drift), so
        // two same-species insects stay a few tens of hertz apart: a beat, not roughness.
        let jitter = CALL_PITCH_HZ * (self.random.unit() as f32 * 2.0 - 1.0);
        let drifted = PITCH_DRIFT_HZ * drift as f32;
        let tempo = 0.94 + 0.12 * self.random.unit() as f32;
        // This call's own small variation: loudness and pulse interval (its pitch jitter is the
        // main stream's, above).
        let call_gain =
            10f32.powf(CALL_GAIN_DB * (self.call_random.unit() as f32 * 2.0 - 1.0) / 20.0);
        self.call_pulse = 1.0 + CALL_PULSE * (self.call_random.unit() * 2.0 - 1.0);
        let mut phrase = Phrase {
            age: 0.0,
            length: 0.0,
            pitch: self.pitch + jitter + drifted,
            level: self.species.level() * self.loudness * (0.65 + 0.35 * moment.knob) * call_gain,
            count: 1,
            tempo,
            ring: 0.5,
        };
        let pick = self.random.unit();
        match self.species {
            Species::Suzumushi => {
                phrase.ring = 0.4 + 0.5 * pick as f32;
                phrase.length = f64::from(phrase.ring);
            }
            Species::Cricket => {
                phrase.count = 3 + (pick * 3.0) as u32;
                phrase.length =
                    f64::from((phrase.count - 1) as f32 * CRICKET_SPACING * tempo + CRICKET_PULSE);
            }
            Species::Matsumushi => {
                phrase.length = f64::from(MATSUMUSHI_STARTS[2] * tempo + MATSUMUSHI_LENGTHS[2]);
            }
            Species::Kutsuwamushi => {
                phrase.count = 2 + (pick * 2.0) as u32;
                phrase.length = f64::from(
                    (phrase.count - 1) as f32 * KUTSUWAMUSHI_SPACING * tempo + KUTSUWAMUSHI_VOLLEY,
                );
            }
            Species::Kanetataki => {
                // Each tick rings a little differently (and its interval wobbles a little more).
                phrase.ring = KANETATAKI_RING_MIN
                    + (KANETATAKI_RING_MAX - KANETATAKI_RING_MIN) * self.random.unit() as f32;
                phrase.length = f64::from(phrase.ring);
            }
            // The long songs: how long each lasts is drawn within the SPEC's range.
            Species::Kantan => phrase.length = 2.0 + 2.0 * pick,
            Species::Tsuzure => phrase.length = 1.0 + 2.0 * pick,
            Species::Aomatsumushi => phrase.length = 1.0 + pick,
            Species::Kusahibari => phrase.length = 0.8 + 0.7 * pick,
            Species::Kera => phrase.length = 2.0 + 3.0 * pick,
            Species::Umaoi => {
                phrase.ring = 0.6 + 0.4 * pick as f32;
                phrase.length = f64::from(phrase.ring + UMAOI_PAUSE + UMAOI_CHON);
            }
        }
        self.phrase = phrase;
        self.pulse_phase = 0.0;
        self.pulse_note = 0;
        self.phrases_in_bout += 1;
        let wobble = if self.species.steady() { 0.01 } else { 0.05 };
        self.gap_remaining = f64::from(self.gap_base)
            * (1.0 + wobble * drift)
            * (1.0 + 0.02 * (self.random.unit() * 2.0 - 1.0));
        if self.phrases_in_bout < MIN_PHRASES_PER_BOUT {
            // A short bout must still fit its next phrase before the swell fades: bound the pause
            // by what is left of the audible part, at the slowest rate the swell lets it count
            // down (0.4), with a margin. Longer bouts and later phrases keep their own pause.
            let audible_left = self.bout_length * (1.0 - AUDIBLE_FROM) - self.bout_age;
            let room = (audible_left - phrase.length).max(0.0);
            self.gap_remaining = self.gap_remaining.min(0.35 * room);
        }
    }

    /// One mono sample of the insect as heard at its distance: its voice through the space
    /// between (gain, air absorption, arrival delay). The reverb send is `send` times this.
    fn render(&mut self, sample_rate: f32) -> f32 {
        let raw = self.raw_render(sample_rate);
        if raw == 0.0 {
            // Silent: once the delay line holds nothing but zeros (and the air's filter has died
            // away) there is nothing to move, so a resting insect costs a compare.
            if self.quiet >= self.settle {
                return 0.0;
            }
            self.quiet += 1;
            if self.quiet == self.settle {
                self.air = 0.0;
            }
        } else {
            self.quiet = 0;
        }
        self.space(raw)
    }

    /// The insect's own voice at the source: the phrase's shape, times the wing-stroke pulse,
    /// times the bout's swell.
    fn raw_render(&mut self, sample_rate: f32) -> f32 {
        if !self.phrase.sounding() || self.envelope <= 0.0 {
            return 0.0;
        }
        let (amplitude, ratio) = phrase_sound(self.species, &self.phrase);
        if amplitude <= 0.0 {
            return 0.0;
        }
        // The wing stroke belongs to a grain (a cricket's chirp, a matsumushi note, a
        // kutsuwamushi volley): which one and how it scales the pulse.
        let (note, rate_scale, depth_scale) = stroke_context(
            self.species,
            self.phrase.age as f32,
            self.phrase.tempo,
            self.phrase.ring,
        );
        self.voice_sample(amplitude, ratio, note, rate_scale, depth_scale, sample_rate)
    }

    /// The voice itself, given the phrase's amplitude and pitch ratio and the grain's pulse
    /// scales: the carrier (with the species' wobble, partial and roughness) under the
    /// wing-stroke pulse, whose phase restarts with each grain. Kanetataki has no pulse.
    fn voice_sample(
        &mut self,
        amplitude: f32,
        ratio: f32,
        note: u32,
        rate_scale: f32,
        depth_scale: f32,
        sample_rate: f32,
    ) -> f32 {
        if note != self.pulse_note {
            self.pulse_note = note;
            self.pulse_phase = 0.0;
        }
        let (gain, shape) = if self.species.pulse().is_some() {
            self.pulse_phase = (self.pulse_phase
                + self.pulse_rate * f64::from(rate_scale) * self.call_pulse
                    / f64::from(sample_rate))
            .fract();
            let shape = pulse_shape(self.pulse_phase as f32);
            let depth = (self.pulse_depth * depth_scale).min(MAX_DEPTH);
            (1.0 - depth + depth * shape, shape)
        } else {
            (1.0, 0.0)
        };
        let wobble = pitch_wobble(self.species, shape);
        let frequency = (self.phrase.pitch * ratio * wobble).min(sample_rate * 0.40);
        self.phase += TAU * frequency / sample_rate;
        if self.phase >= TAU {
            self.phase -= TAU;
        }
        let mut sample = self.phase.sin();
        if self.species.rough() {
            // The rattle is rough: a second partial just above the first, and noise in its
            // loudness (a few hundred hertz of it, so it stays around the carrier).
            self.partial_phase += TAU * frequency * KUTSUWAMUSHI_PARTIAL / sample_rate;
            if self.partial_phase >= TAU {
                self.partial_phase -= TAU;
            }
            self.rng ^= self.rng >> 12;
            self.rng ^= self.rng << 25;
            self.rng ^= self.rng >> 27;
            let bits = self.rng.wrapping_mul(0x2545_f491_4f6c_dd1d);
            let white = (bits >> 40) as f32 * (2.0 / 16_777_216.0) - 1.0;
            self.rough += (white - self.rough) * self.rough_pole;
            let rough = (1.0 + ROUGH_DEPTH * self.rough * self.rough_gain).max(0.0);
            sample = (sample + 0.7 * self.partial_phase.sin()) * 0.6 * rough;
        }
        sample * amplitude * gain * self.phrase.level * self.envelope
    }

    /// The path from the source to the listener: inverse-distance gain, the air's low-pass (a
    /// one-pole whose cutoff falls with distance; none for the nearest) and the arrival delay.
    fn space(&mut self, source: f32) -> f32 {
        self.air += (source * self.distance_gain - self.air) * self.air_coef;
        self.delay.push(self.air);
        self.delay.tap(self.delay_samples)
    }

    /// Put the insect at distance `step` (0 nearest) for the current rate. No allocation: the
    /// delay line was sized for the farthest step.
    fn set_distance(&mut self, step: usize, sample_rate: u32) {
        let step = step.min(DISTANCE_METERS.len() - 1);
        let rate = sample_rate as f32;
        self.distance_step = step;
        self.distance_gain = DISTANCE_METERS[0] / DISTANCE_METERS[step];
        let cutoff = DISTANCE_CUTOFF_HZ[step].min(rate * 0.4);
        self.air_coef = if DISTANCE_CUTOFF_HZ[step].is_finite() {
            1.0 - (-TAU * cutoff / rate).exp()
        } else {
            1.0
        };
        self.delay_samples = (DISTANCE_METERS[step] / SOUND_SPEED * rate).round() as usize;
        self.send = DISTANCE_WET[step];
    }

    /// The sample-rate constants (worker side, never per sample): the rough voice's one-pole
    /// coefficient and the gain that makes its noise a unit spread, and the distance's low-pass,
    /// delay and a delay line sized for the farthest distance.
    fn set_rate(&mut self, sample_rate: u32) {
        let pole = 1.0 - (-TAU * ROUGH_HZ / sample_rate as f32).exp();
        self.rough_pole = pole;
        self.rough_gain = 1.0 / ((1.0_f32 / 3.0).sqrt() * (pole / (2.0 - pole)).sqrt());
        let farthest = DISTANCE_METERS[DISTANCE_METERS.len() - 1];
        let longest = (farthest / SOUND_SPEED * sample_rate as f32).ceil() as usize + 2;
        self.delay = DelayLine::new(longest);
        self.air = 0.0;
        self.quiet = 0;
        self.settle = longest + 256;
        self.set_distance(self.distance_step, sample_rate);
    }

    fn invalidate_history(&mut self) {
        // A phrase being sung when the voice leaves does not resume; the world (bout, gap)
        // carries on.
        self.phrase.cancel();
        self.phase = 0.0;
        self.partial_phase = 0.0;
        self.pulse_phase = 0.0;
        self.pulse_note = 0;
        self.rough = 0.0;
        self.air = 0.0;
        self.delay.invalidate();
        self.quiet = 0;
    }
}

// Wind: a quiet, continuously undulating breeze behind the insects. A slow wave (period 10 to
// 23 s, the period itself drifting over minutes) rises and falls without a break, with an
// occasional larger swell on top; a low dark rumble and a high leaf rustle, both small. The wind
// is meant to be well below the insects (see the margin test).
const TAU64: f64 = std::f64::consts::TAU;
const WAVE_PERIOD_MIDDLE: f64 = 16.5;
const WAVE_PERIOD_SWING: f64 = 6.5;
const WAVE_DRIFT_SECONDS: f64 = 480.0;
const SWELL_MIN_SECONDS: f64 = 10.0;
const SWELL_MAX_SECONDS: f64 = 18.0;
/// The wind's level never falls below this share of its size: there is no gust-then-silence.
const WIND_FLOOR: f32 = 0.30;
/// The knob's curve: `1 - (1 - x)^3`, 0 at 0 and 1 at 1, steeper at the bottom than the middle so
/// the middle of the slider is already audible, yet with a finite slope at 0 (a knob move never
/// makes the wind jump).
fn wind_curve(x: f32) -> f32 {
    1.0 - (1.0 - x.clamp(0.0, 1.0)).powi(3)
}
const RUMBLE_LEVEL: f32 = 0.0047;
const RUSTLE_LEVEL: f32 = 0.0155;
const RUSTLE_Q: f32 = 0.6;
const FLUTTER_HZ: f32 = 22.0;

struct Wind {
    random: WorldRandom,
    /// Slow drift of the wave's period, and the wave's own phase (both radians, f64).
    drift_phase: f64,
    wave_phase: f64,
    /// The wave, 0 to 1 (a smooth sine of `wave_phase`).
    wave: f32,
    swell_wait: f64,
    swell_age: f64,
    /// 0 while no swell is rising and falling.
    swell_length: f64,
    swell_strength: f32,
    /// The swell's current share, 0 to 1.
    swell: f32,
    /// Per-sample level of the world: floor, wave and swell times the knob's size; 0 at knob 0.
    level: f32,
    rumble_cutoff: f32,
    rustle_center: f32,
    rumble_noise: Noise,
    rumble_pink: Pinkpass<f32>,
    rumble_filters: [Lowpole<f32, U2>; 2],
    rustle_noise: [Noise; 2],
    flutter_noise: [Noise; 2],
    flutter_state: [f32; 2],
    flutter_pole: f32,
    flutter_gain: f32,
    rustle: [Resonator<f32, U3>; 2],
}

fn pole(cutoff: f32, rate: u32) -> f32 {
    1.0 - (-TAU * cutoff.min(rate as f32 * 0.4) / rate as f32).exp()
}

impl Wind {
    fn new(seed: u64, rate: u32) -> Self {
        let mut random = WorldRandom::new(splitmix(seed ^ 0x77_1d));
        let drift_phase = TAU64 * random.unit();
        let wave_phase = TAU64 * random.unit();
        let swell_wait = 20.0 + 40.0 * random.unit();
        let noise = |salt: u64| {
            let mut noise = Noise::new();
            noise.set_hash(seed.wrapping_add(salt));
            noise
        };
        Self {
            random,
            drift_phase,
            wave_phase,
            wave: 0.5,
            swell_wait,
            swell_age: 0.0,
            swell_length: 0.0,
            swell_strength: 0.0,
            swell: 0.0,
            level: 0.0,
            rumble_cutoff: 90.0,
            rustle_center: 1_500.0,
            rumble_noise: noise(0xa54f_f53a_5f1d_36f1),
            rumble_pink: Pinkpass::new(),
            rumble_filters: [Lowpole::new(90.0), Lowpole::new(90.0)],
            rustle_noise: [noise(0x3c6e_f372_fe94_f82a), noise(0xbb67_ae85_84ca_a73b)],
            flutter_noise: [noise(0x510e_527f_ade6_82d1), noise(0x9b05_688c_2b3e_6c1f)],
            flutter_state: [0.0; 2],
            flutter_pole: pole(FLUTTER_HZ, rate),
            flutter_gain: 1.0,
            rustle: std::array::from_fn(|_| Resonator::new(1_500.0, RUSTLE_Q)),
        }
    }

    /// The wave's current period in seconds: 10 to 23, drifting slowly.
    fn period(&self) -> f64 {
        WAVE_PERIOD_MIDDLE + WAVE_PERIOD_SWING * self.drift_phase.sin()
    }

    fn advance(&mut self, delta: f64, wind: f32, contributing: bool, sample_rate: u32) {
        // The slow wave never stops (it is world time): it only becomes audible with the knob.
        self.drift_phase = (self.drift_phase + TAU64 * delta / WAVE_DRIFT_SECONDS) % TAU64;
        self.wave_phase = (self.wave_phase + TAU64 * delta / self.period()) % TAU64;
        self.wave = (0.5 + 0.5 * self.wave_phase.sin()) as f32;
        if self.swell_length > 0.0 {
            self.swell_age += delta;
            if self.swell_age >= self.swell_length {
                self.swell_length = 0.0;
                self.swell_age = 0.0;
                // Swells come more often with a higher knob.
                let scale = f64::from(1.9 - 1.3 * wind);
                self.swell_wait = (35.0 + 45.0 * self.random.unit()) * scale;
            }
        } else {
            if wind > 0.0 {
                self.swell_wait -= delta;
            }
            if self.swell_wait <= 0.0 {
                if contributing {
                    self.swell_length = SWELL_MIN_SECONDS
                        + (SWELL_MAX_SECONDS - SWELL_MIN_SECONDS) * self.random.unit();
                    self.swell_age = 0.0;
                    self.swell_strength = 0.6 + 0.4 * self.random.unit() as f32;
                } else {
                    self.swell_wait = 1.0 + self.random.unit();
                }
            }
        }
        self.swell = if self.swell_length > 0.0 {
            let bell = (PI * (self.swell_age / self.swell_length) as f32).sin();
            bell * bell * self.swell_strength
        } else {
            0.0
        };
        // A floor, the wave, and the occasional larger swell on top; the knob sets the size of
        // all of them and reaches exactly 0 (silent) with it. Never above 1.
        self.level = if wind > 0.0 {
            wind_curve(wind) * (WIND_FLOOR + (0.25 + 0.20 * wind) * self.wave + 0.25 * self.swell)
        } else {
            0.0
        };
        self.rumble_cutoff = 70.0 + 110.0 * self.level;
        // The rustle's band follows the knob, always below Nyquist at any supported rate.
        self.rustle_center = (1_100.0 + 1_300.0 * wind).min(sample_rate as f32 * 0.40);
    }

    /// Stereo wind sample. The noise is scaled before the filters, so the wind dying away with
    /// the knob leaves nothing behind in them.
    fn render(&mut self) -> [f32; 2] {
        if self.level == 0.0 {
            return [0.0; 2];
        }
        let level = self.level;
        let white = self.rumble_noise.tick(&Default::default())[0] * level;
        let pink = self.rumble_pink.tick(&[white].into())[0];
        let first = self.rumble_filters[0].tick(&[pink, self.rumble_cutoff].into())[0];
        let rumble =
            self.rumble_filters[1].tick(&[first, self.rumble_cutoff].into())[0] * RUMBLE_LEVEL;
        std::array::from_fn(|channel| {
            let source = self.rustle_noise[channel].tick(&Default::default())[0];
            // Leaves flutter: the rustle's loudness jitters slowly and unevenly.
            let flutter_white = self.flutter_noise[channel].tick(&Default::default())[0];
            self.flutter_state[channel] +=
                (flutter_white - self.flutter_state[channel]) * self.flutter_pole;
            let flutter =
                (0.25 + 0.9 * (self.flutter_state[channel] * self.flutter_gain).abs()).min(1.6);
            let rustle = self.rustle[channel]
                .tick(&[source * level, self.rustle_center, RUSTLE_Q].into())[0]
                * flutter
                * RUSTLE_LEVEL;
            rumble + rustle
        })
    }

    fn invalidate_history(&mut self) {
        self.rumble_pink.reset();
        for filter in &mut self.rumble_filters {
            filter.reset();
        }
        for resonator in &mut self.rustle {
            resonator.reset();
        }
        self.flutter_state = [0.0; 2];
    }
}

/// The night field around the insects: eight early reflections (a mono delay line tapped at 23 to
/// 141 ms, falling in level, panned across the field) and a damped tail (the Schroeder reverb with
/// a 1.6 s decay, then a one-pole low-pass, so the tail is dark). All storage is prepared per
/// rate; leaving the scene invalidates it without clearing anything.
struct Room {
    early: DelayLine,
    taps: [(usize, f32, f32); 8],
    tail: RestartableReverb,
    dark: [f32; 2],
    dark_coef: f32,
}

impl Room {
    fn new(rate: u32) -> Result<Self, VoicePreparationError> {
        let tail = RestartableReverb::with_room(rate, TAIL_SECONDS, TAIL_SIZE)?;
        let taps = std::array::from_fn(|k| {
            let samples = (REFLECTION_MS[k] / 1_000.0 * rate as f32).round() as usize;
            let level = REFLECTION_LEVEL * 0.82_f32.powi(k as i32);
            let angle = REFLECTION_PAN[k] * FRAC_PI_2;
            (samples, level * angle.cos(), level * angle.sin())
        });
        let longest = taps[taps.len() - 1].0;
        Ok(Self {
            early: DelayLine::new(longest + 2),
            taps,
            tail,
            dark: [0.0; 2],
            dark_coef: 1.0 - (-TAU * TAIL_DARK_HZ.min(rate as f32 * 0.4) / rate as f32).exp(),
        })
    }

    /// The room's wet output for the stereo sound sent to it.
    fn tick(&mut self, send: [f32; 2]) -> [f32; 2] {
        self.early.push(0.5 * (send[0] + send[1]));
        let mut out = [0.0_f32; 2];
        for (delay, left, right) in self.taps {
            let echo = self.early.tap(delay);
            out[0] += echo * left;
            out[1] += echo * right;
        }
        let tail = self.tail.tick(send);
        for channel in 0..2 {
            self.dark[channel] += (tail[channel] - self.dark[channel]) * self.dark_coef;
            out[channel] += self.dark[channel] * TAIL_LEVEL;
        }
        out
    }

    fn invalidate(&mut self) {
        self.early.invalidate();
        self.tail.invalidate();
        self.dark = [0.0; 2];
    }
}

/// The quiet night air: a pink-ish bed, independent per channel, well under everything else. It
/// follows the situation (heard or not), not the wind knob.
struct NightBed {
    noise: [Noise; 2],
    pink: [Pinkpass<f32>; 2],
    low: [Lowpole<f32, U1>; 2],
    level: f32,
    phase: f64,
}

impl NightBed {
    fn new(seed: u64) -> Self {
        let noise = |salt: u64| {
            let mut noise = Noise::new();
            noise.set_hash(seed.wrapping_add(salt));
            noise
        };
        Self {
            noise: [noise(0x2b7e_1516_28ae_d2a6), noise(0xabf7_1588_09cf_4f3c)],
            pink: std::array::from_fn(|_| Pinkpass::new()),
            low: std::array::from_fn(|_| Lowpole::new(BED_CUTOFF_HZ)),
            level: BED_LEVEL,
            phase: splitmix(seed ^ 0xbed) as f64 / u64::MAX as f64 * TAU64,
        }
    }

    /// The level now: the bed breathes by a quarter over 41 s of world time.
    fn advance(&mut self, time: f64) {
        self.level =
            BED_LEVEL * (1.0 + 0.25 * (TAU64 * time / BED_SWELL_SECONDS + self.phase).sin() as f32);
    }

    fn render(&mut self) -> [f32; 2] {
        std::array::from_fn(|channel| {
            let white = self.noise[channel].tick(&Default::default())[0] * self.level;
            let pink = self.pink[channel].tick(&[white].into())[0];
            self.low[channel].tick(&[pink].into())[0]
        })
    }

    fn invalidate_history(&mut self) {
        for pink in &mut self.pink {
            pink.reset();
        }
        for low in &mut self.low {
            low.reset();
        }
    }
}

struct AutumnVoice {
    sample_rate: u32,
    dry_delay: Option<VoiceLatencyFrames>,
    contributing: bool,
    insects_knob: KnobRamp,
    wind_knob: KnobRamp,
    insects: Box<[Insect]>,
    wind: Wind,
    bed: NightBed,
    breath_intensity: Normalized,
    dc: [DCBlock<f32>; 2],
    room: Room,
}

impl AutumnVoice {
    fn new(config: VoicePreparation) -> Result<Self, VoicePreparationError> {
        if !(1..=MAX_INSECTS).contains(&config.grain_capacity) {
            return Err(VoicePreparationError::InvalidParameters);
        }
        let room = Room::new(config.sample_rate)?;
        let mut voice = Self {
            sample_rate: config.sample_rate,
            dry_delay: None,
            contributing: true,
            insects_knob: KnobRamp::new(KNOBS[0].initial),
            wind_knob: KnobRamp::new(KNOBS[1].initial),
            insects: (0..config.grain_capacity)
                .map(|index| Insect::new(index, config.seed))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            wind: Wind::new(config.seed, config.sample_rate),
            bed: NightBed::new(config.seed),
            breath_intensity: Normalized::ZERO,
            dc: std::array::from_fn(|_| DCBlock::new(8.0)),
            room,
        };
        voice.prepare_rate(config.sample_rate)?;
        Ok(voice)
    }

    fn invalidate_history(&mut self) {
        for insect in &mut self.insects {
            insect.invalidate_history();
        }
        self.wind.invalidate_history();
        self.bed.invalidate_history();
        for dc in &mut self.dc {
            dc.reset();
        }
        self.room.invalidate();
    }
}

impl Voice for AutumnVoice {
    fn advance_world(
        &mut self,
        frame: VoiceFrame,
        knobs: &[Normalized],
        events: &mut VisualWriter<'_>,
    ) {
        self.insects_knob.advance(knobs[0], frame.delta_seconds);
        self.wind_knob.advance(knobs[1], frame.delta_seconds);
        let knob = self.insects_knob.value();
        let wind = self.wind_knob.value();
        let moment = Moment {
            delta: frame.delta_seconds,
            time: frame.world.get(),
            knob,
            pool: self.insects.len(),
            // How many may be inside a bout at once: none at exactly 0.
            max_singing: singing_cap(knob),
            contributing: self.contributing,
        };
        let mut singing = self
            .insects
            .iter()
            .filter(|insect| insect.singing())
            .count();
        let mut swell = 0.0_f32;
        for insect in &mut self.insects {
            match insect.advance(&moment, singing, events) {
                1 => singing += 1,
                -1 => singing -= 1,
                _ => {}
            }
            swell += insect.envelope;
        }
        self.bed.advance(frame.world.get());
        self.wind.advance(
            frame.delta_seconds,
            wind,
            self.contributing,
            frame.sample_rate,
        );
        self.breath_intensity = Normalized::new(
            (0.20 + 0.45 * (swell / MAX_SINGING as f32).min(1.0) + 0.30 * self.wind.level)
                .clamp(0.0, 1.0),
        )
        .expect("bounded breath intensity");
    }

    fn render_sample(&mut self) -> [f32; 2] {
        if !self.contributing {
            return [0.0; 2];
        }
        let rate = self.sample_rate as f32;
        let (mut dry, mut send) = ([0.0_f32; 2], [0.0_f32; 2]);
        for insect in &mut self.insects {
            let sample = insect.render(rate) * INSECT_TRIM;
            dry[0] += sample * insect.pan[0];
            dry[1] += sample * insect.pan[1];
            // The farther the insect, the more of its sound reaches the room.
            send[0] += sample * insect.send * insect.pan[0];
            send[1] += sample * insect.send * insect.pan[1];
        }
        let wind = self.wind.render();
        let bed = self.bed.render();
        dry[0] += wind[0] + bed[0];
        dry[1] += wind[1] + bed[1];
        for (sample, dc) in dry.iter_mut().zip(&mut self.dc) {
            *sample = dc.tick(&[*sample].into())[0];
        }
        let wet = self.room.tick(send);
        [
            soft_ceiling((dry[0] + wet[0]) * OUTPUT_TRIM),
            soft_ceiling((dry[1] + wet[1]) * OUTPUT_TRIM),
        ]
    }

    fn set_contributing(&mut self, active: bool) {
        if self.contributing != active {
            self.invalidate_history();
            self.contributing = active;
        }
    }

    fn breath_intensity(&self) -> Normalized {
        self.breath_intensity
    }

    fn prepared_delay(&self) -> Result<VoiceLatencyFrames, VoicePreparationError> {
        self.dry_delay
            .ok_or(VoicePreparationError::UnresolvedCausalDelay)
    }

    fn prepare_rate(
        &mut self,
        sample_rate: u32,
    ) -> Result<VoiceLatencyFrames, VoicePreparationError> {
        if !(8_000..=384_000).contains(&sample_rate) {
            return Err(VoicePreparationError::InvalidRate);
        }
        let room = if sample_rate != self.sample_rate {
            Some(Room::new(sample_rate)?)
        } else {
            None
        };
        // Worker-owned preparation only; the callback only ticks statically typed nodes.
        for dc in &mut self.dc {
            prepare_node(dc, sample_rate);
        }
        let dc_delay = dry_latency(&mut self.dc[0])?;
        if dry_latency(&mut self.dc[1])? != dc_delay {
            return Err(VoicePreparationError::UnresolvedCausalDelay);
        }
        let wind = &mut self.wind;
        prepare_node(&mut wind.rumble_noise, sample_rate);
        prepare_node(&mut wind.rumble_pink, sample_rate);
        let mut rumble_delay = dry_latency(&mut wind.rumble_pink)?;
        for filter in &mut wind.rumble_filters {
            filter.set_cutoff(wind.rumble_cutoff.min(sample_rate as f32 * 0.40));
            prepare_node(filter, sample_rate);
            rumble_delay += dry_latency(filter)?;
        }
        let mut rustle_delay = None;
        for channel in 0..2 {
            prepare_node(&mut wind.rustle_noise[channel], sample_rate);
            prepare_node(&mut wind.flutter_noise[channel], sample_rate);
            let center = wind.rustle_center.min(sample_rate as f32 * 0.40);
            wind.rustle[channel].set_center_q(center, RUSTLE_Q);
            prepare_node(&mut wind.rustle[channel], sample_rate);
            let delay = dry_latency(&mut wind.rustle[channel])?;
            if rustle_delay.is_some_and(|previous| previous != delay) {
                return Err(VoicePreparationError::UnresolvedCausalDelay);
            }
            rustle_delay = Some(delay);
        }
        let bed = &mut self.bed;
        let mut bed_delay = None;
        for channel in 0..2 {
            prepare_node(&mut bed.noise[channel], sample_rate);
            prepare_node(&mut bed.pink[channel], sample_rate);
            bed.low[channel].set_cutoff(BED_CUTOFF_HZ.min(sample_rate as f32 * 0.40));
            prepare_node(&mut bed.low[channel], sample_rate);
            let delay = dry_latency(&mut bed.pink[channel])? + dry_latency(&mut bed.low[channel])?;
            if bed_delay.is_some_and(|previous| previous != delay) {
                return Err(VoicePreparationError::UnresolvedCausalDelay);
            }
            bed_delay = Some(delay);
        }
        // Rumble, rustle and the night bed are parallel paths into the common DC blocker, and the
        // insects are oscillators whose space (delay) is acoustic, not latency: every dry route
        // must agree.
        if rustle_delay != Some(rumble_delay) || bed_delay != Some(rumble_delay) {
            return Err(VoicePreparationError::UnresolvedCausalDelay);
        }
        wind.flutter_pole = pole(FLUTTER_HZ, sample_rate);
        // Normalise the one-pole's noise spread (uniform white, variance 1/3) to about 0.3.
        let spread =
            (1.0_f32 / 3.0).sqrt() * (wind.flutter_pole / (2.0 - wind.flutter_pole)).sqrt();
        wind.flutter_gain = 0.3 / spread;
        for insect in &mut self.insects {
            insect.set_rate(sample_rate);
        }
        self.sample_rate = sample_rate;
        if let Some(room) = room {
            self.room = room;
        } else {
            self.room.invalidate();
        }
        let delay = VoiceLatencyFrames::new(rumble_delay + dc_delay)
            .ok_or(VoicePreparationError::UnresolvedCausalDelay)?;
        self.dry_delay = Some(delay);
        Ok(delay)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        audio::{
            timing::{MachNanos, StreamEpoch, WorldSeconds},
            transport::VisualRing,
        },
        situations::SituationIndex,
    };

    fn voice(rate: u32, knobs: [Normalized; 2]) -> AutumnVoice {
        voice_seeded(19, rate, knobs)
    }

    fn voice_seeded(seed: u64, rate: u32, knobs: [Normalized; 2]) -> AutumnVoice {
        let mut voice = AutumnVoice::new(VoicePreparation {
            sample_rate: rate,
            seed,
            grain_capacity: MAX_INSECTS,
        })
        .unwrap();
        voice.insects_knob = KnobRamp::new(knobs[0]);
        voice.wind_knob = KnobRamp::new(knobs[1]);
        voice
    }

    fn now(sample: usize, rate: u32) -> MachNanos {
        MachNanos::new((sample as f64 / f64::from(rate) * 1e9) as u64)
    }

    fn step(
        voice: &mut AutumnVoice,
        knobs: &[Normalized; 2],
        ring: &VisualRing,
        sample: usize,
    ) -> [f32; 2] {
        let rate = voice.sample_rate;
        advance(
            voice,
            knobs,
            ring,
            sample as f64 / f64::from(rate),
            1.0 / f64::from(rate),
        );
        voice.render_sample()
    }

    /// Advance only the world (bouts, fireflies, knobs) to `time` by a step of `delta` seconds,
    /// emitting its events at that time: a coarse step runs minutes of world in a moment.
    fn advance(
        voice: &mut AutumnVoice,
        knobs: &[Normalized; 2],
        ring: &VisualRing,
        time: f64,
        delta: f64,
    ) {
        let world = WorldSeconds::new(time).unwrap();
        let mut events = VisualWriter::new(
            ring,
            StreamEpoch::new(1).unwrap(),
            SituationIndex::from_index(0, 1).unwrap(),
            world,
            MachNanos::new((time * 1e9) as u64),
        );
        voice.advance_world(
            VoiceFrame {
                world,
                delta_seconds: delta,
                sample_rate: voice.sample_rate,
            },
            knobs,
            &mut events,
        );
    }

    fn knobs(insects: f32, wind: f32) -> [Normalized; 2] {
        [
            Normalized::new(insects).unwrap(),
            Normalized::new(wind).unwrap(),
        ]
    }

    /// Every species, and the pool slots a species sings from.
    const ALL_SPECIES: [Species; 11] = [
        Species::Suzumushi,
        Species::Cricket,
        Species::Matsumushi,
        Species::Kutsuwamushi,
        Species::Kanetataki,
        Species::Kantan,
        Species::Tsuzure,
        Species::Aomatsumushi,
        Species::Kusahibari,
        Species::Umaoi,
        Species::Kera,
    ];

    fn slots(species: Species) -> Vec<usize> {
        (0..MAX_INSECTS)
            .filter(|index| SPECIES[*index] == species)
            .collect()
    }

    /// A lone insect in the middle of a very long bout (envelope 1 throughout), so its phrases
    /// repeat at the bout's peak rate.
    fn plateau_insect(index: usize, seed: u64) -> Insect {
        let mut insect = Insect::new(index, seed);
        insect.bout_length = 1e9;
        insect.bout_age = 5e8;
        insect
    }

    fn moment(time: f64, delta: f64, knob: f32) -> Moment {
        Moment {
            delta,
            time,
            knob,
            pool: MAX_INSECTS,
            max_singing: MAX_SINGING,
            contributing: true,
        }
    }

    /// Energy of `samples` at `frequency` (a single DFT bin).
    fn energy_at(samples: &[f32], frequency: f32, rate: f32) -> f64 {
        let (mut real, mut imag) = (0.0_f64, 0.0_f64);
        for (index, sample) in samples.iter().enumerate() {
            let phase =
                std::f64::consts::TAU * f64::from(frequency) * index as f64 / f64::from(rate);
            real += f64::from(*sample) * phase.cos();
            imag += f64::from(*sample) * phase.sin();
        }
        real * real + imag * imag
    }

    /// In-place radix-2 FFT of `data` (its length a power of two).
    fn fft(data: &mut [(f64, f64)]) {
        let n = data.len();
        let mut j = 0;
        for i in 1..n {
            let mut bit = n >> 1;
            while j & bit != 0 {
                j ^= bit;
                bit >>= 1;
            }
            j ^= bit;
            if i < j {
                data.swap(i, j);
            }
        }
        let mut length = 2;
        while length <= n {
            let angle = -std::f64::consts::TAU / length as f64;
            for start in (0..n).step_by(length) {
                for k in 0..length / 2 {
                    let (wr, wi) = ((angle * k as f64).cos(), (angle * k as f64).sin());
                    let (ar, ai) = data[start + k];
                    let (br, bi) = data[start + k + length / 2];
                    let (tr, ti) = (br * wr - bi * wi, br * wi + bi * wr);
                    data[start + k] = (ar + tr, ai + ti);
                    data[start + k + length / 2] = (ar - tr, ai - ti);
                }
            }
            length <<= 1;
        }
    }

    /// The share of the energy between 400 Hz and 0.45 x the rate that lies in `low..=high`,
    /// from a zero-padded FFT of at most 1.5 Hz per bin (the narrow-line voices must not fall
    /// between the bins).
    fn band_share(samples: &[f32], rate: f32, low: f32, high: f32) -> f64 {
        let size = ((rate / 1.5) as usize)
            .next_power_of_two()
            .max(samples.len().next_power_of_two());
        let mut data = vec![(0.0_f64, 0.0_f64); size];
        for (slot, sample) in data.iter_mut().zip(samples) {
            slot.0 = f64::from(*sample);
        }
        fft(&mut data);
        let bin = f64::from(rate) / size as f64;
        let (mut inside, mut total) = (0.0, 0.0);
        for (index, (re, im)) in data.iter().enumerate().take(size / 2) {
            let frequency = index as f64 * bin;
            if frequency < 400.0 || frequency >= f64::from(rate) * 0.45 {
                continue;
            }
            let energy = re * re + im * im;
            total += energy;
            if (f64::from(low)..=f64::from(high)).contains(&frequency) {
                inside += energy;
            }
        }
        inside / total
    }

    /// One phrase of an insect, rendered offline at the full bout swell.
    fn rendered_phrase(index: usize, seed: u64, rate: u32) -> (Insect, Vec<f32>) {
        let mut insect = plateau_insect(index, seed);
        insect.set_rate(rate);
        insect.envelope = 1.0;
        insect.start_phrase(&moment(3.0, 1.0 / f64::from(rate), 1.0));
        let samples = (0..(insect.phrase.length * f64::from(rate)) as usize + 200)
            .map(|_| {
                let sample = insect.render(rate as f32);
                insect.phrase.age += 1.0 / f64::from(rate);
                sample
            })
            .collect();
        (insect, samples)
    }

    #[test]
    fn the_bout_envelope_is_a_smooth_bell_from_zero_to_zero() {
        assert_eq!(bout_envelope(0.0), 0.0);
        assert_eq!(bout_envelope(1.0), 0.0);
        assert_eq!(bout_envelope(0.5), 1.0);
        let mut previous = 0.0;
        for step in 1..=500 {
            let u = step as f32 / 1000.0;
            let value = bout_envelope(u);
            assert!((0.0..=1.0).contains(&value));
            assert!(value >= previous, "rises to the middle at {u}");
            // Symmetric: it falls the way it rose.
            assert!((value - bout_envelope(1.0 - u)).abs() < 1e-5);
            assert!(value - previous < 0.02, "no jump at {u}");
            previous = value;
        }
        // Zero slope at the ends, a plateau in the middle, a genuine rise and fall around it.
        assert!(bout_envelope(0.02) < 0.01);
        assert!(bout_envelope(0.98) < 0.01);
        assert!(bout_envelope(0.1) > 0.02 && bout_envelope(0.1) < 0.5);
        assert!(bout_envelope(0.3) < 1.0);
        assert!(bout_envelope(0.36) == 1.0 && bout_envelope(0.64) == 1.0);
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -3.0, 7.0] {
            assert!(bout_envelope(bad).is_finite());
            assert_eq!(bout_envelope(bad), 0.0);
        }
    }

    #[test]
    fn the_registration_has_two_knobs_the_insect_pool_and_the_soft_defaults() {
        let definition = definition();
        assert_eq!(definition.id, "autumn");
        assert_eq!(definition.label, "秋の夜長");
        let ids: Vec<_> = definition.knobs.iter().map(|knob| knob.id).collect();
        assert_eq!(ids, ["insects", "wind"]);
        assert_eq!(definition.knobs[0].initial.get(), 0.55);
        assert_eq!(definition.knobs[1].initial.get(), 0.50);
        assert_eq!(definition.grain_capacity, MAX_INSECTS);
        assert_eq!(definition.visual.shape, VisualShape::SlowGlow);
        assert!(
            AutumnVoice::new(VoicePreparation {
                sample_rate: 48_000,
                seed: 1,
                grain_capacity: MAX_INSECTS + 1,
            })
            .is_err()
        );
        assert!(
            AutumnVoice::new(VoicePreparation {
                sample_rate: 48_000,
                seed: 1,
                grain_capacity: 0,
            })
            .is_err()
        );
    }

    #[test]
    fn every_species_sings_inside_its_band_for_every_insect_and_phrase() {
        let rate = 24_000;
        for (index, species) in SPECIES.into_iter().enumerate() {
            let (low, high) = species.band();
            for seed in 1..=60_u64 {
                let (insect, samples) = rendered_phrase(index, seed, rate);
                let energy: f64 = samples.iter().map(|s| f64::from(*s).powi(2)).sum();
                assert!(energy > 0.0, "{species:?} makes sound");
                let share = band_share(&samples, rate as f32, low, high);
                assert!(
                    share > 0.9,
                    "{species:?} #{index} pitch {} has only {share:.3} in {low}..{high}",
                    insect.pitch
                );
            }
        }
    }

    #[test]
    fn each_species_has_its_own_phrase_shape() {
        let rate = 1_000.0;
        let pulses = |species: Species, count: u32, tempo: f32, ring: f32, length: f64| {
            let phrase = Phrase {
                age: 0.0,
                length,
                pitch: 4_000.0,
                level: 1.0,
                count,
                tempo,
                ring,
            };
            let mut runs = 0;
            let mut inside = false;
            let mut peak = 0.0_f32;
            for tick in 0..(length * rate) as usize + 50 {
                let phrase = Phrase {
                    age: tick as f64 / rate,
                    ..phrase
                };
                let (amplitude, _) = phrase_sound(species, &phrase);
                assert!((0.0..=1.0).contains(&amplitude));
                peak = peak.max(amplitude);
                if amplitude > 0.02 && !inside {
                    runs += 1;
                }
                inside = amplitude > 0.02;
            }
            (runs, peak)
        };
        // A bell rings once, 0.4 to 0.9 s.
        for ring in [0.4, 0.65, 0.9] {
            assert_eq!(
                pulses(Species::Suzumushi, 1, 1.0, ring, f64::from(ring)).0,
                1
            );
        }
        // "コロコロ": a few short pulses.
        for count in 3..=5 {
            let length = f64::from((count - 1) as f32 * CRICKET_SPACING + CRICKET_PULSE);
            assert_eq!(pulses(Species::Cricket, count, 1.0, 0.5, length).0, count);
        }
        // "チン・チロ・リン": three notes at three pitches.
        let length = f64::from(MATSUMUSHI_STARTS[2] + MATSUMUSHI_LENGTHS[2]);
        assert_eq!(pulses(Species::Matsumushi, 1, 1.0, 0.5, length).0, 3);
        let ratios: std::collections::BTreeSet<u32> = MATSUMUSHI_RATIOS
            .iter()
            .map(|ratio| (ratio * 1000.0) as u32)
            .collect();
        assert_eq!(ratios.len(), 3);
        // "ガチャガチャ": one held volley after another (its rattle is the wing-stroke pulse).
        let volleys = 3;
        let length = f64::from((volleys - 1) as f32 * KUTSUWAMUSHI_SPACING + KUTSUWAMUSHI_VOLLEY);
        let fine = |tick: usize| tick as f64 / 10_000.0;
        let mut runs = 0;
        let mut inside = false;
        for tick in 0..(length * 10_000.0) as usize + 100 {
            let phrase = Phrase {
                age: fine(tick),
                length,
                pitch: 2_300.0,
                level: 1.0,
                count: volleys,
                tempo: 1.0,
                ring: 0.5,
            };
            let (amplitude, _) = phrase_sound(Species::Kutsuwamushi, &phrase);
            if amplitude > 0.02 && !inside {
                runs += 1;
            }
            inside = amplitude > 0.02;
        }
        assert_eq!(runs, volleys);
        // A tap is one short ping, of the length it was drawn with.
        for ring in [KANETATAKI_RING_MIN, KANETATAKI_RING_MAX] {
            assert_eq!(
                pulses(Species::Kanetataki, 1, 1.0, ring, f64::from(ring)).0,
                1
            );
        }
    }

    /// An insect of `index` mid-bout at `envelope` with a phrase lasting `seconds`, for measuring
    /// its pulse (the phrase's own shape is not what is measured).
    fn held(index: usize, seed: u64, envelope: f32, seconds: f64) -> Insect {
        let mut insect = plateau_insect(index, seed);
        insect.envelope = 1.0;
        insect.start_phrase(&moment(3.0, 0.001, 1.0));
        insect.phrase.length = seconds;
        insect.phrase.ring = seconds as f32;
        insect.phrase.count = (seconds * 20.0) as u32 + 1;
        insect.envelope = envelope;
        insect
    }

    #[test]
    fn every_species_but_the_kanetataki_pulses_inside_its_range_and_the_kanetataki_does_not() {
        for (index, species) in SPECIES.into_iter().enumerate() {
            for seed in 1..=20_u64 {
                let insect = Insect::new(index, seed);
                let Some(((rate_low, rate_high), (depth_low, depth_high))) = species.pulse() else {
                    assert_eq!(species, Species::Kanetataki);
                    assert_eq!(insect.pulse_base, 0.0);
                    continue;
                };
                assert!(
                    (f64::from(rate_low)..=f64::from(rate_high)).contains(&insect.pulse_base),
                    "{species:?}: {} Hz",
                    insect.pulse_base
                );
                assert!((depth_low..=depth_high).contains(&insect.pulse_depth));
            }
        }
        // The SPEC's ranges, spelled out.
        let range = |species: Species| species.pulse().unwrap();
        assert_eq!(range(Species::Suzumushi), ((50.0, 90.0), (0.30, 0.50)));
        assert!(range(Species::Cricket).0 == (30.0, 50.0) && range(Species::Cricket).1.0 >= 0.8);
        assert!(range(Species::Matsumushi).0.0 * MATSUMUSHI_PULSE_RATES[1] >= 25.0);
        assert!(range(Species::Matsumushi).0.1 * MATSUMUSHI_PULSE_RATES[0] <= 35.0);
        assert_eq!(range(Species::Kutsuwamushi), ((15.0, 30.0), (0.80, 0.95)));
        // Several insects of one species differ from one another.
        let rates: std::collections::BTreeSet<u64> = (1..=20)
            .map(|seed| Insect::new(0, seed).pulse_base.to_bits())
            .collect();
        assert!(rates.len() > 15);
    }

    #[test]
    fn the_pulse_rate_rises_eight_percent_with_the_swell_and_wanders_three_percent() {
        for (index, species) in SPECIES.into_iter().enumerate() {
            if species.pulse().is_none() {
                continue;
            }
            let mut insect = Insect::new(index, 7);
            let rate_at = |insect: &mut Insect, envelope: f32, time: f64| {
                insect.envelope = envelope;
                insect.update_pulse(time);
                insect.pulse_rate
            };
            // The same moment, the swell at its foot and at its peak: 8% either side of the base.
            let (low, middle, high) = (
                rate_at(&mut insect, 0.0, 4.0),
                rate_at(&mut insect, 0.5, 4.0),
                rate_at(&mut insect, 1.0, 4.0),
            );
            assert!((high / middle - 1.08).abs() < 1e-9, "{species:?}");
            assert!((low / middle - 0.92).abs() < 1e-9, "{species:?}");
            assert!((high / low - 1.08 / 0.92).abs() < 1e-9);
            // The wander: within 3% of the base at the middle of the swell, and it does move.
            let swing: Vec<f64> = (0..400)
                .map(|k| rate_at(&mut insect, 0.5, f64::from(k) * 0.5) / insect.pulse_base)
                .collect();
            assert!(
                swing
                    .iter()
                    .all(|r| (0.97 - 1e-9..=1.03 + 1e-9).contains(r))
            );
            let (lo, hi) = swing
                .iter()
                .fold((9.0_f64, 0.0_f64), |(l, h), r| (l.min(*r), h.max(*r)));
            assert!(hi - lo > 0.05, "{species:?}: it wanders {lo}..{hi}");
            // Slowly: within the 3% wander, so at most 6% over ten seconds.
            for pair in swing.windows(21) {
                assert!((pair[20] - pair[0]).abs() <= 0.0601);
            }
        }
    }

    #[test]
    fn a_real_bout_runs_its_pulse_faster_at_the_peak_of_its_swell_than_at_the_foot() {
        let rate = 24_000;
        let knobs = knobs(1.0, 0.0);
        let mut voice = voice(rate, knobs);
        let ring = VisualRing::new(32, 1);
        let (mut foot, mut peak) = (Vec::new(), Vec::new());
        for sample in 0..rate as usize * 200 {
            step(&mut voice, &knobs, &ring, sample);
            for insect in &voice.insects {
                if insect.phrase.sounding() && insect.species.pulse().is_some() {
                    let relative = insect.pulse_rate / insect.pulse_base;
                    assert!((0.92 * 0.97 - 1e-9..=1.08 * 1.03 + 1e-9).contains(&relative));
                    if insect.envelope > 0.99 {
                        peak.push(relative);
                    } else if insect.envelope < 0.15 {
                        foot.push(relative);
                    }
                }
            }
        }
        let mean = |values: &[f64]| values.iter().sum::<f64>() / values.len() as f64;
        assert!(!peak.is_empty() && !foot.is_empty());
        assert!(
            mean(&peak) / mean(&foot) > 1.10,
            "peak {} foot {}",
            mean(&peak),
            mean(&foot)
        );
    }

    /// A held insect rendered flat (the phrase's own shape taken out, so only the carrier and the
    /// wing-stroke pulse are heard): 24 kHz, `seconds` long, at the swell `envelope`, in the grain
    /// `note` (the matsumushi's three notes scale the pulse; the rest pass 0).
    fn flat_run(index: usize, seed: u64, envelope: f32, note: u32, seconds: f64) -> Vec<f32> {
        let rate = 24_000_u32;
        let mut insect = plateau_insect(index, seed);
        insect.set_rate(rate);
        insect.envelope = envelope;
        insect.phrase.level = 1.0;
        insect.phrase.pitch = insect.pitch;
        let (rate_scale, depth_scale) = match (insect.species, note) {
            (Species::Matsumushi, n) => (
                MATSUMUSHI_PULSE_RATES[n as usize],
                MATSUMUSHI_PULSE_DEPTHS[n as usize],
            ),
            _ => (1.0, 1.0),
        };
        (0..(seconds * f64::from(rate)) as usize)
            .map(|tick| {
                insect.update_pulse(3.0 + tick as f64 / f64::from(rate));
                insect.voice_sample(1.0, 1.0, note, rate_scale, depth_scale, rate as f32)
            })
            .collect()
    }

    /// The rectified carrier smoothed over 1 ms and decimated to 2 kHz.
    fn envelope_of(samples: &[f32]) -> Vec<f32> {
        let window = 24;
        let rectified: Vec<f32> = samples.iter().map(|s| s.abs()).collect();
        let envelope: Vec<f32> = rectified
            .windows(window)
            .map(|w| w.iter().sum::<f32>() / window as f32)
            .step_by(12)
            .collect();
        // Skip the first 0.1 s (the pulse's first attack and the smoothing's start).
        envelope[200..].to_vec()
    }

    /// Where the envelope's spectrum peaks between 10 and 150 Hz (0.25 Hz steps).
    fn envelope_peak_hz(envelope: &[f32]) -> f32 {
        let mean = envelope.iter().sum::<f32>() / envelope.len() as f32;
        let centred: Vec<f32> = envelope.iter().map(|v| v - mean).collect();
        let (mut best, mut best_energy) = (0.0_f32, 0.0_f64);
        let mut frequency = 10.0;
        while frequency < 150.0 {
            let energy = energy_at(&centred, frequency, 2_000.0);
            if energy > best_energy {
                (best, best_energy) = (frequency, energy);
            }
            frequency += 0.25;
        }
        best
    }

    /// How deep the envelope swings: 1 - min / max of the smoothed carrier.
    fn envelope_depth(envelope: &[f32]) -> f32 {
        let (low, high) = envelope
            .iter()
            .fold((f32::MAX, 0.0_f32), |(l, h), v| (l.min(*v), h.max(*v)));
        1.0 - low / high
    }

    #[test]
    fn rendered_pulse_rates_peak_inside_each_species_range_and_follow_the_insects_own_base() {
        // (species, the SPEC's range in Hz). Matsumushi has its own test below; ウマオイ has no
        // stated range (its own base range is checked).
        for (species, low, high) in [
            (Species::Suzumushi, 50.0, 90.0),
            (Species::Cricket, 30.0, 50.0),
            (Species::Kutsuwamushi, 15.0, 30.0),
            (Species::Kantan, 35.0, 60.0),
            (Species::Tsuzure, 60.0, 90.0),
            (Species::Aomatsumushi, 40.0, 55.0),
            (Species::Kusahibari, 70.0, 100.0),
            (Species::Umaoi, 30.0, 45.0),
            (Species::Kera, 30.0, 45.0),
        ] {
            for index in slots(species) {
                for seed in 1..=8_u64 {
                    let base = Insect::new(index, seed).pulse_base as f32;
                    let peak = envelope_peak_hz(&envelope_of(&flat_run(index, seed, 0.5, 0, 3.0)));
                    assert!(
                        (low * 0.97..=high * 1.03).contains(&peak),
                        "{species:?} {index} seed {seed}: the envelope peaks at {peak} Hz, outside {low}..{high}"
                    );
                    assert!(
                        (peak - base).abs() < 0.05 * base + 0.4,
                        "{species:?} {index} seed {seed}: peak {peak} Hz vs base {base} Hz"
                    );
                }
            }
        }
        // A matsumushi's three notes pulse at 25 to 35 Hz, each at its own speed.
        for seed in 1..=8_u64 {
            let peaks: Vec<f32> = (0..3)
                .map(|note| envelope_peak_hz(&envelope_of(&flat_run(2, seed, 0.5, note, 3.0))))
                .collect();
            assert!(
                peaks
                    .iter()
                    .all(|p| (25.0 * 0.97..=35.0 * 1.03).contains(p)),
                "seed {seed}: {peaks:?}"
            );
            assert!(
                peaks[0] > peaks[1] * 1.1,
                "the first note is the fastest: {peaks:?}"
            );
            assert!(
                peaks[2] > peaks[1] * 1.05 && peaks[0] > peaks[2] * 1.03,
                "{peaks:?}"
            );
        }
    }

    #[test]
    fn rendered_pulse_depths_are_shallow_for_suzumushi_medium_for_matsumushi_and_deep_for_the_rest()
    {
        for seed in 1..=8_u64 {
            let depth =
                |index, note| envelope_depth(&envelope_of(&flat_run(index, seed, 0.5, note, 2.0)));
            let suzumushi = depth(0, 0);
            assert!((0.28..=0.52).contains(&suzumushi), "suzumushi {suzumushi}");
            let cricket = depth(1, 0);
            assert!(cricket >= 0.78, "cricket {cricket}");
            let kutsuwamushi = depth(slots(Species::Kutsuwamushi)[0], 0);
            assert!(kutsuwamushi >= 0.75, "kutsuwamushi {kutsuwamushi}");
            let notes: Vec<f32> = (0..3).map(|note| depth(2, note)).collect();
            assert!(
                notes.iter().all(|d| (0.3..=0.68).contains(d)),
                "matsumushi {notes:?}"
            );
            assert!(
                notes[1] > notes[0],
                "each note has its own depth: {notes:?}"
            );
            // Sharp: a cricket's pulses are grains, so it is well below its top most of the time.
            let envelope = envelope_of(&flat_run(1, seed, 0.5, 0, 2.0));
            let top = envelope.iter().copied().fold(0.0, f32::max);
            let high = envelope.iter().filter(|v| **v > 0.8 * top).count();
            assert!(
                high * 100 < envelope.len() * 40,
                "cricket {high} of {}",
                envelope.len()
            );
        }
    }

    #[test]
    fn rendered_pulses_run_faster_at_the_top_of_the_swell_within_eight_percent_of_the_base() {
        for (index, species) in SPECIES.into_iter().enumerate() {
            if species.pulse().is_none() {
                continue;
            }
            for seed in [2_u64, 5, 9] {
                let base = Insect::new(index, seed).pulse_base as f32;
                let note_scale = if species == Species::Matsumushi {
                    MATSUMUSHI_PULSE_RATES[0]
                } else {
                    1.0
                };
                let foot = envelope_peak_hz(&envelope_of(&flat_run(index, seed, 0.1, 0, 3.0)));
                let top = envelope_peak_hz(&envelope_of(&flat_run(index, seed, 1.0, 0, 3.0)));
                assert!(
                    top / foot > 1.10 && top / foot < 1.21,
                    "{species:?} seed {seed}: foot {foot} Hz, top {top} Hz"
                );
                // Within 8% (and the 3% wander, and the DFT's resolution) of its own base.
                for peak in [foot, top] {
                    assert!(
                        (peak / (base * note_scale) - 1.0).abs() < 0.08 + 0.03 + 0.02 + 0.016,
                        "{species:?} seed {seed}: {peak} Hz vs base {base} Hz"
                    );
                }
            }
        }
    }

    #[test]
    fn kanetataki_has_no_pulse_and_every_tick_rings_its_own_length() {
        let envelope = envelope_of(&flat_run(3, 5, 0.5, 0, 0.5));
        // A pure carrier: its smoothed rectified envelope is flat (no fast pulse in it).
        let (low, high) = envelope
            .iter()
            .fold((f32::MAX, 0.0_f32), |(l, h), v| (l.min(*v), h.max(*v)));
        assert!((high - low) / high < 0.1, "{low}..{high}");
        let rings: std::collections::BTreeSet<u32> = (0..60)
            .map(|k| {
                let mut insect = plateau_insect(3, 9);
                insect.envelope = 1.0;
                for _ in 0..k {
                    insect.random.unit();
                }
                insect.start_phrase(&moment(2.0, 0.001, 1.0));
                assert!((KANETATAKI_RING_MIN..=KANETATAKI_RING_MAX).contains(&insect.phrase.ring));
                insect.phrase.ring.to_bits()
            })
            .collect();
        assert!(
            rings.len() > 40,
            "a ring of its own each tick: {}",
            rings.len()
        );
    }

    #[test]
    fn matsumushi_notes_differ_in_speed_and_depth_by_their_scales() {
        let mut seen = std::collections::BTreeSet::new();
        for (note, start) in MATSUMUSHI_STARTS.into_iter().enumerate() {
            let (id, speed, depth) = stroke_context(Species::Matsumushi, start + 0.01, 1.0, 0.5);
            assert_eq!(id as usize, note);
            seen.insert(((speed * 1000.0) as u32, (depth * 1000.0) as u32));
        }
        assert_eq!(seen.len(), 3, "a speed and depth of its own for every note");
    }

    #[test]
    fn the_pulse_makes_no_clicks_a_slow_attack_and_a_wobble_under_half_a_percent() {
        // A stroke bites in at least 2.5 ms at the very fastest pulse (90 Hz, 8% warm, 3% wander).
        let fastest_attack = std::hint::black_box(PULSE_ATTACK) / (90.0 * 1.08 * 1.03);
        assert!(fastest_attack >= 0.0025, "{fastest_attack}");
        let mut previous = pulse_shape(0.0);
        assert_eq!(previous, 0.0);
        let mut top = 0.0_f32;
        for step in 1..=10_000 {
            let value = pulse_shape(step as f32 / 10_000.0);
            assert!((0.0..=1.0).contains(&value));
            assert!((value - previous).abs() < 0.002, "a step at {step}");
            top = top.max(value);
            previous = value;
        }
        assert_eq!(
            previous, 0.0,
            "ends where it began: continuous across periods"
        );
        assert!((top - 1.0).abs() < 1e-3);
        // Fast rise, slightly slower fall.
        assert!(pulse_shape(PULSE_ATTACK) == 1.0 && PULSE_ATTACK < 0.5);
        // The pitch wobble: at most 0.5%, suzumushi only.
        for step in 0..=100 {
            let shape = pulse_shape(step as f32 / 100.0);
            let wobble = pitch_wobble(Species::Suzumushi, shape);
            assert!((1.0..=1.005).contains(&wobble));
            for other in [
                Species::Cricket,
                Species::Matsumushi,
                Species::Kutsuwamushi,
                Species::Kanetataki,
            ] {
                assert_eq!(pitch_wobble(other, shape), 1.0);
            }
        }
        // The rendered voices: no sample-to-sample step beyond what the carrier itself makes.
        // Flat (the phrase's shape taken out): beyond the carrier's own slope only the pulse's
        // smooth gain may add to a step, a few per cent of the peak at the very most. A gain
        // jump of a tenth would trip this; the roughness of the kutsuwamushi is noise, allowed
        // more.
        for (index, species) in SPECIES.into_iter().enumerate() {
            for seed in 1..=5_u64 {
                let samples = flat_run(index, seed, 1.0, 0, 1.5);
                let peak = samples.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
                let pitch = Insect::new(index, seed).pitch;
                let partial = if species == Species::Kutsuwamushi {
                    1.08
                } else {
                    1.0
                };
                let slack = if species == Species::Kutsuwamushi {
                    0.3
                } else {
                    0.035
                };
                let carrier = pitch * partial * 1.005;
                let bound = peak * (TAU * carrier / 24_000.0 * 1.02 + slack);
                let step = samples
                    .windows(2)
                    .map(|pair| (pair[1] - pair[0]).abs())
                    .fold(0.0_f32, f32::max);
                assert!(
                    step <= bound,
                    "{species:?} seed {seed}: step {step} over {bound}"
                );
            }
        }
        for rate in [24_000_u32, 48_000] {
            for (index, species) in SPECIES.into_iter().enumerate() {
                for seed in 1..=4_u64 {
                    let mut insect = held(index, seed, 1.0, 0.8);
                    insect.set_rate(rate);
                    let pitch = insect.phrase.pitch;
                    let mut samples = Vec::new();
                    for tick in 0..(0.8 * f64::from(rate)) as usize {
                        insect.update_pulse(3.0 + tick as f64 / f64::from(rate));
                        samples.push(insect.render(rate as f32));
                        insect.phrase.age += 1.0 / f64::from(rate);
                    }
                    let peak = samples.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
                    assert!(peak > 0.0, "{species:?} sounds");
                    // The carrier's own steepest step, with the partial and the pitch ratios.
                    let carrier = (pitch * 1.12 * 1.1).min(rate as f32 * 0.4);
                    let bound = peak * TAU * carrier / rate as f32 * 1.3;
                    let step = samples
                        .windows(2)
                        .map(|pair| (pair[1] - pair[0]).abs())
                        .fold(0.0_f32, f32::max);
                    assert!(
                        step <= bound,
                        "{species:?} {rate}: step {step} over {bound}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_pulsing_voice_keeps_a_phase_that_advances_by_exactly_its_rate() {
        let rate = 48_000_f64;
        let mut insect = held(0, 4, 1.0, 1.0);
        insect.set_rate(48_000);
        let mut previous = None::<f64>;
        let mut checked = 0;
        for tick in 0..24_000 {
            insect.update_pulse(3.0 + tick as f64 / rate);
            insect.render(rate as f32);
            insect.phrase.age += 1.0 / rate;
            if let Some(before) = previous {
                let advance = (insect.pulse_phase - before).rem_euclid(1.0);
                assert!((advance - insect.pulse_rate * insect.call_pulse / rate).abs() < 1e-9);
                checked += 1;
            }
            previous = Some(insect.pulse_phase);
        }
        assert!(checked > 20_000);
    }

    /// The start times and lengths of the phrases a lone insect sings at the bout's peak.
    fn phrases(index: usize, seed: u64, seconds: f64) -> Vec<(f64, f64)> {
        let ring = VisualRing::new(4, 1);
        let delta = 1.0 / 1_000.0;
        let mut insect = plateau_insect(index, seed);
        let mut found = Vec::new();
        for tick in 0..(seconds / delta) as usize {
            let time = tick as f64 * delta;
            let mut events = VisualWriter::new(
                &ring,
                StreamEpoch::new(1).unwrap(),
                SituationIndex::from_index(0, 1).unwrap(),
                WorldSeconds::new(time).unwrap(),
                MachNanos::new((time * 1e9) as u64),
            );
            insect.advance(&moment(time, delta, 1.0), 1, &mut events);
            assert_eq!(insect.envelope, 1.0, "the plateau holds");
            if insect.phrase.sounding() && insect.phrase.age == 0.0 {
                found.push((time, insect.phrase.length));
            }
        }
        found
    }

    #[test]
    fn phrases_repeat_with_a_short_period_that_is_each_insects_own() {
        let mut rates = Vec::new();
        for (index, species) in SPECIES.into_iter().enumerate() {
            let starts = phrases(index, 5, 150.0);
            assert!(starts.len() >= 8, "{species:?} keeps repeating");
            let gaps: Vec<f64> = starts
                .windows(2)
                .map(|pair| pair[1].0 - (pair[0].0 + pair[0].1))
                .collect();
            let (low, high) = species.gap_range();
            let mean = gaps.iter().sum::<f64>() / gaps.len() as f64;
            for gap in &gaps {
                assert!(
                    (f64::from(low) * 0.92..=f64::from(high) * 1.08).contains(gap),
                    "{species:?}: gap {gap} outside {low}..{high}"
                );
                // The insect's own period, not a fresh random one each time.
                assert!(
                    (gap - mean).abs() / mean < 0.12,
                    "{species:?}: gap {gap} drifts from its own mean {mean}"
                );
            }
            if species == Species::Suzumushi {
                for ((_, length), gap) in starts.iter().zip(&gaps) {
                    assert!((0.4..=0.9).contains(length), "ring {length}");
                    assert!(
                        (1.0..=2.0).contains(gap),
                        "SPEC: a 1 to 2 s pause, got {gap}"
                    );
                }
            }
            if species == Species::Kanetataki {
                let periods: Vec<f64> = starts
                    .windows(2)
                    .map(|pair| pair[1].0 - pair[0].0)
                    .collect();
                let (min, max) = periods
                    .iter()
                    .fold((f64::MAX, 0.0_f64), |(a, b), p| (a.min(*p), b.max(*p)));
                assert!(max / min < 1.12, "a steady tap: {min}..{max}");
            }
            rates.push((species, starts.len()));
        }
        let rate_of = |wanted: Species| {
            rates
                .iter()
                .filter(|(species, _)| *species == wanted)
                .map(|(_, count)| *count)
                .max()
                .unwrap()
        };
        for other in [
            Species::Suzumushi,
            Species::Cricket,
            Species::Matsumushi,
            Species::Kanetataki,
        ] {
            assert!(
                rate_of(Species::Kutsuwamushi) < rate_of(other),
                "the rattler calls rarer than {other:?}"
            );
        }
    }

    #[test]
    fn periods_drift_slowly_so_two_insects_slide_in_and_out_of_phase() {
        // Two insects of one species (suzumushi: indices 0, 4, 9) with different seeds-derived
        // periods: the offset between their phrase starts changes over time instead of locking.
        let a = phrases(0, 5, 150.0);
        let b = phrases(4, 5, 150.0);
        let offsets: Vec<f64> = a
            .iter()
            .map(|(start, _)| {
                b.iter()
                    .map(|(other, _)| other - start)
                    .filter(|offset| *offset >= 0.0)
                    .fold(f64::MAX, f64::min)
            })
            .filter(|offset| *offset < 5.0)
            .collect();
        let (min, max) = offsets
            .iter()
            .fold((f64::MAX, 0.0_f64), |(a, b), v| (a.min(*v), b.max(*v)));
        assert!(max - min > 0.3, "the phase moves: {min}..{max}");
    }

    #[test]
    fn a_bout_rises_sings_and_falls_and_its_firefly_matches_the_insect() {
        let rate = 24_000;
        let knobs = knobs(1.0, 0.0);
        let mut voice = voice(rate, knobs);
        let ring = VisualRing::new(64, 1);
        let mut dst = [None; 64];
        let mut observed = 0;
        let mut heights = Vec::new();
        let mut starts = Vec::new();
        let mut lengths = Vec::new();
        let mut previous_envelope = [0.0_f32; MAX_INSECTS];
        for sample in 0..rate as usize * 80 {
            step(&mut voice, &knobs, &ring, sample);
            let when = now(sample, rate);
            for (index, insect) in voice.insects.iter().enumerate() {
                // Audio gain and picture brightness are the same function of the same age.
                let expected = if insect.singing() {
                    bout_envelope((insect.bout_age / insect.bout_length) as f32)
                } else {
                    0.0
                };
                assert_eq!(insect.envelope, expected);
                assert!(
                    (insect.envelope - previous_envelope[index]).abs() < 1e-3,
                    "the swell never steps"
                );
                previous_envelope[index] = insect.envelope;
                if insect.singing() && insect.bout_age == 0.0 {
                    starts.push(sample);
                    lengths.push(insect.bout_length);
                    assert!(
                        (BOUT_MIN_SECONDS..=BOUT_MAX_SECONDS).contains(&insect.bout_length),
                        "a bout is 4 to 12 s, got {}",
                        insect.bout_length
                    );
                    let count = ring.read_live_into(StreamEpoch::new(1).unwrap(), when, &mut dst);
                    let angle = insect.x * FRAC_PI_2;
                    assert_eq!(insect.pan, [angle.cos(), angle.sin()]);
                    let firefly = dst[..count]
                        .iter()
                        .flatten()
                        .find(|event| {
                            event.shape == VisualShape::Firefly
                                && event.present == when
                                && event.x.get() == insect.x
                        })
                        .expect("the bout's start lights a firefly at the insect's own position");
                    assert_eq!(
                        firefly.lifetime,
                        Duration::from_secs_f64(insect.bout_length),
                        "the firefly lives exactly as long as the bout"
                    );
                    assert!(firefly.lifetime >= Duration::from_secs_f64(FIREFLY_MIN_SECONDS));
                    assert!(firefly.large);
                    assert!(
                        (0.10..=0.90).contains(&firefly.y.get()),
                        "anywhere in the window, clear of its edges"
                    );
                    heights.push(firefly.y.get());
                    observed += 1;
                }
            }
            let singing = voice
                .insects
                .iter()
                .filter(|insect| insect.singing())
                .count();
            assert!(singing <= MAX_SINGING, "fireflies stay few: {singing}");
        }
        assert!(observed >= 6, "several bouts in 80 s: {observed}");
        // Over many bouts both halves of the window get their fireflies (at least 35% above the
        // middle), and the whole height is used.
        let upper = heights.iter().filter(|y| **y < 0.5).count();
        assert!(
            upper as f32 >= 0.35 * heights.len() as f32,
            "{upper} of {} in the upper half",
            heights.len()
        );
        assert!(heights.iter().any(|y| *y < 0.2) && heights.iter().any(|y| *y > 0.8));
        assert!(lengths.iter().any(|length| *length < 7.0));
        assert!(lengths.iter().any(|length| *length > 9.0));
        // Insects do not all start together.
        starts.sort_unstable();
        assert!(
            starts
                .windows(2)
                .filter(|pair| pair[1] - pair[0] < 240)
                .count()
                < starts.len() / 2
        );
    }

    #[test]
    fn the_sound_of_a_bout_is_scaled_by_the_same_swell_the_firefly_uses() {
        let rate = 48_000;
        let render = |envelope: f32| {
            let mut insect = plateau_insect(0, 9);
            insect.envelope = 1.0;
            insect.start_phrase(&moment(2.0, 1.0 / 48_000.0, 0.5));
            insect.envelope = envelope;
            (0..12_000)
                .map(|_| {
                    let sample = insect.render(rate as f32);
                    insect.phrase.age += 1.0 / f64::from(rate);
                    sample
                })
                .collect::<Vec<f32>>()
        };
        let full = render(1.0);
        let swell = bout_envelope(0.1);
        let part = render(swell);
        assert!(full.iter().any(|sample| sample.abs() > 0.004));
        for (full, part) in full.iter().zip(&part) {
            assert!((full * swell - part).abs() < 1e-6);
        }
        assert!(render(0.0).iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn a_firefly_event_for_a_hidden_voice_is_never_made_and_phrases_start_after_the_swell_rises() {
        let rate = 24_000;
        let knobs = knobs(1.0, 0.0);
        let mut hidden = voice(rate, knobs);
        hidden.set_contributing(false);
        let ring = VisualRing::new(16, 1);
        for sample in 0..rate as usize * 40 {
            assert_eq!(step(&mut hidden, &knobs, &ring, sample), [0.0; 2]);
            assert!(hidden.insects.iter().all(|insect| !insect.singing()));
        }
        // While audible, no phrase starts before its bout's swell is above the floor.
        let mut audible = voice(rate, knobs);
        let mut phrases_seen = 0;
        for sample in 0..rate as usize * 40 {
            step(&mut audible, &knobs, &ring, sample);
            for insect in &audible.insects {
                if insect.phrase.sounding() && insect.phrase.age == 0.0 {
                    phrases_seen += 1;
                    assert!(insect.envelope >= PHRASE_MIN_ENVELOPE);
                }
            }
        }
        assert!(phrases_seen > 10);
    }

    #[test]
    fn the_only_visual_events_are_fireflies_one_per_bout_and_phrases_emit_nothing() {
        let rate = 24_000;
        let knobs = knobs(1.0, 0.0);
        let mut voice = voice(rate, knobs);
        let ring = VisualRing::new(256, 1);
        let mut dst = [None; 64];
        let (mut bouts, mut phrases) = (0, 0);
        let mut seen = std::collections::BTreeSet::new();
        let last_look = rate as usize * 58;
        for sample in 0..=last_look {
            step(&mut voice, &knobs, &ring, sample);
            bouts += voice
                .insects
                .iter()
                .filter(|insect| insect.singing() && insect.bout_age == 0.0)
                .count();
            phrases += voice
                .insects
                .iter()
                .filter(|insect| insect.phrase.sounding() && insect.phrase.age == 0.0)
                .count();
            // A look every second sees every firefly (each lives 4 s or more).
            if sample % rate as usize == 0 {
                let when = now(sample, rate);
                let live = ring.read_live_into(StreamEpoch::new(1).unwrap(), when, &mut dst);
                for event in dst[..live].iter().flatten() {
                    assert_eq!(
                        event.shape,
                        VisualShape::Firefly,
                        "stars are the background's"
                    );
                    seen.insert(event.serial);
                }
            }
        }
        assert!(
            phrases > bouts && bouts > 3,
            "{phrases} phrases, {bouts} bouts"
        );
        assert_eq!(seen.len(), bouts, "one firefly per bout, nothing else");
    }

    #[test]
    fn every_bout_of_every_species_is_audible_and_sings_at_least_two_phrases() {
        assert!(bout_envelope(AUDIBLE_FROM as f32) >= PHRASE_MIN_ENVELOPE);
        assert!(bout_envelope(AUDIBLE_FROM as f32 - 0.01) < PHRASE_MIN_ENVELOPE);
        let ring = VisualRing::new(8, 1);
        let delta = 0.004;
        let mut counts: Vec<Vec<u32>> = vec![Vec::new(); MAX_INSECTS];
        for seed in 1..=10_u64 {
            for (index, species) in SPECIES.into_iter().enumerate() {
                let mut insect = Insect::new(index, seed);
                let (shortest, longest) = species.bout_range();
                let mut singing = false;
                for tick in 0..(600.0 / delta) as usize {
                    let time = tick as f64 * delta;
                    let mut events = VisualWriter::new(
                        &ring,
                        StreamEpoch::new(1).unwrap(),
                        SituationIndex::from_index(0, 1).unwrap(),
                        WorldSeconds::new(time).unwrap(),
                        MachNanos::new((time * 1e9) as u64),
                    );
                    insect.advance(&moment(time, delta, 1.0), 1, &mut events);
                    if insect.singing() && !singing {
                        assert!(
                            (shortest..=longest).contains(&insect.bout_length),
                            "{species:?} bout of {} s",
                            insect.bout_length
                        );
                    }
                    if singing && !insect.singing() {
                        counts[index].push(insect.phrases_in_bout);
                    }
                    singing = insect.singing();
                }
            }
        }
        for (index, counts) in counts.iter().enumerate() {
            let species = SPECIES[index];
            assert!(counts.len() >= 30, "{species:?}: {} bouts", counts.len());
            let fewest = counts.iter().min().unwrap();
            assert!(
                *fewest >= MIN_PHRASES_PER_BOUT,
                "{species:?} #{index}: a bout of {fewest} phrase(s)"
            );
            // The quick callers repeat many times in a bout; the slow ones are held to two.
            let mean = counts.iter().sum::<u32>() as f64 / counts.len() as f64;
            if matches!(species, Species::Suzumushi | Species::Kanetataki) {
                assert!(mean >= 2.8, "{species:?} #{index}: {mean} phrases a bout");
            }
        }
    }

    #[test]
    fn insects_at_zero_start_nothing_new_but_finish_their_bouts_and_the_wind_carries_on() {
        let rate = 24_000;
        let on = knobs(1.0, 1.0);
        let off = knobs(0.0, 1.0);
        let mut voice = voice(rate, on);
        let ring = VisualRing::new(32, 1);
        let mut sample = 0;
        // Run until several insects are inside a bout and the wind is audible.
        while sample < rate as usize * 300
            && voice
                .insects
                .iter()
                .filter(|insect| insect.singing())
                .count()
                < 2
        {
            step(&mut voice, &on, &ring, sample);
            sample += 1;
        }
        assert!(voice.wind.level > 0.0);
        // Let the knob reach zero (0.2 s), then look.
        for _ in 0..rate as usize {
            step(&mut voice, &off, &ring, sample);
            sample += 1;
        }
        assert_eq!(voice.insects_knob.value(), 0.0);
        let mut was: Vec<bool> = voice.insects.iter().map(Insect::singing).collect();
        let inside = was.clone();
        assert!(
            inside.iter().any(|singing| *singing),
            "a bout is in progress"
        );
        let mut left: Vec<f64> = voice
            .insects
            .iter()
            .map(|insect| insect.bout_length - insect.bout_age)
            .collect();
        let (mut new_bouts, mut early_ends, mut finishing_phrases, mut other_phrases) =
            (0, 0, 0, 0);
        let mut gust_energy = 0.0_f64;
        for _ in 0..rate as usize * 14 {
            let frame = step(&mut voice, &off, &ring, sample);
            sample += 1;
            gust_energy += f64::from(frame[0].powi(2) + frame[1].powi(2));
            for (index, insect) in voice.insects.iter().enumerate() {
                new_bouts += usize::from(!was[index] && insect.singing());
                // A bout ends only when it has run its whole length.
                early_ends += usize::from(was[index] && !insect.singing() && left[index] > 1e-3);
                if insect.phrase.sounding() && insect.phrase.age == 0.0 {
                    if inside[index] {
                        finishing_phrases += 1;
                    } else {
                        other_phrases += 1;
                    }
                }
                was[index] = insect.singing();
                left[index] = insect.bout_length - insect.bout_age;
            }
        }
        assert_eq!(new_bouts, 0, "no new bout at zero");
        assert_eq!(
            other_phrases + finishing_phrases,
            0,
            "no phrase starts at zero, even inside a bout in progress"
        );
        assert_eq!(early_ends, 0, "the bout (and its firefly) lives to its end");
        // Bouts last at most 12 s: after 14 s everything has ended and nothing sounds.
        assert!(voice.insects.iter().all(|insect| !insect.singing()));
        assert!(voice.insects.iter().all(|insect| !insect.phrase.sounding()));
        assert!(gust_energy > 0.0, "the wind carries on");
    }

    #[test]
    fn a_knob_lowered_to_a_non_zero_value_lets_running_bouts_keep_calling_to_their_end() {
        let rate = 24_000;
        let on = knobs(1.0, 0.0);
        let low = knobs(0.05, 0.0);
        let mut voice = voice(rate, on);
        let ring = VisualRing::new(32, 1);
        let mut sample = 0;
        while sample < rate as usize * 300
            && voice.insects.iter().filter(|i| i.singing()).count() < 3
        {
            step(&mut voice, &on, &ring, sample);
            sample += 1;
        }
        for _ in 0..rate as usize {
            step(&mut voice, &low, &ring, sample);
            sample += 1;
        }
        assert!(voice.insects_knob.value() > 0.0);
        let inside: Vec<bool> = voice.insects.iter().map(Insect::singing).collect();
        assert!(inside.iter().any(|b| *b));
        let mut phrases = 0;
        for _ in 0..rate as usize * 14 {
            step(&mut voice, &low, &ring, sample);
            sample += 1;
            for (index, insect) in voice.insects.iter().enumerate() {
                phrases += usize::from(
                    inside[index] && insect.phrase.sounding() && insect.phrase.age == 0.0,
                );
            }
        }
        assert!(phrases > 0, "running bouts keep calling at a low knob");
    }

    #[test]
    fn wind_at_zero_is_silent_and_no_swell_starts_but_a_running_one_ends_and_insects_carry_on() {
        let rate = 24_000;
        let on = knobs(1.0, 1.0);
        let off = knobs(1.0, 0.0);
        // Two identical worlds; one keeps the wind, the other turns it to zero during a swell.
        let mut kept = voice(rate, on);
        let mut zeroed = voice(rate, on);
        let ring = VisualRing::new(32, 1);
        let mut sample = 0;
        while sample < rate as usize * 200 && zeroed.wind.swell_length == 0.0 {
            step(&mut kept, &on, &ring, sample);
            step(&mut zeroed, &on, &ring, sample);
            sample += 1;
        }
        assert!(
            zeroed.wind.swell_length > 0.0,
            "a swell is running at the switch"
        );
        assert!(zeroed.wind.level > 0.0);
        for _ in 0..rate as usize * 2 {
            step(&mut kept, &on, &ring, sample);
            step(&mut zeroed, &off, &ring, sample);
            sample += 1;
        }
        assert_eq!(zeroed.wind_knob.value(), 0.0);
        assert_eq!(zeroed.wind.level, 0.0);
        assert!(zeroed.wind.swell_length > 0.0, "the same swell, now silent");
        let (mut starts_zeroed, mut starts_kept) = (0, 0);
        let (mut was_zeroed, mut was_kept) = (true, kept.wind.swell_length > 0.0);
        let mut frozen_wait = None;
        let mut insect_phrases = 0;
        for _ in 0..rate as usize * 120 {
            step(&mut kept, &on, &ring, sample);
            step(&mut zeroed, &off, &ring, sample);
            sample += 1;
            assert_eq!(zeroed.wind.level, 0.0, "no wind while the knob is 0");
            assert_eq!(zeroed.wind.render(), [0.0; 2]);
            let (now_zeroed, now_kept) =
                (zeroed.wind.swell_length > 0.0, kept.wind.swell_length > 0.0);
            starts_zeroed += usize::from(!was_zeroed && now_zeroed);
            starts_kept += usize::from(!was_kept && now_kept);
            (was_zeroed, was_kept) = (now_zeroed, now_kept);
            if !now_zeroed {
                // Between swells the countdown does not run at zero.
                let wait = *frozen_wait.get_or_insert(zeroed.wind.swell_wait);
                assert_eq!(zeroed.wind.swell_wait, wait);
            }
            insect_phrases += zeroed
                .insects
                .iter()
                .filter(|insect| insect.phrase.sounding() && insect.phrase.age == 0.0)
                .count();
        }
        assert_eq!(starts_zeroed, 0, "no new swell at zero");
        assert!(
            starts_kept >= 1,
            "the same world with the wind on did start one"
        );
        assert!(
            frozen_wait.is_some(),
            "the running swell ended inside the window"
        );
        assert!(insect_phrases > 0, "insects are independent of the wind");
    }

    /// The wind alone, advanced in world time only (no audio), sampled every 10 ms.
    fn wind_series(knob: f32, seconds: usize) -> (Vec<f32>, Vec<f32>, usize) {
        let mut wind = Wind::new(19, 24_000);
        let (mut waves, mut levels, mut swells) = (Vec::new(), Vec::new(), 0);
        let mut was = false;
        for _ in 0..seconds * 100 {
            wind.advance(0.01, knob, true, 24_000);
            waves.push(wind.wave);
            levels.push(wind.level);
            let now = wind.swell_length > 0.0;
            swells += usize::from(now && !was);
            if now {
                assert!((SWELL_MIN_SECONDS..=SWELL_MAX_SECONDS).contains(&wind.swell_length));
            }
            was = now;
        }
        (waves, levels, swells)
    }

    #[test]
    fn the_wind_wave_has_an_8_to_25_second_period_that_drifts_and_never_falls_silent() {
        let (waves, levels, _) = wind_series(0.5, 3_600);
        // Peaks of the wave (local maxima), one every period.
        let peaks: Vec<f64> = (1..waves.len() - 1)
            .filter(|&i| waves[i] > waves[i - 1] && waves[i] >= waves[i + 1] && waves[i] > 0.9)
            .map(|i| i as f64 * 0.01)
            .collect();
        let periods: Vec<f64> = peaks.windows(2).map(|pair| pair[1] - pair[0]).collect();
        assert!(periods.len() > 150, "{} periods an hour", periods.len());
        let (shortest, longest) = periods
            .iter()
            .fold((f64::MAX, 0.0_f64), |(a, b), p| (a.min(*p), b.max(*p)));
        assert!(
            shortest >= 8.0 && longest <= 25.0,
            "{shortest}..{longest} s"
        );
        assert!(
            longest - shortest > 5.0,
            "the period drifts: {shortest}..{longest}"
        );
        // Gradually: neighbouring periods differ by under two seconds.
        assert!(periods.windows(2).all(|p| (p[1] - p[0]).abs() < 2.0));
        // Smooth and continuous: never silent between waves, never a sharp onset.
        let floor = wind_curve(0.5) * WIND_FLOOR;
        assert!(
            levels.iter().all(|l| *l >= floor - 1e-6 && *l <= 1.0),
            "floor {floor}"
        );
        // A swell on top of a wave can rise at 0.22 of full level per second at most: a long
        // swing from the floor to the top, never a sharp onset (checked at knob 1, swells
        // included, as well as at 0.5).
        for knob in [0.5, 1.0] {
            let (_, levels, swells) = wind_series(knob, 3_600);
            assert!(swells > 10);
            let steepest = levels
                .windows(2)
                .map(|pair| (pair[1] - pair[0]).abs() / 0.01)
                .fold(0.0, f32::max);
            assert!(
                steepest < 0.25,
                "knob {knob}: steepest rise or fall {steepest} per second"
            );
        }
    }

    #[test]
    fn the_wind_knob_sets_the_wave_size_and_how_often_a_larger_swell_comes() {
        let (_, soft, soft_swells) = wind_series(0.15, 3_600);
        let (waves, loud, loud_swells) = wind_series(1.0, 3_600);
        assert!(
            loud_swells > soft_swells * 3 / 2,
            "{soft_swells} vs {loud_swells}"
        );
        assert!(
            soft_swells >= 5,
            "even a soft wind swells now and then: {soft_swells}"
        );
        let peak = |levels: &[f32]| levels.iter().copied().fold(0.0, f32::max);
        let low = |levels: &[f32]| levels.iter().copied().fold(f32::MAX, f32::min);
        assert!(
            peak(&loud) > 1.5 * peak(&soft),
            "{} vs {}",
            peak(&soft),
            peak(&loud)
        );
        // The swell adds on top of the wave: the loud wind's peak is above its wave's peak.
        let wave_only = WIND_FLOOR + 0.45 * waves.iter().copied().fold(0.0, f32::max);
        assert!(
            peak(&loud) > wave_only + 0.1,
            "{} vs {wave_only}",
            peak(&loud)
        );
        // The wave undulates more at a higher knob.
        assert!(peak(&loud) - low(&loud) > peak(&soft) - low(&soft));
    }

    /// The wind's loudest second is at least this many decibels below the insects' default RMS.
    const WIND_MARGIN_DB: f64 = 2.0;

    /// The A-weighting of `f` Hz in decibels (the standard analytic curve, 0 dB at 1 kHz).
    fn a_weight_db(f: f64) -> f64 {
        let f2 = f * f;
        let ra = 12194.0_f64.powi(2) * f2 * f2
            / ((f2 + 20.6_f64.powi(2))
                * ((f2 + 107.7_f64.powi(2)) * (f2 + 737.9_f64.powi(2))).sqrt()
                * (f2 + 12194.0_f64.powi(2)));
        20.0 * ra.log10() + 2.0
    }

    /// Per second of a wind-only run (insects at 0): the RMS in dBFS of the output (one channel;
    /// both are alike), unweighted and A-weighted (small speakers lose everything under 200 Hz).
    fn wind_seconds(seed: u64, wind: f32, seconds: usize) -> Vec<(f64, f64)> {
        let rate = 24_000_u32;
        let knobs = knobs(0.0, wind);
        let mut voice = AutumnVoice::new(VoicePreparation {
            sample_rate: rate,
            seed,
            grain_capacity: MAX_INSECTS,
        })
        .unwrap();
        voice.insects_knob = KnobRamp::new(knobs[0]);
        voice.wind_knob = KnobRamp::new(knobs[1]);
        let ring = VisualRing::new(8, 1);
        let (mut out, mut buffer) = (Vec::new(), Vec::new());
        for sample in 0..rate as usize * seconds {
            buffer.push(step(&mut voice, &knobs, &ring, sample)[0]);
            if buffer.len() < rate as usize {
                continue;
            }
            let power =
                buffer.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() / buffer.len() as f64;
            let size = 32_768;
            let mut data = vec![(0.0_f64, 0.0_f64); size];
            for (slot, x) in data.iter_mut().zip(&buffer) {
                slot.0 = f64::from(*x);
            }
            fft(&mut data);
            let weighted: f64 = data
                .iter()
                .enumerate()
                .take(size / 2)
                .skip(1)
                .map(|(index, (re, im))| {
                    let frequency = index as f64 * f64::from(rate) / size as f64;
                    (re * re + im * im) * 10f64.powf(a_weight_db(frequency) / 10.0)
                })
                .sum::<f64>()
                * 2.0
                / (size as f64 * buffer.len() as f64);
            out.push((10.0 * power.log10(), 10.0 * weighted.log10()));
            buffer.clear();
        }
        out
    }

    /// Mean (by power), median and loudest of per-second levels, picked with `pick`.
    fn level_stats(seconds: &[(f64, f64)], pick: fn(&(f64, f64)) -> f64) -> (f64, f64, f64) {
        let mut values: Vec<f64> = seconds.iter().map(pick).collect();
        let mean = 10.0
            * (values.iter().map(|v| 10f64.powf(v / 10.0)).sum::<f64>() / values.len() as f64)
                .log10();
        values.sort_by(f64::total_cmp);
        (mean, values[values.len() / 2], values[values.len() - 1])
    }

    #[test]
    fn the_wind_is_audible_in_the_gaps_by_default_and_its_loudest_second_stays_below_the_insects() {
        // Insects alone at the default knob (whole-run RMS), and the insects' own quiet moments.
        let (insects, _) = levels(24_000, knobs(0.55, 0.0), 120);
        let mut at_one = Vec::new();
        for seed in [3_u64, 19, 71] {
            at_one.extend(wind_seconds(seed, 1.0, 300));
        }
        let (_, _, loudest) = level_stats(&at_one, |x| x.0);
        let (_, _, loudest_weighted) = level_stats(&at_one, |x| x.1);
        // Behind the insects at its very loudest (knob 1, over 15 minutes, three worlds)...
        assert!(
            loudest <= insects - WIND_MARGIN_DB
                && loudest_weighted <= insects - WIND_MARGIN_DB + 1.0,
            "loudest second {loudest:.1} dBFS (A {loudest_weighted:.1}) vs insects {insects:.1}"
        );
        // ...but gently present: not far under it either.
        assert!(loudest > insects - 6.0, "{loudest:.1}");
        // At the default knob its seconds average about -46 dBFS, the level of the insects' quiet
        // moments, so it is there in the gaps between calls.
        let default = KNOBS[1].initial.get();
        let mut at_default = Vec::new();
        for seed in [3_u64, 19, 71] {
            at_default.extend(wind_seconds(seed, default, 300));
        }
        let (mean, median, _) = level_stats(&at_default, |x| x.0);
        let (mean_weighted, _, _) = level_stats(&at_default, |x| x.1);
        assert!(
            (-47.0..=-44.0).contains(&mean) && (-47.0..=-44.0).contains(&median),
            "default wind: mean {mean:.1}, median {median:.1} dBFS"
        );
        assert!(
            (-46.5..=-43.0).contains(&mean_weighted),
            "A-weighted {mean_weighted:.1}"
        );
        // The slider is audible in the middle: a quarter of it is within 6 dB of its top.
        let quarter = wind_seconds(19, 0.25, 120);
        let (quarter_mean, _, _) = level_stats(&quarter, |x| x.0);
        let (top_mean, _, _) = level_stats(&at_one, |x| x.0);
        assert!(
            top_mean - quarter_mean < 7.0 && quarter_mean > -52.0,
            "{quarter_mean:.1}"
        );
        // The default scene is hardly made louder by its wind.
        let (both, _) = levels(24_000, knobs(0.55, default), 120);
        assert!(both - insects < 0.7, "{both:.1} vs {insects:.1}");
    }

    #[test]
    fn the_insects_knob_changes_how_many_sing_and_how_often() {
        let rate = 24_000;
        let census = |insects: f32| {
            let knobs = knobs(insects, 0.0);
            let mut voice = voice(rate, knobs);
            let ring = VisualRing::new(4, 1);
            let (mut bouts, mut singing_samples, mut peak) = (0_usize, 0_usize, 0_usize);
            for sample in 0..rate as usize * 200 {
                step(&mut voice, &knobs, &ring, sample);
                let singing = voice
                    .insects
                    .iter()
                    .filter(|insect| insect.singing())
                    .count();
                bouts += voice
                    .insects
                    .iter()
                    .filter(|insect| insect.singing() && insect.bout_age == 0.0)
                    .count();
                singing_samples += singing;
                peak = peak.max(singing);
            }
            (bouts, singing_samples as f64 / (rate as f64 * 200.0), peak)
        };
        let (few_bouts, few_mean, few_peak) = census(0.2);
        let (many_bouts, many_mean, many_peak) = census(1.0);
        assert!(many_bouts > few_bouts * 2, "{few_bouts} vs {many_bouts}");
        assert!(many_mean > few_mean * 1.8, "{few_mean} vs {many_mean}");
        assert!(few_peak <= 4, "{few_peak}");
        assert!(many_peak <= MAX_SINGING && many_peak > few_peak);
    }

    #[test]
    fn knob_ramps_and_everything_derived_from_them_move_without_steps() {
        let rate = 24_000;
        let low = knobs(0.0, 0.0);
        let high = knobs(1.0, 1.0);
        let mut voice = voice(rate, low);
        let ring = VisualRing::new(4, 1);
        let mut sample = 0;
        for _ in 0..rate as usize * 5 {
            step(&mut voice, &low, &ring, sample);
            sample += 1;
        }
        let per_sample = 1.6 / (0.2 * rate as f32);
        let mut previous = (
            voice.insects_knob.value(),
            voice.wind_knob.value(),
            voice.breath_intensity.get(),
        );
        for round in 0..2 {
            let target = if round == 0 { &high } else { &low };
            for _ in 0..rate as usize * 6 {
                step(&mut voice, target, &ring, sample);
                sample += 1;
                let now = (
                    voice.insects_knob.value(),
                    voice.wind_knob.value(),
                    voice.breath_intensity.get(),
                );
                assert!((now.0 - previous.0).abs() <= per_sample);
                assert!((now.1 - previous.1).abs() <= per_sample);
                assert!(
                    (now.2 - previous.2).abs() < 2e-4,
                    "breath steps by {}",
                    now.2 - previous.2
                );
                previous = now;
            }
        }
    }

    /// Level statistics of a run: (RMS in dBFS over both channels, peak).
    fn levels(rate: u32, knobs: [Normalized; 2], seconds: usize) -> (f64, f32) {
        levels_seeded(19, rate, knobs, seconds)
    }

    fn levels_seeded(seed: u64, rate: u32, knobs: [Normalized; 2], seconds: usize) -> (f64, f32) {
        let mut voice = voice_seeded(seed, rate, knobs);
        let ring = VisualRing::new(8, 1);
        let (mut energy, mut peak) = (0.0_f64, 0.0_f32);
        for sample in 0..rate as usize * seconds {
            let frame = step(&mut voice, &knobs, &ring, sample);
            assert!(frame.iter().all(|value| value.is_finite()));
            energy += f64::from(frame[0]).powi(2) + f64::from(frame[1]).powi(2);
            peak = peak.max(frame[0].abs()).max(frame[1].abs());
        }
        let rms = (energy / (rate as f64 * seconds as f64 * 2.0)).sqrt();
        (20.0 * rms.log10(), peak)
    }

    #[test]
    fn default_loudness_matches_the_other_situations_and_peaks_stay_under_the_knee() {
        // Rain, rock and deep were balanced at about -35 dBFS RMS at 100% app volume
        // (docs/verification.md: raw -40.8/-51.1/-46.6 with trims 2.0/6.0/4.0).
        // Measured with this same method (24 kHz, 90 s, both channels): autumn -34.9 dBFS (P37), rain
        // -35.1 and deep -35.1 at their default knobs.
        // The production seed is the clock, so every seed must be in the window; the loudness
        // of the pool is balanced by design, so the spread between seeds is small.
        let results: Vec<(f64, f32)> = (1..=8_u64)
            .map(|seed| levels_seeded(seed * 7919 + 3, 24_000, knobs(0.55, 0.50), 120))
            .collect();
        for (rms, peak) in &results {
            assert!((-37.0..=-33.0).contains(rms), "default RMS {rms} dBFS");
            assert!(*peak > 0.0 && *peak < 0.5, "default peak {peak}");
        }
        let (low, high) = results.iter().fold((f64::MAX, f64::MIN), |(l, h), (r, _)| {
            (l.min(*r), h.max(*r))
        });
        assert!(
            (high - low) / 2.0 <= 1.5,
            "spread across seeds {low:.1}..{high:.1} dBFS"
        );
    }

    #[test]
    fn every_knob_extreme_is_finite_and_below_the_soft_ceiling() {
        for (insects, wind) in [(1.0, 1.0), (1.0, 0.0), (0.0, 1.0), (0.0, 0.0), (0.02, 0.02)] {
            let (rms, peak) = levels(24_000, knobs(insects, wind), 45);
            assert!(peak < 0.85, "({insects}, {wind}) peaks at {peak}");
            if insects == 0.0 && wind == 0.0 {
                // Only the night bed: very quiet, but there while the situation is heard.
                assert!(peak > 0.0 && peak < 0.01, "bed peak {peak}");
            } else {
                assert!(
                    rms.is_finite() && rms < -25.0,
                    "({insects}, {wind}) RMS {rms}"
                );
            }
        }
        let (_, loud_peak) = levels(24_000, knobs(1.0, 1.0), 45);
        assert!(
            loud_peak < 0.85,
            "even the fullest scene stays soft: {loud_peak}"
        );
    }

    #[test]
    fn supported_rates_stay_finite_with_the_pool_fixed_and_the_callback_heap_free() {
        for rate in [8_000, 22_050, 44_100, 48_000, 96_000, 192_000, 384_000] {
            let knobs = knobs(1.0, 1.0);
            let mut voice = voice(rate, knobs);
            let ring = VisualRing::new(64, 1);
            let pool = voice.insects.as_ptr();
            let seconds = if rate > 100_000 { 4 } else { 20 };
            let (_, counts) = crate::rt_alloc::measure(|| {
                for sample in 0..rate as usize * seconds {
                    let frame = step(&mut voice, &knobs, &ring, sample);
                    assert!(
                        frame
                            .iter()
                            .all(|value| value.is_finite() && value.abs() < 0.85)
                    );
                    assert!(voice.insects.iter().filter(|i| i.singing()).count() <= MAX_SINGING);
                }
                voice.set_contributing(false);
                voice.set_contributing(true);
            });
            assert_eq!(counts, crate::rt_alloc::Counts::default(), "rate {rate}");
            assert_eq!(voice.insects.as_ptr(), pool);
            assert_eq!(voice.insects.len(), MAX_INSECTS);
            assert!(
                voice
                    .insects
                    .iter()
                    .all(|insect| insect.phrase.pitch.is_finite())
            );
        }
    }

    #[test]
    fn a_rate_change_keeps_the_world_and_stays_causal() {
        let knobs = knobs(1.0, 1.0);
        let mut voice = voice(48_000, knobs);
        let ring = VisualRing::new(8, 1);
        for sample in 0..48_000 * 12 {
            step(&mut voice, &knobs, &ring, sample);
        }
        let ages: Vec<f64> = voice.insects.iter().map(|insect| insect.bout_age).collect();
        assert!(ages.iter().any(|age| *age > 0.0));
        assert_eq!(voice.prepare_rate(22_050).unwrap().get(), 0.0);
        assert!(matches!(
            voice.prepare_rate(0),
            Err(VoicePreparationError::InvalidRate)
        ));
        let after: Vec<f64> = voice.insects.iter().map(|insect| insect.bout_age).collect();
        assert_eq!(ages, after);
        for sample in 0..22_050 * 6 {
            let frame = step(&mut voice, &knobs, &ring, sample + 48_000 * 12);
            assert!(
                frame
                    .iter()
                    .all(|value| value.is_finite() && value.abs() < 0.85)
            );
        }
        assert_eq!(voice.prepared_delay().unwrap().get(), 0.0);
    }

    #[test]
    fn leaving_and_returning_drops_a_phrase_and_the_room_but_keeps_the_bout() {
        let rate = 24_000;
        let knobs = knobs(1.0, 0.0);
        let mut voice = voice(rate, knobs);
        let ring = VisualRing::new(8, 1);
        let mut sample = 0;
        // Run until some insect is in the middle of a phrase.
        while !voice
            .insects
            .iter()
            .any(|insect| insect.phrase.sounding() && insect.phrase.age > 0.05)
        {
            step(&mut voice, &knobs, &ring, sample);
            sample += 1;
        }
        let bouts: Vec<f64> = voice.insects.iter().map(|insect| insect.bout_age).collect();
        voice.set_contributing(false);
        assert!(voice.insects.iter().all(|insect| !insect.phrase.sounding()));
        assert_eq!(voice.render_sample(), [0.0; 2]);
        voice.set_contributing(true);
        assert_eq!(
            bouts,
            voice
                .insects
                .iter()
                .map(|insect| insect.bout_age)
                .collect::<Vec<_>>(),
            "the world does not rewind"
        );
        // The room tail and the insects' delay lines are gone: with no phrase sounding the first
        // samples are only the night bed (a few thousandths at most; the old tail was far more).
        let mut quiet = 0.0_f32;
        let mut checked = 0;
        for _ in 0..2_000 {
            if voice.insects.iter().any(|insect| insect.phrase.sounding()) {
                break;
            }
            let frame = voice.render_sample();
            quiet = quiet.max(frame[0].abs()).max(frame[1].abs());
            checked += 1;
        }
        assert!(checked > 100);
        assert!(quiet < 0.01, "only the bed remains: {quiet}");
    }

    // ---- P38: distance, individuals, call variation, the night field and the air ----

    const RATE48: u32 = 48_000;

    /// The response of insect 0's space at distance `step` to a unit impulse.
    fn space_response(step: usize) -> Vec<f32> {
        let mut insect = Insect::new(0, 1);
        insect.set_rate(RATE48);
        insect.set_distance(step, RATE48);
        (0..4_096)
            .map(|n| insect.space(if n == 0 { 1.0 } else { 0.0 }))
            .collect()
    }

    /// The energy-weighted mean frequency of `samples` (Hz).
    fn centroid(samples: &[f32], rate: u32) -> f64 {
        let size = samples.len().next_power_of_two();
        let mut data = vec![(0.0_f64, 0.0_f64); size];
        for (slot, x) in data.iter_mut().zip(samples) {
            slot.0 = f64::from(*x);
        }
        fft(&mut data);
        let (mut weighted, mut total) = (0.0, 0.0);
        for (index, (re, im)) in data.iter().enumerate().take(size / 2) {
            let energy = re * re + im * im;
            weighted += energy * index as f64 * f64::from(rate) / size as f64;
            total += energy;
        }
        weighted / total
    }

    #[test]
    fn farther_insects_are_quieter_duller_and_later_by_the_inverse_distance_law() {
        let responses: Vec<Vec<f32>> = (0..4).map(space_response).collect();
        let delay_ms = |response: &[f32]| {
            let peak = response.iter().fold(0.0_f32, |m, x| m.max(x.abs()));
            let first = response.iter().position(|x| x.abs() > 0.5 * peak).unwrap();
            first as f64 / f64::from(RATE48) * 1_000.0
        };
        let mut previous_delay = -1.0_f64;
        for (step, response) in responses.iter().enumerate() {
            // Level: the direct sound follows 2 m / distance (the response's DC gain).
            let sum: f64 = response.iter().map(|x| f64::from(*x)).sum();
            let expected = f64::from(DISTANCE_METERS[0] / DISTANCE_METERS[step]);
            assert!(
                (sum - expected).abs() < 1e-3,
                "step {step}: DC gain {sum} vs {expected}"
            );
            // Delay: sound at 343 m/s, from a few ms to under 30 ms, later with distance.
            let delay = delay_ms(response);
            let travel = f64::from(DISTANCE_METERS[step] / SOUND_SPEED) * 1_000.0;
            assert!(
                (delay - travel).abs() < 1.0,
                "step {step}: {delay} ms vs {travel}"
            );
            assert!((3.0..=30.0).contains(&delay));
            assert!(delay > previous_delay, "later with distance");
            previous_delay = delay;
        }
        let gains: Vec<f64> = responses
            .iter()
            .map(|r| r.iter().map(|x| f64::from(*x)).sum())
            .collect();
        assert!(
            gains.windows(2).all(|g| g[1] < g[0]),
            "quieter with distance: {gains:?}"
        );
        // Spectral centroid and 5 kHz loss: strictly falling over the four distances.
        let centres: Vec<f64> = responses.iter().map(|r| centroid(r, RATE48)).collect();
        assert!(centres.windows(2).all(|c| c[1] < c[0] * 0.9), "{centres:?}");
        let loss: Vec<f64> = responses
            .iter()
            .map(|r| {
                let dc: f64 = r.iter().map(|x| f64::from(*x)).sum();
                energy_at(r, 5_000.0, RATE48 as f32).sqrt() / dc
            })
            .collect();
        assert!(loss[0] > 0.97, "the nearest loses nothing: {}", loss[0]);
        assert!(loss.windows(2).all(|l| l[1] < l[0]), "{loss:?}");
        assert!(loss[3] < 0.75, "the farthest is dull at 5 kHz: {}", loss[3]);
    }

    #[test]
    fn a_far_phrase_is_quieter_with_more_of_it_sent_to_the_room_and_the_wet_share_rises() {
        let mut energies = Vec::new();
        let mut ratios = Vec::new();
        for step in 0..4 {
            let mut insect = held(0, 3, 1.0, 0.6);
            insect.set_rate(RATE48);
            insect.set_distance(step, RATE48);
            let mut room = Room::new(RATE48).unwrap();
            let (mut dry_energy, mut wet_energy) = (0.0_f64, 0.0_f64);
            for tick in 0..(2.0 * f64::from(RATE48)) as usize {
                insect.update_pulse(3.0 + tick as f64 / f64::from(RATE48));
                let dry = insect.render(RATE48 as f32);
                insect.phrase.age += 1.0 / f64::from(RATE48);
                let wet = room.tick([dry * insect.send, dry * insect.send]);
                dry_energy += f64::from(dry).powi(2);
                wet_energy += f64::from(wet[0]).powi(2) + f64::from(wet[1]).powi(2);
            }
            energies.push(dry_energy);
            ratios.push(wet_energy / 2.0 / dry_energy);
        }
        assert!(energies.windows(2).all(|e| e[1] < e[0]), "{energies:?}");
        assert!(ratios.windows(2).all(|r| r[1] > r[0]), "wet/dry {ratios:?}");
        assert!(ratios[3] > 4.0 * ratios[0], "{ratios:?}");
        // Modest even for the farthest: the room never exceeds the direct sound.
        assert!(ratios[3] < 1.0, "{ratios:?}");
        let sends: Vec<f32> = (0..4)
            .map(|step| {
                let mut insect = Insect::new(0, 1);
                insect.set_distance(step, RATE48);
                insect.send
            })
            .collect();
        assert_eq!(sends, DISTANCE_WET.to_vec());
    }

    #[test]
    fn every_insect_sits_at_a_distance_from_its_own_stream_and_the_pan_and_firefly_x_are_unchanged()
    {
        let mut seen = [0; 4];
        for seed in 1..=40_u64 {
            for index in 0..MAX_INSECTS {
                let insect = Insect::new(index, seed);
                seen[insect.distance_step] += 1;
                // Deterministic.
                assert_eq!(Insect::new(index, seed).distance_step, insect.distance_step);
                // x and the pan still come from the main stream's second draw, as before.
                let mut main = WorldRandom::new(splitmix(
                    seed ^ (index as u64 + 1).wrapping_mul(0x2545_f491_4f6c_dd1d),
                ));
                main.unit();
                let spread = (index as f32 * 0.618_034 + 0.12 * main.unit() as f32).fract();
                assert_eq!(insect.x, 0.08 + 0.84 * spread);
                let angle = insect.x * FRAC_PI_2;
                assert_eq!(insect.pan, [angle.cos(), angle.sin()]);
            }
        }
        assert!(
            seen.iter().all(|n| *n >= 40),
            "every distance is used: {seen:?}"
        );
    }

    #[test]
    fn same_species_individuals_are_detuned_by_a_few_to_a_few_tens_of_hertz() {
        let (mut up, mut down) = (0, 0);
        for seed in 1..=40_u64 {
            for (index, species) in SPECIES.into_iter().enumerate() {
                let offset = Insect::new(index, seed).pitch - species.pitch_center();
                assert!(
                    (DETUNE_MIN_HZ - 1e-3..=DETUNE_MAX_HZ + 1e-3).contains(&offset.abs()),
                    "{species:?}: {offset} Hz"
                );
                if offset > 0.0 {
                    up += 1;
                } else {
                    down += 1;
                }
            }
        }
        assert!(up > 100 && down > 100, "{up} up, {down} down");
    }

    /// Two suzumushi (pool slots 0 and 4, both singing at the default knob) each start a real call
    /// (`start_phrase`, with its jitter and drift) and sing it together for two seconds through
    /// `raw_render`, the pulse and the pitch wobble included. Returns the two phrases' pitches and
    /// the three signals (one alone, the other alone, both).
    fn duet(seed: u64) -> (f32, f32, Vec<f32>, Vec<f32>, Vec<f32>) {
        let rate = 24_000_u32;
        let sing = |index: usize| {
            let mut insect = plateau_insect(index, seed);
            insect.set_rate(rate);
            insect.envelope = 1.0;
            insect.start_phrase(&moment(3.0, 0.001, 0.55));
            insect.phrase.length = 2.0;
            insect.phrase.ring = 2.0;
            let pitch = insect.phrase.pitch;
            let samples: Vec<f32> = (0..2 * rate as usize)
                .map(|tick| {
                    insect.update_pulse(3.0 + tick as f64 / f64::from(rate));
                    let sample = insect.raw_render(rate as f32);
                    insect.phrase.age += 1.0 / f64::from(rate);
                    sample
                })
                .collect();
            (pitch, samples)
        };
        let ((pa, a), (pb, b)) = (sing(0), sing(4));
        let both = a.iter().zip(&b).map(|(x, y)| x + y).collect();
        (pa, pb, a, b, both)
    }

    #[test]
    fn two_suzumushi_calling_together_beat_at_the_difference_of_their_pitches() {
        let at = |signal: &[f32], beat: f32| -> f64 {
            let envelope = envelope_of(signal);
            let mean = envelope.iter().sum::<f32>() / envelope.len() as f32;
            let centred: Vec<f32> = envelope.iter().map(|v| v - mean).collect();
            let mut best = 0.0_f64;
            let mut frequency = beat - 2.0;
            while frequency <= beat + 2.0 {
                best = best.max(energy_at(&centred, frequency, 2_000.0));
                frequency += 0.25;
            }
            best
        };
        let mut checked = 0;
        for seed in 1..=300_u64 {
            let (pa, pb, a, b, both) = duet(seed);
            let beat = (pa - pb).abs();
            if !(14.0..=45.0).contains(&beat) {
                continue;
            }
            let (alone_a, alone_b, together) = (at(&a, beat), at(&b, beat), at(&both, beat));
            assert!(
                together > 4.0 * alone_a.max(alone_b),
                "seed {seed}: beat {beat} Hz: together {together:.3e}, alone {alone_a:.3e} / {alone_b:.3e}"
            );
            checked += 1;
            if checked == 8 {
                break;
            }
        }
        assert_eq!(checked, 8, "enough seeds with a clear beat");
    }

    #[test]
    fn same_species_pairs_stay_a_few_tens_of_hertz_apart_through_every_call_and_drift() {
        // Within a species: detune up to 30 Hz either way, the call 4 Hz and the drift 4 Hz.
        let limit = 2.0 * (DETUNE_MAX_HZ + CALL_PITCH_HZ + PITCH_DRIFT_HZ) + 0.01;
        let mut widest = 0.0_f32;
        for seed in 1..=40_u64 {
            for species in ALL_SPECIES {
                let indices = slots(species);
                if indices.len() < 2 {
                    continue;
                }
                for call in 0..40 {
                    let pitches: Vec<f32> = indices
                        .iter()
                        .map(|index| {
                            let mut insect = plateau_insect(*index, seed);
                            insect.envelope = 1.0;
                            insect.start_phrase(&moment(5.0 + f64::from(call) * 7.3, 0.001, 0.55));
                            insect.phrase.pitch
                        })
                        .collect();
                    for (i, a) in pitches.iter().enumerate() {
                        for b in &pitches[i + 1..] {
                            widest = widest.max((a - b).abs());
                        }
                    }
                }
            }
        }
        assert!(widest <= limit, "pair difference {widest} Hz");
        assert!(
            widest < 80.0 && widest > 30.0,
            "a few tens of hertz: {widest}"
        );
        const { assert!(CALL_PITCH_HZ + PITCH_DRIFT_HZ <= 8.0) };
    }

    #[test]
    fn each_call_varies_by_a_small_fixed_amount_in_loudness_pitch_and_pulse_interval() {
        for (index, species) in SPECIES.into_iter().enumerate() {
            let mut insect = plateau_insect(index, 11);
            insect.envelope = 1.0;
            let (mut gains, mut pitches, mut pulses) = (Vec::new(), Vec::new(), Vec::new());
            for call in 0..600 {
                let m = moment(10.0 + f64::from(call) * 3.7, 0.001, 0.8);
                insect.start_phrase(&m);
                let nominal = insect.species.level() * insect.loudness * (0.65 + 0.35 * 0.8);
                gains.push(20.0 * (insect.phrase.level / nominal).log10());
                pitches.push(insect.phrase.pitch - insect.pitch);
                pulses.push(insect.call_pulse);
            }
            let span = |v: &[f32]| v.iter().fold(0.0_f32, |m, x| m.max(x.abs()));
            assert!(
                span(&gains) <= CALL_GAIN_DB + 1e-3 && span(&gains) > 1.2,
                "{species:?} gain {}",
                span(&gains)
            );
            // Pitch: the call's own 4 Hz plus the slow 4 Hz drift, never more than 8 Hz off.
            assert!(
                span(&pitches) <= CALL_PITCH_HZ + PITCH_DRIFT_HZ + 1e-3 && span(&pitches) > 5.0
            );
            assert!(
                pulses
                    .iter()
                    .all(|p| (1.0 - CALL_PULSE..=1.0 + CALL_PULSE).contains(p)),
                "{species:?}"
            );
            let (low, high) = pulses
                .iter()
                .fold((9.0_f64, 0.0_f64), |(l, h), p| (l.min(*p), h.max(*p)));
            assert!(low < 0.96 && high > 1.04, "{species:?}: {low}..{high}");
            // Many different calls, not a few repeated ones.
            let distinct: std::collections::BTreeSet<u32> =
                gains.iter().map(|g| g.to_bits()).collect();
            assert!(distinct.len() > 590);
        }
    }

    #[test]
    fn call_variation_comes_from_its_own_stream_and_leaves_the_bout_and_phrase_timing_alone() {
        // Two insects of the same seed differ only in how many call-variation draws were taken:
        // their bout and gap timing (the main stream) is identical.
        let ring = VisualRing::new(8, 1);
        let run = |extra_draws: usize| {
            let mut insect = plateau_insect(0, 9);
            for _ in 0..extra_draws {
                insect.call_random.unit();
            }
            let mut starts = Vec::new();
            for tick in 0..60_000 {
                let time = tick as f64 * 0.002;
                let mut events = VisualWriter::new(
                    &ring,
                    StreamEpoch::new(1).unwrap(),
                    SituationIndex::from_index(0, 1).unwrap(),
                    WorldSeconds::new(time).unwrap(),
                    MachNanos::new((time * 1e9) as u64),
                );
                insect.advance(&moment(time, 0.002, 1.0), 1, &mut events);
                if insect.phrase.sounding() && insect.phrase.age == 0.0 {
                    starts.push(tick);
                }
            }
            starts
        };
        assert_eq!(run(0), run(17));
    }

    #[test]
    fn the_room_has_eight_falling_panned_reflections_and_a_dark_tail_of_one_to_two_seconds() {
        let rate = 24_000_u32;
        let room = Room::new(rate).unwrap();
        // Reflections: eight, from 20 to 145 ms, later is quieter, spread across the field.
        let times: Vec<f32> = room
            .taps
            .iter()
            .map(|tap| tap.0 as f32 / rate as f32 * 1_000.0)
            .collect();
        assert_eq!(times.len(), 8);
        assert!(times[0] >= 20.0 && times[7] <= 145.0 && times.windows(2).all(|t| t[1] > t[0]));
        let level = |tap: &(usize, f32, f32)| tap.1.hypot(tap.2);
        assert!(room.taps.windows(2).all(|t| level(&t[1]) < level(&t[0])));
        assert!(room.taps.iter().any(|t| t.1 > t.2) && room.taps.iter().any(|t| t.2 > t.1));
        // Impulse response: nothing before the first reflection; a tail with an RT60 of 1 to 2 s.
        let mut room = Room::new(rate).unwrap();
        let response: Vec<[f32; 2]> = (0..rate as usize * 6)
            .map(|n| room.tick(if n == 0 { [1.0, 1.0] } else { [0.0; 2] }))
            .collect();
        let first = response
            .iter()
            .position(|f| f[0] != 0.0 || f[1] != 0.0)
            .unwrap();
        assert!(
            first as f32 / rate as f32 * 1_000.0 >= 22.5,
            "first sound at {first}"
        );
        let energy: Vec<f64> = response
            .iter()
            .map(|f| f64::from(f[0]).powi(2) + f64::from(f[1]).powi(2))
            .collect();
        let start = (0.15 * f64::from(rate)) as usize;
        let mut decay = vec![0.0_f64; energy.len()];
        let mut running = 0.0;
        for index in (start..energy.len()).rev() {
            running += energy[index];
            decay[index] = running;
        }
        let at = |db: f64| {
            (start..energy.len())
                .find(|&i| 10.0 * (decay[i] / decay[start]).log10() < db)
                .unwrap()
        };
        // Schroeder backward integration, a 20 dB stretch scaled to 60 dB.
        let rt60 = (at(-25.0) - at(-5.0)) as f64 / f64::from(rate) * 3.0;
        assert!((1.1..=2.0).contains(&rt60), "RT60 {rt60:.2} s");
        // The tail is darker than the sound that went in (a flat impulse): its energy above 4 kHz
        // against 100 Hz to 1.5 kHz is a fraction of the impulse's.
        let tail: Vec<f32> = response[start..].iter().map(|f| f[0]).collect();
        let size = tail.len().next_power_of_two();
        let mut data = vec![(0.0_f64, 0.0_f64); size];
        for (slot, x) in data.iter_mut().zip(&tail) {
            slot.0 = f64::from(*x);
        }
        fft(&mut data);
        let band = |low: f64, high: f64| -> f64 {
            data.iter()
                .enumerate()
                .take(size / 2)
                .filter(|(i, _)| {
                    let f = *i as f64 * f64::from(rate) / size as f64;
                    (low..high).contains(&f)
                })
                .map(|(_, (re, im))| re * re + im * im)
                .sum()
        };
        let tail_ratio = band(4_000.0, 10_000.0) / band(100.0, 1_500.0);
        let flat_ratio = 6_000.0 / 1_400.0;
        assert!(
            tail_ratio < 0.25 * flat_ratio,
            "tail {tail_ratio} vs flat {flat_ratio}"
        );
        // Restartable: after invalidation nothing old can be heard.
        room.tick([1.0, 1.0]);
        room.invalidate();
        for _ in 0..rate as usize * 6 {
            assert_eq!(room.tick([0.0; 2]), [0.0; 2]);
        }
    }

    #[test]
    fn the_room_adds_a_modest_wet_share_that_does_not_wash_out_the_rhythm() {
        // A short 3.75 kHz call (50 ms) into the room: its total wet energy is a fraction of the
        // call's own, so the room is clearly below the direct sound.
        let rate = 24_000_u32;
        let mut room = Room::new(rate).unwrap();
        let (mut call, mut wet) = (0.0_f64, Vec::new());
        for n in 0..rate as usize * 3 {
            let input = if n < 1_200 {
                0.5 * (TAU * 3_750.0 * n as f32 / rate as f32).sin()
            } else {
                0.0
            };
            call += 2.0 * f64::from(input).powi(2);
            let out = room.tick([input, input]);
            wet.push(f64::from(out[0]).powi(2) + f64::from(out[1]).powi(2));
        }
        let total: f64 = wet.iter().sum();
        assert!(
            total < 0.8 * call,
            "the room's total energy {total} vs the call's {call}"
        );
        // One second later the tail has fallen well below its early level.
        let early: f64 = wet[..rate as usize / 2].iter().sum::<f64>() / (rate as f64 / 2.0);
        let later: f64 = wet[rate as usize..rate as usize + 4_800]
            .iter()
            .sum::<f64>()
            / 4_800.0;
        assert!(later < early * 0.2, "{later} vs {early}");
    }

    #[test]
    fn the_night_bed_is_very_quiet_follows_the_situation_and_has_no_clicks() {
        // Alone (no insects, no wind) it sits about -60 dBFS, far under the insects' -35.
        let (bed, peak) = levels(24_000, knobs(0.0, 0.0), 120);
        let (insects, _) = levels(24_000, knobs(0.55, 0.0), 60);
        assert!((-64.0..=-56.0).contains(&bed), "bed {bed:.1} dBFS");
        assert!(bed <= insects - 20.0, "{bed:.1} vs insects {insects:.1}");
        assert!(peak < 0.01);
        // It follows the situation, not the wind knob: with the wind at zero it is there; hidden,
        // it is exactly silent; coming back it starts quietly (the engine's gain ramp does the
        // fade), with no step.
        let ring = VisualRing::new(8, 1);
        let k = knobs(0.0, 0.0);
        let mut voice = voice(24_000, k);
        for sample in 0..24_000 {
            step(&mut voice, &k, &ring, sample);
        }
        voice.set_contributing(false);
        for sample in 24_000..24_500 {
            assert_eq!(step(&mut voice, &k, &ring, sample), [0.0; 2]);
        }
        voice.set_contributing(true);
        let mut previous = [0.0_f32; 2];
        let mut biggest_step = 0.0_f32;
        for sample in 24_500..(24_500 + 24_000) {
            let frame = step(&mut voice, &k, &ring, sample);
            biggest_step = biggest_step
                .max((frame[0] - previous[0]).abs())
                .max((frame[1] - previous[1]).abs());
            previous = frame;
        }
        assert!(biggest_step < 0.008, "step {biggest_step}");
        // Its level breathes by a quarter, slowly, in world time.
        let mut bed = NightBed::new(5);
        let levels: Vec<f32> = (0..4_100)
            .map(|k| {
                bed.advance(f64::from(k) * 0.1);
                bed.level
            })
            .collect();
        let (low, high) = levels
            .iter()
            .fold((f32::MAX, 0.0_f32), |(l, h), v| (l.min(*v), h.max(*v)));
        assert!(
            (low - 0.75 * BED_LEVEL).abs() < 0.01 * BED_LEVEL
                && (high - 1.25 * BED_LEVEL).abs() < 0.01 * BED_LEVEL
        );
        assert!(
            levels
                .windows(2)
                .all(|l| (l[1] - l[0]).abs() < 0.005 * BED_LEVEL)
        );
    }

    #[test]
    fn delay_lines_hold_exactly_what_was_written_and_forget_on_invalidation_in_constant_time() {
        let mut line = DelayLine::new(8);
        assert_eq!(line.tap(0), 0.0);
        for value in 1..=12 {
            line.push(value as f32);
        }
        assert_eq!(line.tap(0), 12.0);
        assert_eq!(line.tap(3), 9.0);
        assert_eq!(line.tap(7), 5.0);
        assert_eq!(line.tap(8), 0.0, "past the line's length");
        line.invalidate();
        for delay in 0..8 {
            assert_eq!(line.tap(delay), 0.0);
        }
        line.push(7.0);
        assert_eq!(line.tap(0), 7.0);
        assert_eq!(line.tap(1), 0.0, "nothing from before the invalidation");
    }

    #[test]
    fn low_sample_rates_keep_the_whole_voice_and_the_bed_finite_and_stable() {
        for rate in [8_000_u32, 11_025] {
            let k = knobs(0.55, 0.50);
            let mut voice = voice_seeded(5, rate, k);
            let ring = VisualRing::new(8, 1);
            let mut peak = 0.0_f32;
            for sample in 0..rate as usize * 20 {
                let frame = step(&mut voice, &k, &ring, sample);
                assert!(frame.iter().all(|v| v.is_finite() && v.abs() < 0.85));
                peak = peak.max(frame[0].abs());
            }
            assert!(peak > 0.0);
            // The bed alone is finite and tiny (its filter has no per-tick cutoff input: the
            // cutoff is clamped once at preparation).
            let mut probe = voice_seeded(7, rate, knobs(0.0, 0.0));
            probe.bed.level = 1.0;
            let bed: Vec<f32> = (0..rate as usize).map(|_| probe.bed.render()[0]).collect();
            assert!(bed.iter().all(|v| v.is_finite() && v.abs() < 5.0));
        }
    }

    // ---- P39: twenty-four voices, eleven species ----

    #[test]
    fn the_pool_holds_twenty_four_voices_of_eleven_species_in_a_fixed_mix_that_every_prefix_shares()
    {
        assert_eq!((MAX_INSECTS, MAX_SINGING), (24, 16));
        assert_eq!(SPECIES.len(), 24);
        assert_eq!(DISTANCE_TABLE.len(), 24);
        assert_eq!(definition().grain_capacity, 24);
        let counts: Vec<(Species, usize)> = ALL_SPECIES
            .into_iter()
            .map(|species| (species, slots(species).len()))
            .collect();
        assert_eq!(counts.iter().map(|c| c.1).sum::<usize>(), 24);
        // Suzumushi, cricket and matsumushi are the many; every other species has one or two.
        for (species, count) in &counts {
            match species {
                Species::Suzumushi => assert_eq!(*count, 5),
                Species::Cricket | Species::Matsumushi => assert_eq!(*count, 4),
                _ => assert!((1..=2).contains(count), "{species:?}: {count}"),
            }
        }
        let distinct = |k: usize| {
            ALL_SPECIES
                .into_iter()
                .filter(|species| SPECIES[..k].contains(species))
                .count()
        };
        // Any prefix (any setting of the knob) already has variety; the default's 14 has all 11.
        for k in 4..=24 {
            assert!(distinct(k) + 3 >= k.min(11), "{k}: {}", distinct(k));
        }
        assert_eq!(distinct(14), 11);
        // Every prefix mixes near and far singers.
        for k in 4..=24 {
            let near = DISTANCE_TABLE[..k].iter().filter(|d| **d <= 1).count();
            let far = DISTANCE_TABLE[..k].iter().filter(|d| **d >= 2).count();
            assert!(
                near >= 1 && far >= 1 && near * 4 >= k && far * 4 >= k,
                "{k}: {near}/{far}"
            );
        }
        for step in 0..4 {
            assert!(DISTANCE_TABLE.contains(&step));
        }
        // The seed only trades the first two suzumushi (slots 0 and 4, always enabled together).
        for seed in 1..=60_u64 {
            for (index, entry) in DISTANCE_TABLE.iter().enumerate() {
                let step = distance_for(index, seed);
                if index != 0 && index != 4 {
                    assert_eq!(step, *entry, "slot {index}");
                } else {
                    assert!(step == DISTANCE_TABLE[0] || step == DISTANCE_TABLE[4]);
                }
            }
            assert_eq!(
                distance_for(0, seed) + distance_for(4, seed),
                DISTANCE_TABLE[0] + DISTANCE_TABLE[4]
            );
        }
    }

    #[test]
    fn the_enabled_count_and_the_singing_cap_rise_progressively_with_the_knob() {
        let enabled = |knob: f32| {
            (0..MAX_INSECTS)
                .filter(|index| Insect::new(*index, 1).enabled(&moment(0.0, 0.001, knob)))
                .count()
        };
        assert_eq!((enabled(0.0), singing_cap(0.0)), (0, 0));
        assert_eq!((enabled(1.0), singing_cap(1.0)), (24, 16));
        // The default: 14 voices may sing, up to 9 at once.
        assert_eq!((enabled(0.55), singing_cap(0.55)), (14, 9));
        for step in 0..10 {
            let (low, high) = (step as f32 / 10.0, (step + 1) as f32 / 10.0);
            assert!(enabled(high) > enabled(low), "{low}..{high}");
            assert!(singing_cap(high) > singing_cap(low), "{low}..{high}");
        }
        // The first voice to sing is the first of the table, then in order.
        for knob in [0.1, 0.3, 0.7] {
            let on: Vec<bool> = (0..MAX_INSECTS)
                .map(|index| Insect::new(index, 1).enabled(&moment(0.0, 0.001, knob)))
                .collect();
            assert!(
                on.windows(2).all(|pair| pair[0] || !pair[1]),
                "a prefix is on"
            );
        }
    }

    /// The census of a world run (no audio) at `knob`: (mean number singing, most at once, bouts
    /// begun), over `seconds` in steps of 10 ms.
    fn census(seed: u64, knob: f32, seconds: f64) -> (f64, usize, usize) {
        let knobs = knobs(knob, 0.0);
        let mut voice = voice_seeded(seed, 24_000, knobs);
        let ring = VisualRing::new(256, 1);
        let (mut total, mut peak, mut bouts, mut steps) = (0_usize, 0_usize, 0_usize, 0_usize);
        let delta = 0.01;
        for tick in 0..(seconds / delta) as usize {
            advance(&mut voice, &knobs, &ring, tick as f64 * delta, delta);
            let singing = voice.insects.iter().filter(|i| i.singing()).count();
            bouts += voice
                .insects
                .iter()
                .filter(|i| i.singing() && i.bout_age == 0.0)
                .count();
            total += singing;
            peak = peak.max(singing);
            steps += 1;
        }
        (total as f64 / steps as f64, peak, bouts)
    }

    #[test]
    fn more_insects_sing_at_once_as_the_knob_rises_up_to_sixteen_bouts_and_sixteen_fireflies() {
        let mut previous = 0.0;
        for knob in [0.1, 0.25, 0.4, 0.55, 0.7, 0.85, 1.0] {
            let runs: Vec<(f64, usize, usize)> = (1..=4_u64)
                .map(|seed| census(seed * 31, knob, 400.0))
                .collect();
            let mean = runs.iter().map(|r| r.0).sum::<f64>() / 4.0;
            let peak = runs.iter().map(|r| r.1).max().unwrap();
            assert!(peak <= singing_cap(knob), "{knob}: {peak} at once");
            assert!(
                mean > previous * 1.08,
                "{knob}: mean {mean} after {previous}"
            );
            previous = mean;
            if knob == 0.55 {
                // The default scene: several singing at any time, never more than 9.
                assert!(
                    (3.0..=8.0).contains(&mean) && (6..=9).contains(&peak),
                    "{mean} {peak}"
                );
            }
            if knob == 1.0 {
                assert!(
                    (13..=MAX_SINGING).contains(&peak),
                    "{peak} at once at the top"
                );
                assert!(mean >= 9.0, "{mean}");
            }
        }
    }

    #[test]
    fn a_firefly_is_alive_exactly_while_its_insect_sings_and_sixteen_never_crowd_each_other_out() {
        use crate::audio::transport::VISIBLE_CAPACITY;
        let knobs = knobs(1.0, 0.0);
        let mut voice = voice_seeded(5, 24_000, knobs);
        let ring = VisualRing::new(crate::audio::transport::EVENT_CAPACITY, 1);
        let epoch = StreamEpoch::new(1).unwrap();
        let mut live = vec![None; VISIBLE_CAPACITY];
        let (mut peak, mut checked) = (0, 0);
        let delta = 0.01;
        for tick in 0..60_000 {
            let time = tick as f64 * delta;
            advance(&mut voice, &knobs, &ring, time, delta);
            // Well away from any start or end, the lit fireflies are the singing insects.
            let near_edge = voice.insects.iter().any(|i| {
                (i.singing() && (i.bout_age < 0.05 || i.bout_length - i.bout_age < 0.05))
                    || (!i.singing() && i.rest_remaining < 0.05)
            });
            if tick % 50 == 0 && !near_edge {
                let at = MachNanos::new((time * 1e9) as u64);
                let count = ring.read_live_into(epoch, at, &mut live);
                let fireflies = live[..count]
                    .iter()
                    .flatten()
                    .filter(|event| event.shape == VisualShape::Firefly)
                    .count();
                let singing = voice.insects.iter().filter(|i| i.singing()).count();
                assert_eq!(fireflies, singing, "at {time:.2} s");
                peak = peak.max(fireflies);
                checked += 1;
            }
        }
        assert!(
            checked > 500 && (13..=MAX_SINGING).contains(&peak),
            "{checked} {peak}"
        );
        // Sixteen long-lived fireflies and a crowd of other particles filling the visible set: a
        // firefly (a large particle) is never the one that is dropped.
        let ring = VisualRing::new(crate::audio::transport::EVENT_CAPACITY, 1);
        let present = MachNanos::new(1_000_000_000);
        let mut events = VisualWriter::new(
            &ring,
            epoch,
            SituationIndex::from_index(0, 1).unwrap(),
            WorldSeconds::new(1.0).unwrap(),
            present,
        );
        let particle = |x: f32, shape: VisualShape, large: bool, seconds: f64| ParticleVisual {
            x: Normalized::new(x).unwrap(),
            y: Normalized::new(0.5).unwrap(),
            strength: Normalized::new(1.0).unwrap(),
            radius: LogicalPixels::new(4.0).unwrap(),
            shape,
            large,
            lifetime: Duration::from_secs_f64(seconds),
        };
        for index in 0..MAX_SINGING {
            assert!(events.emit(particle(
                index as f32 / 16.0,
                VisualShape::Firefly,
                true,
                12.0
            )));
        }
        for index in 0..VISIBLE_CAPACITY + 200 {
            assert!(events.emit(particle(
                (index % 100) as f32 / 100.0,
                VisualShape::Point,
                false,
                5.0
            )));
        }
        let mut visible = [None; VISIBLE_CAPACITY];
        let count = ring.read_live_into(epoch, MachNanos::new(2_000_000_000), &mut visible);
        assert_eq!(count, VISIBLE_CAPACITY);
        let fireflies = visible
            .iter()
            .flatten()
            .filter(|event| event.shape == VisualShape::Firefly)
            .count();
        assert_eq!(fireflies, MAX_SINGING, "no firefly was evicted");
    }

    #[test]
    fn loudness_stays_in_the_window_across_seeds_and_the_knob_and_peaks_stay_soft() {
        // Full knob: 16 voices may sing (about 12 do), still bounded and not ear-piercing.
        let full: Vec<(f64, f32)> = (1..=8_u64)
            .map(|seed| levels_seeded(seed * 104_729 + 7, 24_000, knobs(1.0, 1.0), 60))
            .collect();
        for (rms, peak) in &full {
            assert!((-33.0..=-28.0).contains(rms), "full RMS {rms} dBFS");
            assert!(*peak > 0.0 && *peak < 0.62, "full peak {peak}");
        }
        let (low, high) = full.iter().fold((f64::MAX, f64::MIN), |(l, h), (r, _)| {
            (l.min(*r), h.max(*r))
        });
        assert!(
            (high - low) / 2.0 <= 1.5,
            "full spread {low:.1}..{high:.1} dBFS"
        );
        // The knob is progressive and every step is heard: about 4 to 5 dB a notch.
        let rms = |insects: f32| levels(24_000, knobs(insects, 0.5), 60).0;
        let (low, middle, top) = (rms(0.25), rms(0.55), rms(1.0));
        assert!(
            middle - low > 2.0 && top - middle > 2.0,
            "{low} {middle} {top}"
        );
        assert!(top - low < 12.0, "{low} {top}");
    }

    #[test]
    fn a_resting_insect_costs_a_compare_and_sounds_exactly_as_if_it_were_rendered() {
        let rate = 24_000_u32;
        for index in [0, 3, 12, 13] {
            let mut skipping = plateau_insect(index, 4);
            let mut full = plateau_insect(index, 4);
            for insect in [&mut skipping, &mut full] {
                insect.set_rate(rate);
                insect.envelope = 1.0;
                insect.start_phrase(&moment(3.0, 0.001, 0.8));
            }
            let mut skipped = 0;
            for tick in 0..rate as usize * 8 {
                for insect in [&mut skipping, &mut full] {
                    if insect.phrase.sounding() {
                        insect.phrase.age += 1.0 / f64::from(rate);
                    }
                }
                let a = skipping.render(rate as f32);
                let raw = full.raw_render(rate as f32);
                let b = full.space(raw);
                assert!((a - b).abs() < 1e-9, "{index} tick {tick}: {a} vs {b}");
                if skipping.quiet >= skipping.settle {
                    skipped += 1;
                    assert_eq!(a, 0.0);
                }
            }
            // Most of an eight-second wait for the next phrase was skipped.
            assert!(skipped > rate as usize, "{index}: {skipped} skipped");
        }
    }

    #[test]
    fn sixteen_insects_singing_at_once_never_allocate_in_the_callback() {
        for rate in [44_100, 48_000] {
            let knobs = knobs(1.0, 1.0);
            let mut voice = voice(rate, knobs);
            for insect in voice.insects.iter_mut().take(MAX_SINGING) {
                insect.rest_remaining = 0.0;
            }
            let ring = VisualRing::new(64, 1);
            let (loudest, counts) = crate::rt_alloc::measure(|| {
                let mut singing = 0;
                for sample in 0..rate as usize * 6 {
                    let frame = step(&mut voice, &knobs, &ring, sample);
                    assert!(frame.iter().all(|v| v.is_finite() && v.abs() < 0.85));
                    singing = singing.max(voice.insects.iter().filter(|i| i.singing()).count());
                }
                singing
            });
            assert_eq!(counts, crate::rt_alloc::Counts::default(), "rate {rate}");
            assert_eq!(loudest, MAX_SINGING, "all sixteen sing together at once");
        }
    }

    // ---- the six added species ----

    /// Phrase length and the pause after it, over many calls of every slot of `species` at the
    /// peak of a bout: ((shortest, longest) length, (shortest, longest) pause).
    fn call_ranges(species: Species) -> ((f64, f64), (f64, f64)) {
        let (mut length, mut pause) = ((f64::MAX, 0.0_f64), (f64::MAX, 0.0_f64));
        for index in slots(species) {
            for seed in 1..=6_u64 {
                let mut insect = plateau_insect(index, seed);
                insect.envelope = 1.0;
                for call in 0..80 {
                    insect.start_phrase(&moment(5.0 + f64::from(call) * 6.1, 0.001, 0.55));
                    length = (
                        length.0.min(insect.phrase.length),
                        length.1.max(insect.phrase.length),
                    );
                    pause = (
                        pause.0.min(insect.gap_remaining),
                        pause.1.max(insect.gap_remaining),
                    );
                }
            }
        }
        (length, pause)
    }

    #[test]
    fn the_added_species_call_for_the_lengths_the_spec_gives_and_repeat_every_few_seconds() {
        // (species, SPEC length range of the call, the whole call-and-pause period range)
        for (species, (low, high), (period_low, period_high)) in [
            (Species::Kantan, (2.0, 4.0), (3.2, 6.8)),
            (Species::Tsuzure, (1.0, 3.0), (2.6, 6.6)),
            (Species::Aomatsumushi, (1.0, 2.0), (1.3, 3.2)),
            (Species::Kusahibari, (0.8, 1.5), (2.2, 4.4)),
            (Species::Umaoi, (0.8, 1.3), (3.2, 5.8)),
            (Species::Kera, (2.0, 5.0), (3.8, 9.0)),
        ] {
            let (length, pause) = call_ranges(species);
            assert!(
                length.0 >= low - 1e-6 && length.1 <= high + 1e-6,
                "{species:?}: {length:?}"
            );
            // The draw spans most of the range.
            assert!(
                length.1 - length.0 > 0.6 * (high - low),
                "{species:?}: {length:?}"
            );
            assert!(
                length.0 + pause.0 >= period_low && length.1 + pause.1 <= period_high,
                "{species:?}: period {}..{}",
                length.0 + pause.0,
                length.1 + pause.1
            );
        }
        // ウマオイ: a 0.6 to 1 s "スイー", then a short "チョン".
        for index in slots(Species::Umaoi) {
            for seed in 1..=20_u64 {
                let mut insect = plateau_insect(index, seed);
                insect.envelope = 1.0;
                insect.start_phrase(&moment(5.0, 0.001, 0.55));
                assert!((0.6..=1.0).contains(&insect.phrase.ring));
            }
        }
    }

    #[test]
    fn the_added_species_have_their_own_phrase_shapes() {
        let at = |species: Species, phrase: &Phrase, t: f32| {
            phrase_sound(
                species,
                &Phrase {
                    age: f64::from(t),
                    ..*phrase
                },
            )
        };
        let phrase = |length: f64, ring: f32| Phrase {
            age: 0.0,
            length,
            pitch: 3_000.0,
            level: 1.0,
            count: 1,
            tempo: 1.0,
            ring,
        };
        // カンタン and ケラ: held, with a slow rise and a slow fall.
        for (species, length) in [(Species::Kantan, 3.0), (Species::Kera, 4.0)] {
            let p = phrase(length, 0.5);
            assert!(at(species, &p, 0.02).0 < 0.1 && at(species, &p, length as f32 - 0.02).0 < 0.1);
            assert!(at(species, &p, 1.0).0 > 0.99);
            assert!(at(species, &p, 0.2).0 < at(species, &p, 0.4).0, "rising");
        }
        // ツヅレサセコオロギ and アオマツムシ: a quick start, held to the end.
        for species in [Species::Tsuzure, Species::Aomatsumushi] {
            let p = phrase(1.5, 0.5);
            assert!(at(species, &p, 0.05).0 > 0.9 && at(species, &p, 0.75).0 > 0.99);
        }
        // クサヒバリ: it starts with a short rise in pitch (to the pitch it then holds), without
        // a pulse, and then trills.
        let p = phrase(1.0, 0.5);
        let ratios: Vec<f32> = (0..=10)
            .map(|step| at(Species::Kusahibari, &p, step as f32 * 0.01).1)
            .collect();
        assert!((ratios[0] - KUSAHIBARI_FROM).abs() < 1e-6);
        assert!(
            ratios.windows(2).all(|pair| pair[1] >= pair[0]),
            "rising: {ratios:?}"
        );
        assert!((at(Species::Kusahibari, &p, 0.5).1 - 1.0).abs() < 1e-6);
        assert_eq!(stroke_context(Species::Kusahibari, 0.05, 1.0, 0.5).2, 0.0);
        assert_eq!(stroke_context(Species::Kusahibari, 0.5, 1.0, 0.5).2, 1.0);
        let fade: Vec<f32> = (0..=30)
            .map(|step| stroke_context(Species::Kusahibari, 0.10 + step as f32 * 0.001, 1.0, 0.5).2)
            .collect();
        assert!(
            fade.windows(2)
                .all(|pair| pair[1] >= pair[0] && pair[1] - pair[0] < 0.2)
        );
        // One grain across the handover: the pulse's phase is not restarted there.
        assert_eq!(
            stroke_context(Species::Kusahibari, 0.099, 1.0, 0.5).0,
            stroke_context(Species::Kusahibari, 0.101, 1.0, 0.5).0
        );
        // ウマオイ: a long "スイー" gliding up, a pause, then a short higher "チョン".
        let ring = 0.8;
        let p = phrase(f64::from(ring + UMAOI_PAUSE + UMAOI_CHON), ring);
        let mut runs = Vec::new();
        let mut inside = false;
        for tick in 0..1_100 {
            let t = tick as f32 * 0.001;
            let (amplitude, ratio) = at(Species::Umaoi, &p, t);
            if amplitude > 0.02 && !inside {
                runs.push((t, ratio));
            }
            inside = amplitude > 0.02;
        }
        assert_eq!(runs.len(), 2, "{runs:?}");
        assert!(runs[1].0 > ring + UMAOI_PAUSE - 0.01 && runs[1].1 > 1.05);
        assert!(
            at(Species::Umaoi, &p, ring + 0.07).0 == 0.0,
            "a pause between the two parts"
        );
        assert!(
            at(Species::Umaoi, &p, 0.4).1 > at(Species::Umaoi, &p, 0.1).1,
            "gliding up"
        );
    }

    #[test]
    fn the_added_species_pulse_with_the_depth_that_gives_each_its_voice() {
        for seed in 1..=6_u64 {
            let depth = |species: Species| {
                let index = slots(species)[0];
                envelope_depth(&envelope_of(&flat_run(index, seed, 0.5, 0, 2.0)))
            };
            let (kantan, tsuzure) = (depth(Species::Kantan), depth(Species::Tsuzure));
            assert!((0.38..=0.80).contains(&kantan), "kantan {kantan}");
            // The fast, fine "リリリリ" is the shallowest.
            assert!((0.18..=0.48).contains(&tsuzure), "tsuzure {tsuzure}");
            assert!(tsuzure < kantan);
            let aomatsumushi = depth(Species::Aomatsumushi);
            assert!(
                (0.30..=0.72).contains(&aomatsumushi),
                "aomatsumushi {aomatsumushi}"
            );
            let kusahibari = depth(Species::Kusahibari);
            assert!(
                (0.22..=0.58).contains(&kusahibari),
                "kusahibari {kusahibari}"
            );
            let (umaoi, kera) = (depth(Species::Umaoi), depth(Species::Kera));
            assert!(umaoi >= 0.55, "the rough umaoi {umaoi}");
            assert!((0.38..=0.80).contains(&kera), "kera {kera}");
        }
    }

    /// The RMS (dBFS) of pool slot `index` while its phrases sound on the plateau of a bout, over
    /// `seconds` of calls, and the share of the time it sounds. `heard` measures it after its
    /// distance (gain, air, delay: what reaches the mix); otherwise at the source.
    fn singing_level_of(index: usize, seconds: usize, heard: bool) -> (f64, f64) {
        let rate = 24_000_u32;
        let mut insect = plateau_insect(index, 3);
        insect.set_rate(rate);
        insect.envelope = 1.0;
        let delta = 1.0 / f64::from(rate);
        let (mut energy, mut sounding) = (0.0_f64, 0_usize);
        for tick in 0..rate as usize * seconds {
            let time = 10.0 + tick as f64 * delta;
            if insect.phrase.sounding() {
                insect.update_pulse(time);
                insect.phrase.age += delta;
                sounding += 1;
            } else {
                insect.gap_remaining -= delta;
                if insect.gap_remaining <= 0.0 {
                    insect.start_phrase(&moment(time, delta, 0.55));
                }
            }
            let sample = f64::from(if heard {
                insect.render(rate as f32)
            } else {
                insect.raw_render(rate as f32)
            });
            energy += sample * sample;
        }
        (
            10.0 * (energy / sounding.max(1) as f64).log10(),
            sounding as f64 / (rate as f64 * seconds as f64),
        )
    }

    #[test]
    fn the_added_species_are_as_loud_or_as_quiet_as_the_spec_asks() {
        // At the source (nearest distance): アオマツムシ is a little louder than every other
        // species, クサヒバリ and ケラ are quiet.
        let level = |species| singing_level_of(slots(species)[0], 40, false).0;
        let suzumushi = level(Species::Suzumushi);
        let aomatsumushi = level(Species::Aomatsumushi);
        for species in ALL_SPECIES {
            if species != Species::Aomatsumushi {
                assert!(
                    aomatsumushi > level(species) + 0.3,
                    "{species:?} {} vs {aomatsumushi}",
                    level(species)
                );
            }
        }
        assert!(aomatsumushi - suzumushi < 3.0, "only a little louder");
        assert!(level(Species::Kusahibari) < suzumushi - 2.5);
        assert!(level(Species::Kera) < suzumushi - 3.0);
        // The others sit with the suzumushi (the pair matters for a balanced mix).
        for species in [Species::Kantan, Species::Tsuzure, Species::Umaoi] {
            assert!((level(species) - suzumushi).abs() < 6.0, "{species:?}");
        }
    }

    #[test]
    fn every_species_is_audible_as_heard_in_the_mix_and_aomatsumushi_is_the_loudest() {
        // The level at the output of each slot while its phrase sounds: after its distance, the
        // insect trim and the output trim (in the channel it is panned to).
        let gain = 20.0 * (INSECT_TRIM * OUTPUT_TRIM).log10() as f64;
        let heard: Vec<f64> = (0..MAX_INSECTS)
            .map(|index| singing_level_of(index, 20, true).0 + gain)
            .collect();
        let bed = levels(24_000, knobs(0.0, 0.0), 20).0;
        let species_levels = |species: Species| -> Vec<f64> {
            slots(species)
                .into_iter()
                .map(|index| heard[index])
                .collect()
        };
        // Every call of every species is clearly above the night air (12 dB) and not below
        // about -47 dBFS, however far its slot.
        for species in ALL_SPECIES {
            for level in species_levels(species) {
                assert!(
                    level >= bed + 12.0 && level >= -47.0,
                    "{species:?}: {level:.1} dBFS (night air {bed:.1})"
                );
            }
        }
        // アオマツムシ is near and the loudest of all, a little (not by a huge margin).
        let aomatsumushi = species_levels(Species::Aomatsumushi);
        for index in slots(Species::Aomatsumushi) {
            assert_eq!(DISTANCE_TABLE[index], 0, "aomatsumushi #{index} is near");
        }
        for (index, level) in heard.iter().enumerate() {
            if SPECIES[index] != Species::Aomatsumushi {
                for loud in &aomatsumushi {
                    assert!(
                        loud > &(level + 1.0),
                        "#{index} {:?} {level:.1} vs {loud:.1}",
                        SPECIES[index]
                    );
                }
            }
        }
        // クサヒバリ is small and ケラ a far, quiet background, but both are heard.
        let mut sorted = heard.clone();
        sorted.sort_by(f64::total_cmp);
        let median = (sorted[11] + sorted[12]) / 2.0;
        for species in [Species::Kusahibari, Species::Kera] {
            for index in slots(species) {
                assert!(DISTANCE_TABLE[index] >= 2, "{species:?} #{index} is far");
                assert!(
                    heard[index] < median - 1.0,
                    "{species:?} {} vs median {median}",
                    heard[index]
                );
            }
        }
        assert!(aomatsumushi.iter().all(|level| *level > median + 6.0));
    }

    #[test]
    fn the_added_species_start_without_clicks_and_end_at_zero() {
        let rate = 48_000_u32;
        for species in [
            Species::Kantan,
            Species::Tsuzure,
            Species::Aomatsumushi,
            Species::Kusahibari,
            Species::Umaoi,
            Species::Kera,
        ] {
            for index in slots(species) {
                let mut insect = plateau_insect(index, 7);
                insect.set_rate(rate);
                insect.envelope = 1.0;
                insect.start_phrase(&moment(3.0, 1.0 / f64::from(rate), 1.0));
                let samples: Vec<f32> = (0..(insect.phrase.length * f64::from(rate)) as usize
                    + 200)
                    .map(|_| {
                        let sample = insect.raw_render(rate as f32);
                        insect.phrase.age += 1.0 / f64::from(rate);
                        sample
                    })
                    .collect();
                let peak = samples.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
                // The raw sample step of a high carrier is about its own peak, so test the
                // amplitude envelope instead: the peak over a window of a few carrier cycles
                // (at the band's low edge) may only move gently from one sample to the next. A
                // click or a step in gain (a restarted pulse, a switched grain) is a jump.
                // The rough voices beat against their second partial (1.08x), which ripples the
                // window's peak by itself, so theirs is judged more loosely (their pulse handover
                // is at silence anyway: the "チョン" starts from zero).
                let (cycles, allowed) = if species.rough() {
                    (4.0, 0.30)
                } else {
                    (2.0, 0.08)
                };
                let window = (cycles * rate as f32 / species.band().0) as usize + 1;
                let envelope: Vec<f32> = samples
                    .windows(window)
                    .map(|w| w.iter().fold(0.0_f32, |m, s| m.max(s.abs())))
                    .collect();
                let jump = envelope
                    .windows(2)
                    .map(|pair| (pair[1] - pair[0]).abs())
                    .fold(0.0_f32, f32::max);
                assert!(
                    jump <= allowed * peak,
                    "{species:?} #{index}: the envelope steps by {} of its peak",
                    jump / peak
                );
                assert!(samples.last().is_none_or(|last| last.abs() < 0.02 * peak));
            }
        }
    }

    #[test]
    fn the_kusahibari_trill_takes_over_from_the_chirp_without_a_step_in_loudness() {
        let rate = 48_000_u32;
        for index in slots(Species::Kusahibari) {
            for seed in 1..=40_u64 {
                let mut insect = plateau_insect(index, seed);
                insect.set_rate(rate);
                insect.envelope = 1.0;
                insect.start_phrase(&moment(3.0, 1.0 / f64::from(rate), 1.0));
                let samples: Vec<f32> = (0..rate as usize / 4)
                    .map(|_| {
                        let sample = insect.raw_render(rate as f32);
                        insect.phrase.age += 1.0 / f64::from(rate);
                        sample
                    })
                    .collect();
                // The peak over about two carrier cycles, per sample, from 80 to 140 ms (the
                // chirp ends at 100 ms and the trill's depth fades in over the next 30).
                let window = 17;
                let envelope: Vec<f32> = samples
                    .windows(window)
                    .map(|w| w.iter().fold(0.0_f32, |m, s| m.max(s.abs())))
                    .collect();
                let peak = envelope.iter().fold(0.0_f32, |m, e| m.max(*e));
                let around =
                    &envelope[(0.08 * rate as f32) as usize..(0.14 * rate as f32) as usize];
                let jump = around
                    .windows(2)
                    .map(|pair| (pair[1] - pair[0]).abs())
                    .fold(0.0_f32, f32::max);
                assert!(
                    jump <= 0.06 * peak,
                    "seed {seed}: a step of {} of the peak",
                    jump / peak
                );
            }
        }
    }
}
