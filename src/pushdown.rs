use crate::device::DeviceTier;

/// Classification of compute requirements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ComputeClass {
    Trivial,
    Light,
    Medium,
    Heavy,
    Massive,
}

/// A feature that may be pushed down to a lower tier.
#[derive(Debug, Clone)]
pub struct FeatureSpec {
    pub name: String,
    pub min_tier: DeviceTier,
    pub memory_bytes: usize,
    pub compute_estimate: ComputeClass,
}

/// Result of pushing a feature down.
#[derive(Debug, Clone)]
pub enum FeatureStatus {
    /// Feature can run at this tier
    Available,
    /// Feature is degraded (running at lower quality)
    Degraded,
    /// Feature cannot run at this tier — dropped
    Dropped,
}

impl PartialEq for FeatureStatus {
    fn eq(&self, other: &Self) -> bool {
        core::mem::discriminant(self) == core::mem::discriminant(other)
    }
}

impl Eq for FeatureStatus {}

/// A feature that has been evaluated for push-down.
#[derive(Debug, Clone)]
pub struct PushedFeature {
    pub spec: FeatureSpec,
    pub status: FeatureStatus,
    pub running_at: DeviceTier,
}

/// Tier capacity for compute and memory (rough estimates).
fn tier_capacity(tier: DeviceTier) -> (ComputeClass, usize) {
    match tier {
        DeviceTier::Reflex => (ComputeClass::Trivial, 520_000),         // 520KB
        DeviceTier::Backbone => (ComputeClass::Light, 4_000_000_000),    // 4GB
        DeviceTier::Cortex => (ComputeClass::Heavy, 32_000_000_000),     // 32GB
        DeviceTier::Cloud => (ComputeClass::Massive, usize::MAX),
    }
}

/// Evaluate which features can run at the available tier.
/// Features that require a higher tier are either degraded or dropped.
pub fn push_down(features: &[FeatureSpec], available_tier: DeviceTier) -> Vec<PushedFeature> {
    let (max_compute, max_memory) = tier_capacity(available_tier);

    features
        .iter()
        .map(|spec| {
            let tier_ok = spec.min_tier <= available_tier;
            let memory_ok = spec.memory_bytes <= max_memory;
            let compute_ok = spec.compute_estimate <= max_compute;

            let status = if tier_ok && memory_ok && compute_ok {
                FeatureStatus::Available
            } else if available_tier >= DeviceTier::Backbone
                && spec.compute_estimate <= max_compute
                && spec.memory_bytes <= max_memory
            {
                // Can run degraded if we have at least Backbone and compute/memory fits
                FeatureStatus::Degraded
            } else {
                FeatureStatus::Dropped
            };

            PushedFeature {
                spec: spec.clone(),
                status,
                running_at: available_tier,
            }
        })
        .collect()
}
