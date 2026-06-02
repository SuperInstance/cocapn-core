use std::time::Instant;

use crate::device::DeviceTier;

/// Error type for agent operations.
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("sensor error: {0}")]
    SensorError(String),
    #[error("actuation failed: {0}")]
    ActuationError(String),
    #[error("handoff failed: {0}")]
    HandoffError(String),
    #[error("no fallback available")]
    NoFallback,
}

/// A reading from a sensor.
#[derive(Debug, Clone)]
pub struct SensorReading {
    pub sensor_id: String,
    pub value: f64,
    pub timestamp: Instant,
    pub tier: DeviceTier,
}

impl SensorReading {
    pub fn new(sensor_id: impl Into<String>, value: f64, tier: DeviceTier) -> Self {
        Self {
            sensor_id: sensor_id.into(),
            value,
            timestamp: Instant::now(),
            tier,
        }
    }
}

/// Emergency actions that bypass normal decision flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmergencyAction {
    Shutdown,
    Surface,
    Mayday,
}

/// A decision made by the agent.
#[derive(Debug, Clone)]
pub enum Decision {
    Hold,
    Adjust(f64),
    Escalate(String),
    Emergency(EmergencyAction),
}

/// An action to be executed.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    NoOp,
    SetValue(f64),
    SendMessage(String),
    TriggerHandoff(String),
}

/// The co-captain agent trait.
/// Each device implements this to participate in the framework.
pub trait CoCaptain: Send + Sync {
    fn name(&self) -> &str;
    fn tier(&self) -> DeviceTier;
    fn sense(&mut self, reading: SensorReading) -> Result<(), AgentError>;
    fn decide(&mut self) -> Decision;
    fn act(&mut self, decision: Decision) -> Result<Action, AgentError>;
    fn fallback(&self) -> Option<Box<dyn CoCaptain>>;
}
