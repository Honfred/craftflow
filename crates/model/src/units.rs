//! Internal units (DESIGN.md §3, rule 4).
//!
//! Everything inside the core is **per tick** and `f64`. Conversion to the
//! units a human reads happens only at the UI/CLI boundary, which is what
//! [`TimeUnit`] is for. Floating point means equality must go through
//! [`approx_eq`] rather than `==`.

use serde::{Deserialize, Serialize};

/// Minecraft runs at 20 ticks per second.
pub const TICKS_PER_SECOND: f64 = 20.0;

/// Relative tolerance for comparing computed flows.
pub const EPSILON: f64 = 1e-9;

/// Compares two flows with a tolerance that scales with their magnitude.
///
/// A purely absolute epsilon breaks down once numbers get large (a mB/tick
/// figure for a big fluid network), and a purely relative one breaks down near
/// zero, so this uses the larger of the two baselines.
#[must_use]
pub fn approx_eq(a: f64, b: f64) -> bool {
    let scale = a.abs().max(b.abs()).max(1.0);
    (a - b).abs() <= EPSILON * scale
}

/// A unit of time the user can choose to read rates in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeUnit {
    Tick,
    Second,
    #[default]
    Minute,
    Hour,
}

impl TimeUnit {
    /// How many ticks fit into one of these units.
    #[must_use]
    pub const fn ticks(self) -> f64 {
        match self {
            Self::Tick => 1.0,
            Self::Second => TICKS_PER_SECOND,
            Self::Minute => TICKS_PER_SECOND * 60.0,
            Self::Hour => TICKS_PER_SECOND * 60.0 * 60.0,
        }
    }

    /// Short suffix for display: `/t`, `/s`, `/min`, `/h`.
    #[must_use]
    pub const fn suffix(self) -> &'static str {
        match self {
            Self::Tick => "/t",
            Self::Second => "/s",
            Self::Minute => "/min",
            Self::Hour => "/h",
        }
    }
}

/// A duration, in ticks.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Ticks(pub f64);

impl Ticks {
    #[must_use]
    pub fn from_unit(unit: TimeUnit, amount: f64) -> Self {
        Self(amount * unit.ticks())
    }

    #[must_use]
    pub fn in_unit(self, unit: TimeUnit) -> f64 {
        self.0 / unit.ticks()
    }
}

/// A flow, stored per tick.
///
/// A newtype rather than a bare `f64` so that a per-minute number read from the
/// UI cannot be mistaken for an internal one; the only ways in and out name
/// their unit.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Rate(f64);

impl Rate {
    pub const ZERO: Self = Self(0.0);

    #[must_use]
    pub const fn per_tick(value: f64) -> Self {
        Self(value)
    }

    /// `amount` per one `unit`, e.g. 600 items per minute.
    #[must_use]
    pub fn per(unit: TimeUnit, amount: f64) -> Self {
        Self(amount / unit.ticks())
    }

    #[must_use]
    pub const fn as_per_tick(self) -> f64 {
        self.0
    }

    #[must_use]
    pub fn as_per(self, unit: TimeUnit) -> f64 {
        self.0 * unit.ticks()
    }

    /// `amount` produced every `duration` ticks.
    #[must_use]
    pub fn per_duration(amount: f64, duration: Ticks) -> Self {
        Self(amount / duration.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{Rate, Ticks, TimeUnit, approx_eq};

    #[test]
    fn ticks_per_unit() {
        assert_eq!(TimeUnit::Tick.ticks(), 1.0);
        assert_eq!(TimeUnit::Second.ticks(), 20.0);
        assert_eq!(TimeUnit::Minute.ticks(), 1200.0);
        assert_eq!(TimeUnit::Hour.ticks(), 72_000.0);
    }

    #[test]
    fn rate_converts_between_units() {
        // 600 items per minute is half an item per tick.
        let rate = Rate::per(TimeUnit::Minute, 600.0);
        assert!(approx_eq(rate.as_per_tick(), 0.5));
        assert!(approx_eq(rate.as_per(TimeUnit::Second), 10.0));
        assert!(approx_eq(rate.as_per(TimeUnit::Minute), 600.0));
        assert!(approx_eq(rate.as_per(TimeUnit::Hour), 36_000.0));
    }

    #[test]
    fn rate_round_trips_through_every_unit() {
        for unit in [
            TimeUnit::Tick,
            TimeUnit::Second,
            TimeUnit::Minute,
            TimeUnit::Hour,
        ] {
            let rate = Rate::per(unit, 7.5);
            assert!(approx_eq(rate.as_per(unit), 7.5), "failed for {unit:?}");
        }
    }

    #[test]
    fn rate_from_a_recipe_duration() {
        // A vanilla furnace smelts one item every 200 ticks.
        let rate = Rate::per_duration(1.0, Ticks(200.0));
        assert!(approx_eq(rate.as_per_tick(), 0.005));
        assert!(approx_eq(rate.as_per(TimeUnit::Minute), 6.0));
    }

    #[test]
    fn ticks_convert_to_and_from_units() {
        assert!(approx_eq(Ticks(200.0).in_unit(TimeUnit::Second), 10.0));
        assert!(approx_eq(Ticks::from_unit(TimeUnit::Second, 10.0).0, 200.0));
    }

    #[test]
    fn approx_eq_scales_with_magnitude() {
        assert!(approx_eq(0.1 + 0.2, 0.3));
        assert!(approx_eq(1e9, 1e9 + 0.1));
        assert!(!approx_eq(1.0, 1.001));
        // Near zero the tolerance stays absolute rather than collapsing.
        assert!(approx_eq(0.0, 1e-12));
        assert!(!approx_eq(0.0, 1e-3));
    }
}
