use crate::device::DeviceTier;

/// A single layer in the compute stripe.
#[derive(Debug, Clone)]
pub struct StripeLayer {
    pub tier: DeviceTier,
    pub device_id: String,
    pub healthy: bool,
    pub latency_ms: Option<u64>,
}

/// Events emitted by the stripe during rebalancing.
#[derive(Debug, Clone)]
pub enum StripeEvent {
    LayerAdded(StripeLayer),
    LayerFailed(String),
    Rebalanced {
        from: String,
        to: String,
        reason: String,
    },
    Degraded {
        remaining_tiers: Vec<DeviceTier>,
    },
}

/// The compute stripe — an ordered chain of devices from highest to lowest tier.
#[derive(Debug, Clone, Default)]
pub struct Stripe {
    layers: Vec<StripeLayer>,
}

impl Stripe {
    pub fn new() -> Self {
        Self { layers: Vec::new() }
    }

    pub fn add_layer(&mut self, layer: StripeLayer) -> StripeEvent {
        let event = StripeEvent::LayerAdded(layer.clone());
        self.layers.push(layer);
        // Keep sorted by tier (highest first)
        self.layers.sort_by_key(|b| std::cmp::Reverse(b.tier));
        event
    }

    pub fn remove_layer(&mut self, device_id: &str) -> Option<StripeEvent> {
        let idx = self.layers.iter().position(|l| l.device_id == device_id)?;
        self.layers.remove(idx);
        Some(StripeEvent::LayerFailed(device_id.to_string()))
    }

    /// Mark a layer as unhealthy and rebalance.
    pub fn fail_layer(&mut self, device_id: &str) -> Option<StripeEvent> {
        let layer = self.layers.iter_mut().find(|l| l.device_id == device_id)?;
        let old_tier = layer.tier;
        layer.healthy = false;

        let old_device = device_id.to_string();
        let new_device = self
            .layers
            .iter()
            .find(|l| l.healthy)
            .map(|l| l.device_id.clone());

        match new_device {
            Some(to) => Some(StripeEvent::Rebalanced {
                from: old_device,
                to,
                reason: format!("{:?} layer failed", old_tier),
            }),
            None => Some(StripeEvent::Degraded {
                remaining_tiers: self.get_active_tiers(),
            }),
        }
    }

    /// Get the highest healthy tier currently active.
    pub fn get_active_tier(&self) -> Option<DeviceTier> {
        self.layers
            .iter()
            .find(|l| l.healthy)
            .map(|l| l.tier)
    }

    pub fn get_active_tiers(&self) -> Vec<DeviceTier> {
        self.layers
            .iter()
            .filter(|l| l.healthy)
            .map(|l| l.tier)
            .collect()
    }

    /// Rebalance — find highest healthy tier and emit events.
    pub fn rebalance(&mut self) -> Option<StripeEvent> {
        let healthy: Vec<_> = self.layers.iter().filter(|l| l.healthy).collect();
        if healthy.is_empty() {
            return Some(StripeEvent::Degraded {
                remaining_tiers: vec![],
            });
        }
        // Already sorted by tier desc; just confirm the top
        let top = healthy.first()?;
        // No previous to rebalance from in a simple rebalance
        let _ = top;
        None // No event needed if structure hasn't changed
    }

    /// Fallback path from current active tier down to Reflex.
    pub fn fallback_path(&self) -> Vec<DeviceTier> {
        let active = self.get_active_tier().unwrap_or(DeviceTier::Reflex);
        let all_tiers = [
            DeviceTier::Cloud,
            DeviceTier::Cortex,
            DeviceTier::Backbone,
            DeviceTier::Reflex,
        ];
        all_tiers
            .iter()
            .filter(|&&t| t <= active)
            .copied()
            .collect()
    }

    pub fn layers(&self) -> &[StripeLayer] {
        &self.layers
    }

    pub fn healthy_layers(&self) -> Vec<&StripeLayer> {
        self.layers.iter().filter(|l| l.healthy).collect()
    }
}
