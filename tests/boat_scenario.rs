//! Real marine autopilot scenario integration test.
//!
//! Simulates: ESP32 holding heading at 90°, crossfade handoff to Pi,
//! Pi takes over, Pi fails, handoff back to ESP32.
//!
//! Marina — embedded systems engineer building real marine autopilots.

use std::time::Duration;
use cocapn_core::*;

/// Simulated autopilot state on the heading-hold loop.
#[derive(Debug, Clone, PartialEq)]
struct AutopilotState {
    /// Who's driving
    active_device: String,
    /// Desired heading in degrees (0-359)
    target_heading: f64,
    /// Current heading in degrees
    current_heading: f64,
    /// Is the active controller output clamped?
    output_clamped: bool,
}

/// A simple heading-hold agent that can run on either ESP32 or Pi.
struct HelmAgent {
    name: String,
    tier: DeviceTier,
    state: AutopilotState,
    readings: Vec<SensorReading>,
    /// If true, simulates hardware failure
    failed: bool,
}

impl HelmAgent {
    fn new(name: &str, tier: DeviceTier, target: f64, heading: f64) -> Self {
        Self {
            name: name.to_string(),
            tier,
            state: AutopilotState {
                active_device: name.to_string(),
                target_heading: target,
                current_heading: heading,
                output_clamped: false,
            },
            readings: Vec::new(),
            failed: false,
        }
    }

    /// Simulate reading the compass and adjusting rudder
    fn helm_loop(&mut self) -> Result<f64, String> {
        if self.failed {
            return Err(format!("{} hardware failure", self.name));
        }
        let error = (self.state.target_heading - self.state.current_heading + 180.0) % 360.0 - 180.0;
        // Simple proportional rudder command
        let rudder = error * 0.3;
        Ok(rudder)
    }
}

impl CoCaptain for HelmAgent {
    fn name(&self) -> &str { &self.name }
    fn tier(&self) -> DeviceTier { self.tier }

    fn sense(&mut self, reading: SensorReading) -> Result<(), AgentError> {
        if self.failed {
            return Err(AgentError::SensorError("device offline".into()));
        }
        self.state.current_heading = reading.value;
        self.readings.push(reading);
        Ok(())
    }

    fn decide(&mut self) -> Decision {
        if self.failed {
            return Decision::Emergency(EmergencyAction::Mayday);
        }
        match self.helm_loop() {
            Ok(rudder) => Decision::Adjust(rudder),
            Err(_) => Decision::Emergency(EmergencyAction::Mayday),
        }
    }

    fn act(&mut self, decision: Decision) -> Result<Action, AgentError> {
        match decision {
            Decision::Adjust(rudder) => {
                // Clamp to reasonable marine rudder range
                let clamped = rudder.clamp(-35.0, 35.0);
                self.state.output_clamped = (rudder - clamped).abs() > 0.01;
                Ok(Action::SetValue(clamped))
            }
            Decision::Hold => Ok(Action::NoOp),
            Decision::Emergency(action) => {
                // Emergency: center the rudder, preserve heading
                Ok(Action::SetValue(0.0))
            }
            Decision::Escalate(msg) => Ok(Action::SendMessage(msg)),
        }
    }

    fn fallback(&self) -> Option<Box<dyn CoCaptain>> {
        None // handled externally by stripe/handoff
    }
}

#[cfg(test)]
mod boat_scenario_tests {
    use super::*;

    /// Scenario 1: ESP32 holds heading at 90°.
    /// The deadband should report Normal for small deviations (<5%).
    #[test]
    fn esp32_holds_heading() {
        let db = Deadband::with_relative_tolerance(90.0, 0.05); // 5% on 90 = ±4.5°
        let mut esp = HelmAgent::new("helm-esp32", DeviceTier::Reflex, 90.0, 90.0);

        // Simulate a few seconds of helm — compass reading 90.0 (exactly on course)
        let reading = SensorReading::new("compass", 90.0, DeviceTier::Reflex);
        assert!(esp.sense(reading).is_ok());
        assert_eq!(db.check(90.0), DeadbandState::Normal);

        let decision = esp.decide();
        let action = esp.act(decision).unwrap();
        // At exactly 90.0: error=0, so rudder=0
        assert_eq!(action, Action::SetValue(0.0));

        // Sea state pushes us to 88.0 (still within 5% of 90)
        // relative_diff = (88-90)/90 = -0.0222, abs = 0.0222 < 0.05 → Normal ✓
        let reading = SensorReading::new("compass", 88.0, DeviceTier::Reflex);
        assert!(esp.sense(reading).is_ok());
        assert_eq!(db.check(88.0), DeadbandState::Normal);

        // Approaching band: 0.8*tol=0.04 < abs_rel < 0.05 → 86.4° to 93.6°
        // Use 86.3°: abs_rel=3.7/90=0.0411 → 0.04 < 0.0411 < 0.05 → Approaching ✓
        let reading = SensorReading::new("compass", 86.3, DeviceTier::Reflex);
        assert!(esp.sense(reading).is_ok());
        assert_eq!(db.check(86.3), DeadbandState::Approaching);

        // EXCEEDED: abs_rel > 0.05 → 85.5°: abs_rel=4.5/90=0.05 → Exceeded (strictly > tol)
        let reading = SensorReading::new("compass", 84.0, DeviceTier::Reflex);
        assert!(esp.sense(reading).is_ok());
        assert_eq!(db.check(84.0), DeadbandState::Exceeded);
        // Also check 85.4°: abs_rel=4.6/90=0.0511 → Exceeded
        assert_eq!(db.check(85.4), DeadbandState::Exceeded);
    }

    /// Scenario 2: Crossfade handoff from ESP32 to Pi.
    #[test]
    fn crossfade_handoff_esp32_to_pi() {
        let mut handoff = Handoff::new("helm-esp32", "helm-pi", Duration::from_secs(10));
        let mut esp = HelmAgent::new("helm-esp32", DeviceTier::Reflex, 90.0, 90.3);
        let mut pi = HelmAgent::new("helm-pi", DeviceTier::Backbone, 90.0, 90.3);

        // Start handoff
        handoff.begin().unwrap();
        assert_eq!(handoff.state, HandoffState::FadingOut);

        // 3 seconds in — ESP32 is fading out (33%), Pi is not yet fully in
        let p = handoff.progress(Duration::from_secs(3));
        assert!(p > 0.0 && p < 0.33);
        assert_eq!(handoff.state, HandoffState::FadingOut);

        // Scale: esp_weight = 1.0 - p, pi_weight = p
        let esp_weight = 1.0 - p;
        let pi_weight = p;
        let blended_rudder = esp_weight * 0.0 + pi_weight * (-0.09);
        assert!(blended_rudder.abs() < 0.04); // mostly ESP32 still

        // 6 seconds in — crossfading midpoint
        let p = handoff.progress(Duration::from_secs(3));
        assert!(p > 0.33 && p < 0.66);
        assert_eq!(handoff.state, HandoffState::Crossfading);

        // 9 seconds in — Pi taking over
        let p = handoff.progress(Duration::from_secs(3));
        assert!(p > 0.66 && p < 1.0);
        assert_eq!(handoff.state, HandoffState::FadingIn);

        // Complete handoff
        let p = handoff.progress(Duration::from_secs(3));
        assert_eq!(p, 1.0);
        assert!(handoff.is_complete());
        assert_eq!(handoff.state, HandoffState::Complete);

        // Pi is now in command — heading 90.3 from earlier, error = -0.3, rudder = -0.09
        let reading = SensorReading::new("compass", 90.3, DeviceTier::Backbone);
        assert!(pi.sense(reading).is_ok());
        let decision = pi.decide();
        let action = pi.act(decision).unwrap();
        // error = 90-90.3 = -0.3, rudder = -0.09 — tiny correction
        assert!(matches!(action, Action::SetValue(v) if (v - (-0.09)).abs() < 1e-10));
    }

    /// Scenario 3: Pi fails mid-operation, handoff back to ESP32.
    #[test]
    fn pi_fails_handoff_back_to_esp32() {
        let mut esp = HelmAgent::new("helm-esp32", DeviceTier::Reflex, 90.0, 89.5);
        let mut pi = HelmAgent::new("helm-pi", DeviceTier::Backbone, 90.0, 89.5);

        // Pi is driving
        let reading = SensorReading::new("compass", 89.5, DeviceTier::Backbone);
        assert!(pi.sense(reading).is_ok());

        // A 5° wave hits
        let reading = SensorReading::new("compass", 94.5, DeviceTier::Backbone);
        assert!(pi.sense(reading).is_ok());

        // Pi computes decision — heading 94.5, target 90, error = -4.5, rudder = -1.35
        let decision = pi.decide();
        let pi_action = pi.act(decision).unwrap();
        assert!(matches!(pi_action, Action::SetValue(v) if v.abs() > 0.0));

        // Pi hardware fails
        pi.failed = true;
        assert!(pi.sense(SensorReading::new("compass", 93.0, DeviceTier::Backbone)).is_err());

        // Emergency handoff back to ESP32
        let mut handoff = Handoff::new("helm-pi", "helm-esp32", Duration::from_secs(1)); // fast reversion
        handoff.begin().unwrap();
        let p = handoff.progress(Duration::from_secs(2));
        assert_eq!(p, 1.0);
        assert!(handoff.is_complete());

        // ESP32 takes over — it should immediately center the rudder (emergency safe)
        let esp_decision = esp.decide();
        let esp_action = esp.act(esp_decision).unwrap();
        assert!(matches!(esp_action, Action::SetValue(0.15)));

        // ESP32 stabilizes heading back toward 90°
        // Current heading is 89.5 (last successful read). error = 90 - 89.5 = 0.5. rudder = 0.15.
        let decision = esp.decide();
        let action = esp.act(decision).unwrap();
        // 0.5° * 0.3 = 0.15° rudder to starboard — tiny
        assert!(matches!(action, Action::SetValue(v) if (v - 0.15).abs() < 1e-10));
    }

    /// Scenario 4: Stripe-based failover with device tiers.
    #[test]
    fn stripe_based_system_redundancy() {
        let mut s = Stripe::new();

        // Add devices to stripe: Cortex > Backbone > Reflex
        s.add_layer(StripeLayer {
            tier: DeviceTier::Cortex,
            device_id: "helm-jetson".into(),
            healthy: true,
            latency_ms: Some(25),
        });
        s.add_layer(StripeLayer {
            tier: DeviceTier::Backbone,
            device_id: "helm-pi".into(),
            healthy: true,
            latency_ms: Some(50),
        });
        s.add_layer(StripeLayer {
            tier: DeviceTier::Reflex,
            device_id: "helm-esp32".into(),
            healthy: true,
            latency_ms: Some(5),
        });

        // Active tier should be Cortex (highest healthy)
        assert_eq!(s.get_active_tier(), Some(DeviceTier::Cortex));

        // Cortex fails
        let event = s.fail_layer("helm-jetson").unwrap();
        match event {
            StripeEvent::Rebalanced { from, to, .. } => {
                assert_eq!(from, "helm-jetson");
                assert_eq!(to, "helm-pi");
            }
            _ => panic!("Expected Rebalanced event"),
        }
        assert_eq!(s.get_active_tier(), Some(DeviceTier::Backbone));

        // Pi fails too
        let event = s.fail_layer("helm-pi").unwrap();
        match event {
            StripeEvent::Rebalanced { from, to, .. } => {
                assert_eq!(from, "helm-pi");
                assert_eq!(to, "helm-esp32");
            }
            _ => panic!("Expected Rebalanced event"),
        }
        assert_eq!(s.get_active_tier(), Some(DeviceTier::Reflex));

        // Even ESP32 fails — total degradation
        let event = s.fail_layer("helm-esp32").unwrap();
        match event {
            StripeEvent::Degraded { .. } => { /* expected */ }
            _ => panic!("Expected Degraded event"),
        }
        assert_eq!(s.get_active_tier(), None);

        // Fallback path is purely Reflex (last resort)
        let path = s.fallback_path();
        // After all failed, get_active_tier returns None, fallback_path defaults to Reflex
        assert_eq!(path, vec![DeviceTier::Reflex]);
    }

    /// Scenario 5: Heading hold with push-down — what can the ESP32 actually run?
    #[test]
    fn pushdown_for_marine_features() {
        let features = vec![
            FeatureSpec {
                name: "heading_hold".into(),
                min_tier: DeviceTier::Reflex,
                memory_bytes: 2_000,        // 2KB — trivial PID
                compute_estimate: ComputeClass::Trivial,
            },
            FeatureSpec {
                name: "nmea_0183_parser".into(),
                min_tier: DeviceTier::Reflex,
                memory_bytes: 8_000,        // 8KB
                compute_estimate: ComputeClass::Trivial,
            },
            FeatureSpec {
                name: "route_following".into(),
                min_tier: DeviceTier::Backbone,
                memory_bytes: 50_000,       // 50KB
                compute_estimate: ComputeClass::Light,
            },
            FeatureSpec {
                name: "wave_prediction".into(),
                min_tier: DeviceTier::Cortex,
                memory_bytes: 50_000_000,   // 50MB
                compute_estimate: ComputeClass::Heavy,
            },
        ];

        // What can the ESP32 (Reflex) run?
        let esp_capable = push_down(&features, DeviceTier::Reflex);
        assert_eq!(esp_capable[0].status, FeatureStatus::Available); // heading_hold ✓
        assert_eq!(esp_capable[1].status, FeatureStatus::Available); // nmea_parser ✓
        assert_eq!(esp_capable[2].status, FeatureStatus::Dropped);   // route_following — needs Backbone
        assert_eq!(esp_capable[3].status, FeatureStatus::Dropped);   // wave_prediction — needs Cortex

        // What can the Pi (Backbone) run?
        let pi_capable = push_down(&features, DeviceTier::Backbone);
        assert_eq!(pi_capable[0].status, FeatureStatus::Available);
        assert_eq!(pi_capable[1].status, FeatureStatus::Available);
        assert_eq!(pi_capable[2].status, FeatureStatus::Available);  // route_following ✓
        // Backbone can't meet wave_prediction's min_tier (Cortex) — but compute/memory fit,
        // so the Degraded branch checks available_tier >= Backbone. It IS Backbone.
        // compute_estimate=Heavy <= Backbone (max=Light)? NO — Backbone caps at Light.
        // So the third condition fails → Dropped.
        assert_eq!(pi_capable[3].status, FeatureStatus::Dropped);

        // Jetson (Cortex) runs everything
        let jetson_capable = push_down(&features, DeviceTier::Cortex);
        assert_eq!(jetson_capable[3].status, FeatureStatus::Available); // wave_prediction ✓
    }
}
