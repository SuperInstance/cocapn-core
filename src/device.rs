use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::time::Instant;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Device capability types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Capability {
    Sense,
    Act,
    Route,
    Predict,
    Train,
    Communicate,
}

/// Device tier — determines compute power and role in the hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum DeviceTier {
    /// ESP32/Arduino — hardcoded responses, no thinking
    Reflex,
    /// Raspberry Pi — routes signals, runs local agent
    Backbone,
    /// Jetson/Workstation — perception, models, training
    Cortex,
    /// Remote — heavy compute, APIs, training
    Cloud,
}

/// A device in the CoCapn network.
#[derive(Debug, Clone)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub tier: DeviceTier,
    pub capabilities: HashSet<Capability>,
    pub last_seen: Instant,
}

impl Device {
    pub fn new(id: impl Into<String>, name: impl Into<String>, tier: DeviceTier) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            tier,
            capabilities: HashSet::new(),
            last_seen: Instant::now(),
        }
    }

    pub fn can(&self, cap: Capability) -> bool {
        self.capabilities.contains(&cap)
    }

    pub fn tier(&self) -> DeviceTier {
        self.tier
    }

    pub fn with_capabilities(mut self, caps: impl IntoIterator<Item = Capability>) -> Self {
        self.capabilities = caps.into_iter().collect();
        self
    }
}

impl Hash for Device {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl PartialEq for Device {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Device {}
