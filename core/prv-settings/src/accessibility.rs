//! What the system asked for on the user's behalf.
//!
//! # The platform's answer wins upward, never downward
//!
//! A user who has turned on "reduce motion" in the operating system has already
//! answered the question, for every application, for a reason they did not
//! explain and should not have to. Master Prompt #8 and Master Prompt #16 make
//! accessibility non-negotiable, and the concrete meaning of that is the rule in
//! [`Accessibility::reduce_motion`]: the platform can turn it on and nothing in
//! this application can turn it off.
//!
//! The reverse direction is open. Someone whose system setting is off may still
//! want less motion *here* — a visualiser is not a settings screen — so an
//! in-application preference can add a restraint the platform did not ask for.
//! Effective value is the union, never the intersection.
//!
//! # Not persisted
//!
//! These values come from the platform at launch and change while the
//! application is running. Storing them would mean a copy that is wrong every
//! time the user changes their mind in system settings, and the wrongness would
//! favour *less* accessibility, which is the direction that matters.

/// What the platform reports about the user's needs.
///
/// Supplied by the host at launch and whenever it changes. Every field defaults
/// to "no accommodation requested", because that is what a platform that does
/// not report the setting means — not that the user does not need it, but that
/// we have not been told, and inventing an answer is worse than deferring to the
/// in-application preference.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PlatformAccessibility {
    /// The system-level "reduce motion" setting.
    pub reduce_motion: bool,
    /// The system-level "increase contrast" setting.
    pub increase_contrast: bool,
    /// The system-level "reduce transparency" setting.
    pub reduce_transparency: bool,
    /// The system text size, as a multiple of the default.
    pub text_scale: f32,
}

impl PlatformAccessibility {
    /// The smallest text scale honoured.
    pub const MIN_TEXT_SCALE: f32 = 0.8;
    /// The largest text scale honoured.
    ///
    /// Chosen from the platform's own accessibility sizes rather than from what
    /// the layout is comfortable with. A limit set by the layout is a limit that
    /// tells a user with low vision that their need is an edge case.
    pub const MAX_TEXT_SCALE: f32 = 3.0;

    /// Nothing reported.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            reduce_motion: false,
            increase_contrast: false,
            reduce_transparency: false,
            text_scale: 1.0,
        }
    }

    /// The reported text scale, made usable.
    ///
    /// A platform that reports nothing, or something impossible, means the
    /// default size. Clamping rather than rejecting is right here: a text scale
    /// that fails validation must not become an error the user sees, because
    /// the user did nothing wrong and the remedy is not theirs.
    #[must_use]
    pub fn usable_text_scale(self) -> f32 {
        if self.text_scale.is_nan() || self.text_scale <= 0.0 {
            return 1.0;
        }
        self.text_scale
            .clamp(Self::MIN_TEXT_SCALE, Self::MAX_TEXT_SCALE)
    }
}

/// The accommodations actually in force.
///
/// Built from what the platform asked for and what the user asked for here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Accessibility {
    platform: PlatformAccessibility,
    application_reduce_motion: bool,
    application_increase_contrast: bool,
    application_reduce_transparency: bool,
}

impl Default for Accessibility {
    fn default() -> Self {
        Self::from_platform(PlatformAccessibility::none())
    }
}

impl Accessibility {
    /// Starts from what the platform reports, with nothing added here.
    #[must_use]
    pub const fn from_platform(platform: PlatformAccessibility) -> Self {
        Self {
            platform,
            application_reduce_motion: false,
            application_increase_contrast: false,
            application_reduce_transparency: false,
        }
    }

    /// Replaces the platform's answer, keeping the user's own additions.
    ///
    /// Called when the system setting changes while the application is running.
    pub fn update_platform(&mut self, platform: PlatformAccessibility) {
        self.platform = platform;
    }

    /// Adds — or withdraws — an in-application request for less motion.
    ///
    /// Withdrawing it does not switch motion back on if the platform asked for
    /// less; it only withdraws the extra restraint added here.
    pub fn request_reduced_motion(&mut self, requested: bool) {
        self.application_reduce_motion = requested;
    }

    /// Adds or withdraws an in-application request for more contrast.
    pub fn request_increased_contrast(&mut self, requested: bool) {
        self.application_increase_contrast = requested;
    }

    /// Adds or withdraws an in-application request for less transparency.
    pub fn request_reduced_transparency(&mut self, requested: bool) {
        self.application_reduce_transparency = requested;
    }

    /// Whether motion must be reduced.
    ///
    /// True if either the platform or the application asked. This is the union
    /// rather than the intersection, permanently: the platform can turn it on
    /// and nothing here can turn it off.
    #[must_use]
    pub const fn reduce_motion(&self) -> bool {
        self.platform.reduce_motion || self.application_reduce_motion
    }

    /// Whether contrast must be increased.
    #[must_use]
    pub const fn increase_contrast(&self) -> bool {
        self.platform.increase_contrast || self.application_increase_contrast
    }

    /// Whether transparency must be reduced.
    #[must_use]
    pub const fn reduce_transparency(&self) -> bool {
        self.platform.reduce_transparency || self.application_reduce_transparency
    }

    /// The text scale in force.
    #[must_use]
    pub fn text_scale(&self) -> f32 {
        self.platform.usable_text_scale()
    }

    /// Whether any accommodation is in force.
    ///
    /// What a visualiser checks once rather than three times.
    #[must_use]
    pub fn is_any_requested(&self) -> bool {
        self.reduce_motion()
            || self.increase_contrast()
            || self.reduce_transparency()
            || self.text_scale() > 1.0
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        reason = "fixtures are exact; a test that cannot build one should fail loudly"
    )]

    use super::*;

    #[test]
    fn the_platform_can_turn_reduced_motion_on_and_the_application_cannot_turn_it_off() {
        // The rule the module exists for. Someone who set this in the operating
        // system has already answered, for a reason they should not have to
        // explain to us.
        let mut accessibility = Accessibility::from_platform(PlatformAccessibility {
            reduce_motion: true,
            ..PlatformAccessibility::none()
        });
        assert!(accessibility.reduce_motion());

        accessibility.request_reduced_motion(false);
        assert!(
            accessibility.reduce_motion(),
            "an application preference overrode a platform accommodation"
        );
    }

    #[test]
    fn the_application_can_add_a_restraint_the_platform_did_not_ask_for() {
        // The open direction. A visualiser is not a settings screen, and
        // someone may want less motion here and not everywhere.
        let mut accessibility = Accessibility::default();
        assert!(!accessibility.reduce_motion());

        accessibility.request_reduced_motion(true);
        assert!(accessibility.reduce_motion());

        accessibility.request_reduced_motion(false);
        assert!(
            !accessibility.reduce_motion(),
            "withdrawing an application request should withdraw only that"
        );
    }

    #[test]
    fn every_accommodation_is_a_union_in_the_same_direction() {
        for (set_platform, request, read) in [
            (
                (|p: &mut PlatformAccessibility| p.increase_contrast = true)
                    as fn(&mut PlatformAccessibility),
                Accessibility::request_increased_contrast as fn(&mut Accessibility, bool),
                Accessibility::increase_contrast as fn(&Accessibility) -> bool,
            ),
            (
                |p: &mut PlatformAccessibility| p.reduce_transparency = true,
                Accessibility::request_reduced_transparency,
                Accessibility::reduce_transparency,
            ),
        ] {
            let mut platform = PlatformAccessibility::none();
            set_platform(&mut platform);

            let mut from_platform = Accessibility::from_platform(platform);
            request(&mut from_platform, false);
            assert!(read(&from_platform), "the platform's answer was discarded");

            let mut from_application = Accessibility::default();
            assert!(!read(&from_application));
            request(&mut from_application, true);
            assert!(read(&from_application));
        }
    }

    #[test]
    fn a_system_setting_that_changes_while_running_is_picked_up() {
        // Why these are not persisted: a stored copy is wrong the moment the
        // user changes their mind, and it is wrong in the direction that
        // matters.
        let mut accessibility = Accessibility::default();
        assert!(!accessibility.reduce_motion());

        accessibility.update_platform(PlatformAccessibility {
            reduce_motion: true,
            ..PlatformAccessibility::none()
        });
        assert!(accessibility.reduce_motion());
    }

    #[test]
    fn updating_the_platform_keeps_what_the_user_asked_for_here() {
        let mut accessibility = Accessibility::default();
        accessibility.request_increased_contrast(true);
        accessibility.update_platform(PlatformAccessibility::none());
        assert!(accessibility.increase_contrast());
    }

    #[test]
    fn an_impossible_text_scale_becomes_the_default_rather_than_an_error() {
        // The user did nothing wrong and the remedy is not theirs.
        for reported in [f32::NAN, 0.0, -2.0] {
            let platform = PlatformAccessibility {
                text_scale: reported,
                ..PlatformAccessibility::none()
            };
            assert_eq!(platform.usable_text_scale(), 1.0);
        }
    }

    #[test]
    fn the_text_scale_limit_comes_from_the_platform_not_from_the_layout() {
        // A limit set by what the layout is comfortable with tells a user with
        // low vision that their need is an edge case.
        let huge = PlatformAccessibility {
            text_scale: 10.0,
            ..PlatformAccessibility::none()
        };
        assert_eq!(
            huge.usable_text_scale(),
            PlatformAccessibility::MAX_TEXT_SCALE
        );
        const {
            assert!(PlatformAccessibility::MAX_TEXT_SCALE >= 3.0);
        }

        let tiny = PlatformAccessibility {
            text_scale: 0.1,
            ..PlatformAccessibility::none()
        };
        assert_eq!(
            tiny.usable_text_scale(),
            PlatformAccessibility::MIN_TEXT_SCALE
        );
    }

    #[test]
    fn nothing_requested_is_reported_as_nothing_requested() {
        let accessibility = Accessibility::default();
        assert!(!accessibility.is_any_requested());
        assert_eq!(accessibility.text_scale(), 1.0);

        let mut larger = Accessibility::from_platform(PlatformAccessibility {
            text_scale: 1.5,
            ..PlatformAccessibility::none()
        });
        assert!(larger.is_any_requested());
        larger.request_reduced_motion(true);
        assert!(larger.is_any_requested());
    }
}
