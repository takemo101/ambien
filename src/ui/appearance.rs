//! System accessibility state and the rules it changes in the scene.
//!
//! The native monitor only signals "something changed"; the latest values are read on
//! the main thread afterwards, so a burst of notifications never queues stale state.

use crate::platform::macos::DisplayOptions;
use async_channel::{Receiver, RecvError, Sender, TrySendError};
use ely::theme::Theme;
use gpui::{App, WindowBackgroundAppearance};

use crate::situations::VisualShape;

/// OS display options Ambien reacts to. Both are independent of audio state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct AccessibilityState {
    pub(crate) reduce_motion: bool,
    pub(crate) reduce_transparency: bool,
}

impl AccessibilityState {
    pub(crate) fn motion(self) -> Motion {
        if self.reduce_motion {
            Motion::Reduced
        } else {
            Motion::Full
        }
    }

    /// The scene surface. Reduce transparency replaces the glass with an opaque system
    /// palette whatever the settings say; otherwise the blur setting chooses between the
    /// blurred glass and a plain transparent surface. Turning reduce transparency off
    /// returns to the user's choice.
    pub(crate) fn scene_background(self, blur: bool) -> WindowBackgroundAppearance {
        if self.reduce_transparency {
            WindowBackgroundAppearance::Opaque
        } else if blur {
            WindowBackgroundAppearance::Blurred
        } else {
            WindowBackgroundAppearance::Transparent
        }
    }

    /// Applies motion to GPUI and Ely. The scene reads its own `Motion` separately.
    pub(crate) fn apply_to_app(self, cx: &mut App) {
        cx.set_reduce_motion(self.reduce_motion);
        Theme::update(cx, |theme| theme.reduced_motion = self.reduce_motion);
    }
}

impl From<DisplayOptions> for AccessibilityState {
    fn from(options: DisplayOptions) -> Self {
        Self {
            reduce_motion: options.reduce_motion,
            reduce_transparency: options.reduce_transparency,
        }
    }
}

/// How decorative scene motion is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Motion {
    Full,
    Reduced,
}

// Reduced motion fades a particle in and out over these fractions of its lifetime,
// so a light never appears or disappears abruptly.
const REDUCED_ONSET: f32 = 0.12;
const REDUCED_RELEASE: f32 = 0.6;

fn smoothstep(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    value * value * (3.0 - 2.0 * value)
}

impl Motion {
    /// Opacity envelope over a particle's normalized age (0 = born, 1 = expired).
    pub(crate) fn particle_fade(self, age: f32) -> f32 {
        match self {
            Self::Full => (1.0 - age).clamp(0.0, 1.0).powi(2),
            Self::Reduced => {
                smoothstep(age / REDUCED_ONSET).min(smoothstep((1.0 - age) / REDUCED_RELEASE))
            }
        }
    }

    /// Radius multiplier. Reduced motion keeps every shape at its birth size.
    pub(crate) fn shape_scale(self, shape: VisualShape, age: f32) -> f32 {
        match (self, shape) {
            (Self::Reduced, _) | (_, VisualShape::Point) => 1.0,
            (Self::Full, VisualShape::Impact) => 1.0 + age * 1.6,
            (Self::Full, VisualShape::SlowGlow) => 1.0 + age * 0.4,
        }
    }

    /// Radius of a situation's background glow. The breath only swells the radius in
    /// full motion; reduced motion keeps the size still and lets opacity do the pulsing.
    pub(crate) fn glow_radius(self, contribution: f32, breath: f32) -> f32 {
        let swell = match self {
            Self::Full => contribution * breath,
            Self::Reduced => contribution,
        };
        24.0 + swell * 70.0
    }
}

/// Result of signalling a pending environment re-read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Notify {
    Queued,
    /// A re-read is already pending; it will observe the newest native state.
    Coalesced,
    /// The owner already shut down.
    Closed,
}

/// Send half of the capacity-one change channel. It is `Send` and holds no GPUI state.
#[derive(Clone)]
pub(crate) struct ChangeNotifier(Sender<()>);

impl ChangeNotifier {
    pub(crate) fn notify(&self) -> Notify {
        match self.0.try_send(()) {
            Ok(()) => Notify::Queued,
            Err(TrySendError::Full(())) => Notify::Coalesced,
            Err(TrySendError::Closed(())) => Notify::Closed,
        }
    }

    pub(crate) fn close(&self) {
        self.0.close();
    }
}

pub(crate) struct ChangeReceiver(Receiver<()>);

impl ChangeReceiver {
    pub(crate) async fn recv(&self) -> Result<(), RecvError> {
        self.0.recv().await
    }
}

pub(crate) fn change_channel() -> (ChangeNotifier, ChangeReceiver) {
    let (sender, receiver) = async_channel::bounded(1);
    (ChangeNotifier(sender), ChangeReceiver(receiver))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduce_motion_removes_swelling_and_abrupt_onset_but_keeps_a_soft_pulse() {
        let reduced = Motion::Reduced;
        let full = Motion::Full;
        for age in [0.0, 0.25, 0.5, 0.75, 1.0] {
            assert_eq!(reduced.shape_scale(VisualShape::Impact, age), 1.0);
            assert_eq!(reduced.shape_scale(VisualShape::SlowGlow, age), 1.0);
        }
        assert!(full.shape_scale(VisualShape::Impact, 0.5) > 1.5);
        assert_eq!(full.particle_fade(0.0), 1.0, "full motion starts at once");
        assert_eq!(reduced.particle_fade(0.0), 0.0);
        assert_eq!(reduced.particle_fade(1.0), 0.0);
        let mut previous = reduced.particle_fade(0.0);
        let mut peak: f32 = 0.0;
        for step in 1..=1000 {
            let fade = reduced.particle_fade(step as f32 / 1000.0);
            assert!((fade - previous).abs() < 0.03, "no jump at step {step}");
            peak = peak.max(fade);
            previous = fade;
        }
        assert!(peak > 0.99, "a soft pulse still reaches the full envelope");
        assert_eq!(reduced.glow_radius(0.5, 0.0), reduced.glow_radius(0.5, 1.0));
        assert!(full.glow_radius(0.5, 1.0) > full.glow_radius(0.5, 0.0));
    }

    #[test]
    fn reduce_transparency_is_opaque_and_releasing_it_restores_the_users_choice() {
        let mut state = AccessibilityState::default();
        assert_eq!(
            state.scene_background(true),
            WindowBackgroundAppearance::Blurred
        );
        assert_eq!(
            state.scene_background(false),
            WindowBackgroundAppearance::Transparent
        );
        state.reduce_transparency = true;
        // The system setting wins over the blur choice, whichever it is.
        assert_eq!(
            state.scene_background(true),
            WindowBackgroundAppearance::Opaque
        );
        assert_eq!(
            state.scene_background(false),
            WindowBackgroundAppearance::Opaque
        );
        // Motion is independent of transparency.
        assert_eq!(state.motion(), Motion::Full);
        state.reduce_transparency = false;
        state.reduce_motion = true;
        assert_eq!(
            state.scene_background(true),
            WindowBackgroundAppearance::Blurred
        );
        assert_eq!(
            state.scene_background(false),
            WindowBackgroundAppearance::Transparent
        );
        assert_eq!(state.motion(), Motion::Reduced);
    }

    #[test]
    fn a_burst_of_notifications_leaves_exactly_one_pending_re_read() {
        let (notifier, receiver) = change_channel();
        assert_eq!(notifier.notify(), Notify::Queued);
        for _ in 0..100 {
            assert_eq!(notifier.notify(), Notify::Coalesced);
        }
        assert_eq!(receiver.0.len(), 1);
        receiver.0.try_recv().unwrap();
        assert!(
            receiver.0.try_recv().is_err(),
            "nothing is queued behind it"
        );
        assert_eq!(
            notifier.notify(),
            Notify::Queued,
            "the next change is seen again"
        );
    }

    #[test]
    fn closing_wakes_the_owner_and_later_notifications_report_closed() {
        let (notifier, receiver) = change_channel();
        let clone = notifier.clone();
        notifier.close();
        assert_eq!(clone.notify(), Notify::Closed);
        assert!(receiver.0.try_recv().is_err());
        assert!(
            receiver.0.is_closed(),
            "the owner task can observe shutdown"
        );
    }
}
