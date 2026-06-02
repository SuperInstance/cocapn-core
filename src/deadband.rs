#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Direction of the deadband sensitivity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum DeadbandDirection {
    /// Trigger in both directions
    Both,
    /// Only trigger when value exceeds above center (e.g., overcurrent protection)
    Above,
    /// Only trigger when value drops below center (e.g., conservation — only flag decrease)
    Below,
}

/// State relative to the deadband.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum DeadbandState {
    /// Within tolerance
    Normal,
    /// Nearing the edge of tolerance
    Approaching,
    /// Outside tolerance — action needed
    Exceeded,
}

/// The trigger mechanism. Only fires when a value deviates from center
/// beyond a relative tolerance.
#[derive(Debug, Clone)]
pub struct Deadband {
    pub center: f64,
    pub tolerance: f64,
    pub direction: DeadbandDirection,
}

impl Deadband {
    pub fn new(center: f64, tolerance: f64, direction: DeadbandDirection) -> Self {
        Self {
            center,
            tolerance,
            direction,
        }
    }

    /// Create a deadband with relative tolerance as a fraction (e.g., 0.1 = 10%).
    pub fn with_relative_tolerance(center: f64, pct: f64) -> Self {
        Self {
            center,
            tolerance: pct,
            direction: DeadbandDirection::Both,
        }
    }

    /// Check a value against the deadband.
    pub fn check(&self, value: f64) -> DeadbandState {
        if self.center == 0.0 {
            // If center is zero, use absolute tolerance
            let abs_diff = (value - self.center).abs();
            if abs_diff > self.tolerance.abs() {
                return DeadbandState::Exceeded;
            } else if abs_diff > self.tolerance.abs() * 0.8 {
                return DeadbandState::Approaching;
            }
            return DeadbandState::Normal;
        }

        let relative_diff = (value - self.center) / self.center.abs();

        // Check direction filter
        match self.direction {
            DeadbandDirection::Above => {
                if relative_diff < 0.0 {
                    return DeadbandState::Normal; // Below center is always normal for one-sided above
                }
            }
            DeadbandDirection::Below => {
                if relative_diff > 0.0 {
                    return DeadbandState::Normal; // Above center is always normal for conservation
                }
            }
            DeadbandDirection::Both => {}
        }

        let abs_rel = relative_diff.abs();

        if abs_rel > self.tolerance.abs() {
            DeadbandState::Exceeded
        } else if abs_rel > self.tolerance.abs() * 0.8 {
            DeadbandState::Approaching
        } else {
            DeadbandState::Normal
        }
    }
}
