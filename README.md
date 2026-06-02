# cocapn-core

The heart of CoCapn — device tiers, deadband triggers, compute striping, crossfade handoffs, and the push-down principle.

CoCapn is a distributed agent framework where the same system runs on a $3 ESP32, a $35 Raspberry Pi, a $500 GPU edge device, and cloud APIs — and intelligence pushes *down* to the cheapest hardware that can handle it.

This crate defines the core types and protocols that all other CoCapn crates and polyglot implementations share.

## The 30-Second Version

```rust
use cocapn_core::{Device, DeviceTier, Capability, Deadband, DeadbandDirection};

// A deadband: "is this value still on course?"
let db = Deadband::with_relative_tolerance(100.0, 0.05); // 5% tolerance
assert_eq!(db.check(97.0), DeadbandState::Normal);   // still good
assert_eq!(db.check(107.0), DeadbandState::Exceeded); // drifted too far

// A device at the lowest tier
let esp32 = Device::new(1, "helm-esp32", DeviceTier::Reflex)
    .with_capability(Capability::Sense)
    .with_capability(Capability::Act);
assert!(esp32.can(Capability::Sense));
assert!(!esp32.can(Capability::Predict)); // ESP32 doesn't predict
```

## Architecture

CoCapn uses a layered model where **every layer works without the layers above it**:

```
┌─────────────┐
│    Cloud     │  APIs, LLMs, expensive compute ($$$)
├─────────────┤
│   Cortex     │  Jetson, GPU edge devices ($$)
├─────────────┤
│  Backbone    │  Raspberry Pi, single-board computers ($)
├─────────────┤
│   Reflex     │  ESP32, Arduino, bare metal (¢)
└─────────────┘
```

The ESP32 steers the boat even if the cloud, the GPU, and the Pi all die. That's not a fallback — that's the design.

## Device Tiers

| Tier | Hardware | RAM | Cost | What it does |
|------|----------|-----|------|-------------|
| Reflex | ESP32, Arduino | ~520KB | $3 | Sense, act, hold heading |
| Backbone | Raspberry Pi | 4GB | $35 | Route, communicate, basic ML |
| Cortex | Jetson Orin | 8-64GB + GPU | $500 | Predict, train, vision |
| Cloud | Any API | Unlimited | $/use | Full AI, large models |

```rust
use cocapn_core::DeviceTier;

let tier = DeviceTier::Reflex;
assert!(tier < DeviceTier::Backbone);
assert!(tier < DeviceTier::Cortex);
```

## Deadband

The deadband is the universal trigger in CoCapn. It answers one question: **"has this value drifted far enough from center to warrant action?"**

Three states:
- **Normal** — within tolerance, nothing to do
- **Approaching** — nearing the edge (warn)
- **Exceeded** — outside tolerance (act now)

```rust
let db = Deadband::new(100.0, 0.05, DeadbandDirection::Both);

match db.check(104.5) {
    DeadbandState::Normal => { /* keep going */ },
    DeadbandState::Approaching => { /* warn */ },
    DeadbandState::Exceeded => { /* correct */ },
}
```

### One-Sided (Conservation) Deadbands

For conservation checking, you only care about values *decreasing* — fuel dropping, battery draining, budget depleting. Use `DeadbandDirection::Below`:

```rust
// Only trigger when fuel drops below target
let fuel_db = Deadband::new(100.0, 0.10, DeadbandDirection::Below);
assert_eq!(fuel_db.check(115.0), DeadbandState::Normal);  // more fuel is fine
assert_eq!(fuel_db.check(85.0), DeadbandState::Exceeded); // fuel dropping!
```

## Compute Stripe

A compute stripe is an ordered set of devices across tiers. When one device fails, the next takes over.

```rust
use cocapn_core::{Stripe, Device};

let stripe = Stripe::new()
    .add_device(esp32)
    .add_device(pi)
    .add_device(jetson);

stripe.rebalance(); // redistribute work to available devices
let fallback = stripe.fallback_path(); // ordered list of who takes over
```

## Crossfade Handoff

When control passes between devices, it doesn't switch instantly — it crossfades, like a DJ transitioning between tracks.

States: `Stable → FadingOut → Crossfading → FadingIn → Complete`

```rust
use cocapn_core::Handoff;

let mut handoff = Handoff::new(pi, jetson);
handoff.begin(); // → FadingOut
handoff.progress(0.5); // → Crossfading (50/50 blend)
handoff.progress(1.0); // → Complete
// Can cancel at any point: handoff.cancel() → Stable (back to original)
```

## The Push-Down Principle

Intelligence pushes requirements *down* to the cheapest hardware that can run it. If the Pi can run a model, don't send it to the cloud. If the ESP32 can hold a heading, don't ask the Pi.

```rust
use cocapn_core::pushdown::{PushdownEvaluator, FeatureLevel};

let evaluator = PushdownEvaluator::new()
    .add_feature("heading_hold", FeatureLevel::Reflex)   // ESP32 can do this
    .add_feature("route_planning", FeatureLevel::Backbone) // needs Pi
    .add_feature("weather_forecast", FeatureLevel::Cortex); // needs GPU

// What can the ESP32 do?
let available = evaluator.evaluate(DeviceTier::Reflex);
// → heading_hold: Available, route_planning: Dropped, weather_forecast: Dropped
```

## CoCaptain Trait

The agent interface — sense, decide, act, fallback:

```rust
use cocapn_core::{CoCaptain, Decision, Action, EmergencyAction};

struct HelmKeeper;

impl CoCaptain for HelmKeeper {
    fn sense(&self) -> Vec<f64> { /* read sensors */ }
    fn decide(&self, readings: &[f64]) -> Decision { /* choose action */ }
    fn act(&self, decision: &Decision) -> Action { /* execute */ }
    fn fallback(&self) -> EmergencyAction { /* when everything fails */ }
}
```

## Polyglot

The same core concepts exist across languages:

| Crate | Language | Lines (deadband) | What you gain |
|-------|----------|-------------------|--------------|
| cocapn-core | Rust | ~120 | Memory safety, zero-cost |
| cocapn-c | C99 | ~60 | Runs on literally everything |
| cocapn-zig | Zig | ~80 | Compile-time verification |
| cocapn-ada | Ada | ~100 | Provable correctness |
| cocapn-forth | Forth | ~40 | Runs on a comet |

Same deadband. Same heading. Different languages see it differently.

## Installation

```toml
[dependencies]
cocapn-core = "0.1"
```

Optional features:
```toml
cocapn-core = { version = "0.1", features = ["serde"] }  # serialization
cocapn-core = { version = "0.1", features = ["tokio"] }  # async
```

## Related Crates

- **cocapn-marine** — NMEA parsing, PID autopilot, bathymetry, marine deadbands
- **cocapn-escalation** — Tier escalation, budget tracking, device rebalancing, JEPA-lite prediction
- **cocapn-pushdown** — Feature degradation, device profiles, A/B versioned configs
- **cocapn-opencpn** — Chart contour generation, bathy overlays, Nobeltec import

## The AI-Optional Principle

Every layer in CoCapn works without the layers above it. The ESP32 holds the heading even when the cloud is unreachable. The Pi navigates even when the GPU is rebooting. The system degrades gracefully, never catastrophically.

AI enhances. It does not replace.

## License

MIT
