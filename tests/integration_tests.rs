use cocapn_core::*;

mod device_tests {
    use super::*;

    #[test]
    fn device_new_basic() {
        let d = Device::new("esp-01", "Temperature Sensor", DeviceTier::Reflex);
        assert_eq!(d.id, "esp-01");
        assert_eq!(d.name, "Temperature Sensor");
        assert_eq!(d.tier(), DeviceTier::Reflex);
    }

    #[test]
    fn device_capabilities() {
        let d = Device::new("rpi-01", "Router", DeviceTier::Backbone)
            .with_capabilities([Capability::Route, Capability::Communicate]);
        assert!(d.can(Capability::Route));
        assert!(d.can(Capability::Communicate));
        assert!(!d.can(Capability::Predict));
    }

    #[test]
    fn device_tier_ordering() {
        assert!(DeviceTier::Reflex < DeviceTier::Backbone);
        assert!(DeviceTier::Backbone < DeviceTier::Cortex);
        assert!(DeviceTier::Cortex < DeviceTier::Cloud);
    }

    #[test]
    fn device_equality_by_id() {
        let a = Device::new("x", "A", DeviceTier::Reflex);
        let b = Device::new("x", "B", DeviceTier::Cloud);
        assert_eq!(a, b); // same id
    }

    #[test]
    fn device_empty_capabilities() {
        let d = Device::new("bare", "Bare", DeviceTier::Reflex);
        assert!(!d.can(Capability::Sense));
    }
}

mod deadband_tests {
    use super::*;

    #[test]
    fn deadband_normal_within_tolerance() {
        let db = Deadband::with_relative_tolerance(100.0, 0.1);
        assert_eq!(db.check(95.0), DeadbandState::Normal);
        assert_eq!(db.check(105.0), DeadbandState::Normal);
    }

    #[test]
    fn deadband_exceeded_outside_tolerance() {
        let db = Deadband::with_relative_tolerance(100.0, 0.1);
        assert_eq!(db.check(89.0), DeadbandState::Exceeded);
        assert_eq!(db.check(112.0), DeadbandState::Exceeded);
    }

    #[test]
    fn deadband_approaching_edge() {
        let db = Deadband::with_relative_tolerance(100.0, 0.1);
        // 94% of tolerance = approaching (0.094 > 0.08)
        assert_eq!(db.check(90.6), DeadbandState::Approaching);
        assert_eq!(db.check(109.4), DeadbandState::Approaching);
    }

    #[test]
    fn deadband_one_sided_below_conservation() {
        let db = Deadband::new(100.0, 0.1, DeadbandDirection::Below);
        // Above center is always normal for conservation
        assert_eq!(db.check(150.0), DeadbandState::Normal);
        // Below threshold is exceeded
        assert_eq!(db.check(80.0), DeadbandState::Exceeded);
    }

    #[test]
    fn deadband_one_sided_above() {
        let db = Deadband::new(100.0, 0.1, DeadbandDirection::Above);
        // Below center is always normal
        assert_eq!(db.check(50.0), DeadbandState::Normal);
        // Above threshold is exceeded
        assert_eq!(db.check(120.0), DeadbandState::Exceeded);
    }

    #[test]
    fn deadband_zero_center() {
        let db = Deadband::new(0.0, 5.0, DeadbandDirection::Both);
        assert_eq!(db.check(3.0), DeadbandState::Normal);
        assert_eq!(db.check(6.0), DeadbandState::Exceeded);
    }

    #[test]
    fn deadband_negative_center() {
        let db = Deadband::with_relative_tolerance(-100.0, 0.1);
        assert_eq!(db.check(-95.0), DeadbandState::Normal);
        assert_eq!(db.check(-112.0), DeadbandState::Exceeded);
    }
}

mod stripe_tests {
    use super::*;

    #[test]
    fn stripe_add_layers_sorted() {
        let mut s = Stripe::new();
        s.add_layer(StripeLayer {
            tier: DeviceTier::Reflex,
            device_id: "esp".into(),
            healthy: true,
            latency_ms: Some(5),
        });
        s.add_layer(StripeLayer {
            tier: DeviceTier::Cortex,
            device_id: "jetson".into(),
            healthy: true,
            latency_ms: Some(20),
        });

        // Cortex should be first (highest)
        assert_eq!(s.layers()[0].tier, DeviceTier::Cortex);
        assert_eq!(s.layers()[1].tier, DeviceTier::Reflex);
    }

    #[test]
    fn stripe_active_tier() {
        let mut s = Stripe::new();
        assert_eq!(s.get_active_tier(), None);

        s.add_layer(StripeLayer {
            tier: DeviceTier::Backbone,
            device_id: "rpi".into(),
            healthy: true,
            latency_ms: None,
        });
        assert_eq!(s.get_active_tier(), Some(DeviceTier::Backbone));
    }

    #[test]
    fn stripe_fail_layer_rebalances() {
        let mut s = Stripe::new();
        s.add_layer(StripeLayer {
            tier: DeviceTier::Cortex,
            device_id: "jetson".into(),
            healthy: true,
            latency_ms: Some(10),
        });
        s.add_layer(StripeLayer {
            tier: DeviceTier::Backbone,
            device_id: "rpi".into(),
            healthy: true,
            latency_ms: Some(50),
        });

        let event = s.fail_layer("jetson").unwrap();
        match event {
            StripeEvent::Rebalanced { from, to, reason } => {
                assert_eq!(from, "jetson");
                assert_eq!(to, "rpi");
                assert!(reason.contains("Cortex"));
            }
            _ => panic!("Expected Rebalanced event"),
        }
        assert_eq!(s.get_active_tier(), Some(DeviceTier::Backbone));
    }

    #[test]
    fn stripe_all_layers_failed() {
        let mut s = Stripe::new();
        s.add_layer(StripeLayer {
            tier: DeviceTier::Reflex,
            device_id: "esp".into(),
            healthy: true,
            latency_ms: None,
        });

        let event = s.fail_layer("esp").unwrap();
        match event {
            StripeEvent::Degraded { remaining_tiers } => {
                assert!(remaining_tiers.is_empty());
            }
            _ => panic!("Expected Degraded event"),
        }
        assert_eq!(s.get_active_tier(), None);
    }

    #[test]
    fn stripe_fallback_path() {
        let mut s = Stripe::new();
        s.add_layer(StripeLayer {
            tier: DeviceTier::Cortex,
            device_id: "jetson".into(),
            healthy: true,
            latency_ms: None,
        });
        s.add_layer(StripeLayer {
            tier: DeviceTier::Backbone,
            device_id: "rpi".into(),
            healthy: true,
            latency_ms: None,
        });

        let path = s.fallback_path();
        assert_eq!(
            path,
            vec![DeviceTier::Cortex, DeviceTier::Backbone, DeviceTier::Reflex]
        );
    }

    #[test]
    fn stripe_empty_fallback() {
        let s = Stripe::new();
        let path = s.fallback_path();
        assert_eq!(path, vec![DeviceTier::Reflex]);
    }

    #[test]
    fn stripe_remove_layer() {
        let mut s = Stripe::new();
        s.add_layer(StripeLayer {
            tier: DeviceTier::Reflex,
            device_id: "esp".into(),
            healthy: true,
            latency_ms: None,
        });
        let event = s.remove_layer("esp").unwrap();
        match event {
            StripeEvent::LayerFailed(id) => assert_eq!(id, "esp"),
            _ => panic!("Expected LayerFailed"),
        }
        assert!(s.layers().is_empty());
    }
}

mod handoff_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn handoff_full_transition() {
        let mut h = Handoff::new("jetson", "rpi", Duration::from_secs(10));
        assert_eq!(h.state, HandoffState::Stable);

        h.begin().unwrap();
        assert_eq!(h.state, HandoffState::FadingOut);

        let p = h.progress(Duration::from_secs(4));
        assert!(p > 0.3);
        assert_eq!(h.state, HandoffState::Crossfading);

        let p = h.progress(Duration::from_secs(4));
        assert!(p > 0.6);
        assert_eq!(h.state, HandoffState::FadingIn);

        let p = h.progress(Duration::from_secs(4));
        assert_eq!(p, 1.0);
        assert!(h.is_complete());
    }

    #[test]
    fn handoff_cancel_midway() {
        let mut h = Handoff::new("jetson", "rpi", Duration::from_secs(10));
        h.begin().unwrap();
        h.progress(Duration::from_secs(2));
        h.cancel().unwrap();
        assert!(h.is_cancelled());
        assert_eq!(h.state, HandoffState::Stable);
    }

    #[test]
    fn handoff_cannot_begin_twice() {
        let mut h = Handoff::new("a", "b", Duration::from_secs(5));
        h.begin().unwrap();
        assert!(h.begin().is_err());
    }

    #[test]
    fn handoff_cannot_cancel_complete() {
        let mut h = Handoff::new("a", "b", Duration::from_secs(1));
        h.begin().unwrap();
        h.progress(Duration::from_secs(5));
        assert!(h.is_complete());
        assert!(h.cancel().is_err());
    }

    #[test]
    fn handoff_cannot_cancel_stable() {
        let h = Handoff::new("a", "b", Duration::from_secs(1));
        let mut h = h;
        assert!(h.cancel().is_err());
    }

    #[test]
    fn handoff_zero_duration() {
        let mut h = Handoff::new("a", "b", Duration::from_secs(0));
        h.begin().unwrap();
        let p = h.progress(Duration::from_millis(1));
        assert_eq!(p, 1.0);
        assert!(h.is_complete());
    }

    #[test]
    fn handoff_cannot_begin_cancelled() {
        let mut h = Handoff::new("a", "b", Duration::from_secs(5));
        h.begin().unwrap();
        h.cancel().unwrap();
        assert!(h.begin().is_err());
    }
}

mod pushdown_tests {
    use super::*;

    #[test]
    fn pushdown_all_available_at_cloud() {
        let features = vec![
            FeatureSpec {
                name: "vision".into(),
                min_tier: DeviceTier::Cortex,
                memory_bytes: 1_000_000,
                compute_estimate: ComputeClass::Heavy,
            },
        ];
        let result = push_down(&features, DeviceTier::Cloud);
        assert_eq!(result[0].status, FeatureStatus::Available);
    }

    #[test]
    fn pushdown_drops_heavy_at_reflex() {
        let features = vec![FeatureSpec {
            name: "llm".into(),
            min_tier: DeviceTier::Cloud,
            memory_bytes: 8_000_000_000,
            compute_estimate: ComputeClass::Massive,
        }];
        let result = push_down(&features, DeviceTier::Reflex);
        assert_eq!(result[0].status, FeatureStatus::Dropped);
    }

    #[test]
    fn pushdown_cortex_features_at_backbone() {
        let features = vec![FeatureSpec {
            name: "detector".into(),
            min_tier: DeviceTier::Cortex,
            memory_bytes: 100_000,
            compute_estimate: ComputeClass::Light,
        }];
        // Cortex feature at Backbone — tier is lower but compute/memory fits
        let result = push_down(&features, DeviceTier::Backbone);
        // Should be Degraded since tier doesn't match but Backbone can run it
        assert!(matches!(
            result[0].status,
            FeatureStatus::Degraded | FeatureStatus::Dropped
        ));
    }

    #[test]
    fn pushdown_trivial_at_reflex() {
        let features = vec![FeatureSpec {
            name: "blink".into(),
            min_tier: DeviceTier::Reflex,
            memory_bytes: 100,
            compute_estimate: ComputeClass::Trivial,
        }];
        let result = push_down(&features, DeviceTier::Reflex);
        assert_eq!(result[0].status, FeatureStatus::Available);
    }

    #[test]
    fn pushdown_empty_features() {
        let result: Vec<PushedFeature> = push_down(&[], DeviceTier::Cortex);
        assert!(result.is_empty());
    }

    #[test]
    fn pushdown_mixed_features() {
        let features = vec![
            FeatureSpec {
                name: "trivial".into(),
                min_tier: DeviceTier::Reflex,
                memory_bytes: 100,
                compute_estimate: ComputeClass::Trivial,
            },
            FeatureSpec {
                name: "heavy".into(),
                min_tier: DeviceTier::Cloud,
                memory_bytes: 100_000_000_000,
                compute_estimate: ComputeClass::Massive,
            },
        ];
        let result = push_down(&features, DeviceTier::Backbone);
        assert_eq!(result[0].status, FeatureStatus::Available);
        assert_eq!(result[1].status, FeatureStatus::Dropped);
    }
}

mod agent_tests {
    use super::*;

    struct DummyAgent {
        name: String,
        tier: DeviceTier,
    }

    impl DummyAgent {
        fn new(name: &str, tier: DeviceTier) -> Self {
            Self {
                name: name.to_string(),
                tier,
            }
        }
    }

    impl CoCaptain for DummyAgent {
        fn name(&self) -> &str {
            &self.name
        }
        fn tier(&self) -> DeviceTier {
            self.tier
        }
        fn sense(&mut self, _reading: SensorReading) -> Result<(), AgentError> {
            Ok(())
        }
        fn decide(&mut self) -> Decision {
            Decision::Hold
        }
        fn act(&mut self, decision: Decision) -> Result<Action, AgentError> {
            match decision {
                Decision::Hold => Ok(Action::NoOp),
                Decision::Adjust(v) => Ok(Action::SetValue(v)),
                Decision::Emergency(e) => match e {
                    EmergencyAction::Mayday => Err(AgentError::ActuationError("mayday".into())),
                    _ => Ok(Action::NoOp),
                },
                Decision::Escalate(msg) => Ok(Action::SendMessage(msg)),
            }
        }
        fn fallback(&self) -> Option<Box<dyn CoCaptain>> {
            if self.tier > DeviceTier::Reflex {
                Some(Box::new(DummyAgent::new("fallback-reflex", DeviceTier::Reflex)))
            } else {
                None
            }
        }
    }

    #[test]
    fn agent_basic_flow() {
        let mut agent = DummyAgent::new("test", DeviceTier::Cortex);
        assert_eq!(agent.name(), "test");
        assert_eq!(agent.tier(), DeviceTier::Cortex);

        let reading = SensorReading::new("temp-1", 23.5, DeviceTier::Reflex);
        assert!(agent.sense(reading).is_ok());

        let decision = agent.decide();
        let action = agent.act(decision).unwrap();
        assert_eq!(action, Action::NoOp);
    }

    #[test]
    fn agent_fallback_chain() {
        let agent = DummyAgent::new("cortex", DeviceTier::Cortex);
        let fb = agent.fallback().unwrap();
        assert_eq!(fb.tier(), DeviceTier::Reflex);
        assert!(fb.fallback().is_none());
    }

    #[test]
    fn agent_emergency_action() {
        let mut agent = DummyAgent::new("test", DeviceTier::Cortex);
        let action = agent.act(Decision::Emergency(EmergencyAction::Shutdown)).unwrap();
        assert_eq!(action, Action::NoOp);
    }

    #[test]
    fn agent_escalate() {
        let mut agent = DummyAgent::new("test", DeviceTier::Cortex);
        let action = agent.act(Decision::Escalate("help".into())).unwrap();
        assert_eq!(action, Action::SendMessage("help".into()));
    }
}
