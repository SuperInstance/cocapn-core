#![deny(unsafe_code)]

pub mod agent;
pub mod deadband;
pub mod device;
pub mod handoff;
pub mod pushdown;
pub mod stripe;

// Re-export key types
pub use agent::{Action, AgentError, CoCaptain, Decision, EmergencyAction, SensorReading};
pub use deadband::{Deadband, DeadbandDirection, DeadbandState};
pub use device::{Capability, Device, DeviceTier};
pub use handoff::{Handoff, HandoffState};
pub use pushdown::{push_down, ComputeClass, FeatureSpec, FeatureStatus, PushedFeature};
pub use stripe::{Stripe, StripeEvent, StripeLayer};
