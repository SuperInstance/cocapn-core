use std::time::Duration;

/// State of a handoff between devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandoffState {
    Stable,
    FadingOut,
    Crossfading,
    FadingIn,
    Complete,
}

/// A crossfade transition between two devices.
#[derive(Debug, Clone)]
pub struct Handoff {
    pub from_device: String,
    pub to_device: String,
    pub state: HandoffState,
    pub transition_duration: Duration,
    elapsed: Duration,
    cancelled: bool,
}

impl Handoff {
    pub fn new(
        from_device: impl Into<String>,
        to_device: impl Into<String>,
        transition_duration: Duration,
    ) -> Self {
        Self {
            from_device: from_device.into(),
            to_device: to_device.into(),
            state: HandoffState::Stable,
            transition_duration,
            elapsed: Duration::ZERO,
            cancelled: false,
        }
    }

    /// Begin the handoff.
    pub fn begin(&mut self) -> Result<(), String> {
        if self.state != HandoffState::Stable {
            return Err(format!("Cannot begin handoff from state {:?}", self.state));
        }
        if self.cancelled {
            return Err("Handoff was cancelled".into());
        }
        self.state = HandoffState::FadingOut;
        self.elapsed = Duration::ZERO;
        Ok(())
    }

    /// Advance the handoff by a time delta. Returns progress 0.0..1.0.
    pub fn progress(&mut self, delta: Duration) -> f64 {
        if self.cancelled || self.state == HandoffState::Complete || self.state == HandoffState::Stable {
            return self.state_to_progress();
        }

        self.elapsed += delta;
        let total = self.transition_duration.as_secs_f64();
        let p = if total > 0.0 {
            (self.elapsed.as_secs_f64() / total).clamp(0.0, 1.0)
        } else {
            1.0
        };

        // State transitions based on progress
        if p < 0.33 {
            self.state = HandoffState::FadingOut;
        } else if p < 0.66 {
            self.state = HandoffState::Crossfading;
        } else if p < 1.0 {
            self.state = HandoffState::FadingIn;
        } else {
            self.state = HandoffState::Complete;
        }

        p
    }

    /// Cancel/reverse the transition.
    pub fn cancel(&mut self) -> Result<(), String> {
        if self.state == HandoffState::Complete {
            return Err("Cannot cancel a completed handoff".into());
        }
        if self.state == HandoffState::Stable {
            return Err("Nothing to cancel".into());
        }
        self.cancelled = true;
        self.state = HandoffState::Stable;
        self.elapsed = Duration::ZERO;
        Ok(())
    }

    fn state_to_progress(&self) -> f64 {
        match self.state {
            HandoffState::Stable => 0.0,
            HandoffState::FadingOut => 0.0,
            HandoffState::Crossfading => 0.5,
            HandoffState::FadingIn => 0.8,
            HandoffState::Complete => 1.0,
        }
    }

    pub fn is_complete(&self) -> bool {
        self.state == HandoffState::Complete
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}
