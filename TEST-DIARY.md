# cocapn-core — Test Diary

**Evaluator:** Marina (embedded systems engineer, marine autopilots)  
**Date:** 2026-06-01  
**Crate:** `cocapn-core` v0.1.0  
**Test repo:** `SuperInstance/cocapn-core` (commit from `main`, depth 1)

---

## 1. First Impression — Does the README Explain What This Is?

**Short answer: Yes, but you need to parse it carefully if you don't know "CoCapn".**

The README opens with:

> "The heart of CoCapn — device tiers, deadband triggers, compute striping, crossfade handoffs, and the push-down principle."

And immediately follows with: "CoCapn is a distributed agent framework where the same system runs on a $3 ESP32, a $35 Raspberry Pi, a $500 GPU edge device, and cloud APIs — and intelligence pushes *down* to the cheapest hardware that can handle it."

This is one of the better READMEs I've read from a new crate. It doesn't assume you know the project, but it does kind of drop you into the terminology without a clear "what problem does this solve" elevator pitch at the top. The 30-second code sample is good. The tier diagram is excellent. The "AI-optional principle" gets the core philosophy across in one sentence.

**Verdict:** A new reader will get it, but the first paragraph could be punchier. Like: *"CoCapn is a control hierarchy for systems that need to keep working when expensive compute goes down. The ESP32 holds the heading even if the cloud dies. That's not a fallback — that's the design."* — that's actually already in the README, but buried mid-way.

---

## 2. Build — cargo test, cargo clippy

### Build Results

| Command | Result |
|---------|--------|
| `cargo build` | Clean, no warnings |
| `cargo test` (36 built-in tests) | ✅ All pass |
| `cargo clippy --all-targets -D warnings` | ✅ Clean |
| My scenario tests (5 additional) | ✅ All pass |

**No issues.** The crate is lean — compiled in ~2 seconds. `thiserror` is the only non-optional dependency. `serde` and `tokio` are feature-gated. That's the right call for embedded-adjacent code where you don't want bloat.

### What I'd flag

- `#![deny(unsafe_code)]` — good
- No `no_std` support. For ESP32 this is fine (ESP32-Rust uses std), but if you wanted to port to bare-metal Cortex-M or AVR, you'd need `#![no_std]` compatibility. Not a blocker for this use case, but worth noting.

---

## 3. Architecture Review

I read every source file in `/src/`.

### 3a. The Tier Model

Four tiers: **Reflex → Backbone → Cortex → Cloud**

Actually sensible for marine:

| Tier | Hardware fit | Why |
|------|-------------|-----|
| Reflex | ESP32, any microcontroller | Heading hold, NMEA parsing, basic PID |
| Backbone | Raspberry Pi | Route management, AIS processing, WiFi/cellular bridge |
| Cortex | Jetson, x86 edge | Camera perception, collision avoidance models |
| Cloud | API | Weather routing, fleet analytics |

`DeviceTier` derives `PartialOrd` and `Ord` with `Reflex < Backbone < Cortex < Cloud`. The `PartialOrd` implementation relies on declaration order, which is correct.

The `Device` struct uses `HashSet<Capability>`, which is fine. `last_seen: Instant` is a nice touch for health monitoring. `Device::can()` is clear.

**One concern:** `Device` equality is solely by `id` (the Hash impl only uses `id`). This means two devices with the same ID but different capabilities are equal. In a real system, if a device reboots and its capabilities change, the hash map won't update properly. This is a design detail, not a bug, but worth being aware of.

### 3b. Deadband — Is It Mathematically Correct?

Let me walk through the math carefully.

**Deadband::check(value):**

1. If `center == 0.0`: uses absolute difference vs `tolerance`. Simple linear band. OK.

2. If `center != 0.0`:
   - `relative_diff = (value - center) / |center|`
   - Filter by direction (Above/Below/Both)
   - `abs_rel = |relative_diff|`
   - If `abs_rel > tolerance` → Exceeded
   - If `abs_rel > tolerance * 0.8` → Approaching
   - Else → Normal

**This is correct** for a proportional deadband around a non-zero center. Example:

```
center=100, tolerance=0.05 (5%)
value=107: diff=7/100=0.07 > 0.05 → Exceeded ✓
value=104.5: diff=4.5/100=0.045 < 0.05, > 0.04 → Approaching ✓
value=97.0: diff=-3/100=-0.03, abs=0.03 < 0.04 → Normal ✓
```

**What about center=0 when you want a configurable deadband?** The current behavior when `center=0` falls back to absolute tolerance. That's reasonable for a value like heading error where 0 is the reference point. But for a shaft encoder or compass calibration offset, having a center of 0 with a *relative* expectation would be ambiguous. **This is a minor design issue** — absolute vs relative should be a separate parameter, not inferred from whether center is exactly 0.

**Approaching threshold at exactly 80% of tolerance:** This is a hardcoded magic number. Could be a configurable `warning_threshold` instead. As-is, it works, but it means the "warning zone" is very narrow for small tolerances. Example: 1% tolerance → approaches at 0.8% → only 0.2% band (0.18° on a 90° heading). That's 0.18° — fine for steering, but meaningless for something like voltage monitoring.

**One-sided deadbands (Above/Below):** Correctly implemented. `Below` mode returns `Normal` for any value above center, and vice versa. This is genuinely useful for conservation monitoring.

### 3c. Handoff State Machine

States: `Stable → FadingOut → Crossfading → FadingIn → Complete`

Transitions:
- `begin()`: Stable → FadingOut (starts timer, resets elapsed)
- `progress(delta)`: advances based on elapsed/total ratio
  - 0%–33%: FadingOut
  - 33%–66%: Crossfading
  - 66%–100%: FadingIn
  - ≥100%: Complete
- `cancel()`: ✓ resets to Stable, marks cancelled
- After cancel: `begin()` returns Err — correct

**What's missing:**
1. **Hard timeout / forced takeover.** If the from-device goes silent mid-handoff, what happens? There's no timeout mechanism that forces completion. Real autopilots need: "If no heartbeat from source for X seconds, force complete."
2. **The output blending.** `HandoffState` tracks the phase, but there's no `blend(from_value, to_value, t) -> f64` function. The crossfade concept is *named* but not *implemented*. The struct tracks `elapsed` but doesn't expose a `blend_factor()` or `weight()` method. Any consumer has to compute weights themselves.
3. **No explicit "failure" state.** If the target device goes offline during handoff, there's no way to abort mid-transition other than `cancel()` — which reverts to the original device. But what if the original device is already gone?

**Assessment:** The state machine skeleton is there. For a v0.1 crate, the design direction is right. But for real deployment, you'd need:
- Blend weight calculation
- Heartbeat/timeout on handoff partners
- A `Failed` state or equivalent

### 3d. Compute Stripe

`Stripe` is a sorted list of `StripeLayer`. Sorting is done by tier descending (highest compute first). This is correct for the "highest capable device drives" model.

- `fail_layer()` mutates health flag and emits a `Rebalanced` or `Degraded` event
- `fallback_path()` returns tiers from current down to Reflex
- `rebalance()` is a stub that returns `None` (no-op)

**Issues:**
1. **`rebalance()` is a no-op.** The method exists but doesn't redistribute work. For a production system, this would need to actually reallocate features/tasks among healthy devices.
2. **Health is a single bool.** No health metrics, no last-seen timestamp, no latency history. Real systems need: "this device has been failing 30% of pings in the last 60 seconds → mark unhealthy."
3. **No device-level metadata on the stripe.** If a device fails, the event says "Cortex layer failed" but doesn't carry enough info for the consumer to know what to re-deploy.

### 3e. Push-Down Evaluator

`push_down(features, available_tier) → Vec<PushedFeature>`

Evaluates each feature against tier capacity (compute class + memory). Three outcomes: Available, Degraded, Dropped.

The `tier_capacity()` function maps:
- `Reflex → (Trivial, 520KB)`
- `Backbone → (Light, 4GB)`
- `Cortex → (Heavy, 32GB)`
- `Cloud → (Massive, MAX)`

**This has real problems for embedded:**

1. **520KB RAM for ESP32 is generous.** Real ESP32-S3 has ~512KB SRAM. Subtract WiFi stack, RTOS overhead, SDK — you're left with maybe 200-300KB for application code. The 520KB figure is misleading.
2. **"Light" compute for Backbone / Pi 4GB is reasonable** — a Pi 4 can run a lightweight ML model. But `ComputeClass::Light` as `<= Light` means Backbone considers itself capable of Medium compute. That's a bug: tier_capacity says Backbone = `Light`, but the `push_down` check `spec.compute_estimate <= max_compute` means a Medium compute task would pass for Backbone.
3. **The Degraded logic is weird.** It checks: `available_tier >= Backbone && compute fits && memory fits`. So a feature with min_tier=Cortex, compute=Heavy, memory=100MB running on Backbone → the compute check fails (Backbone max=Light < Heavy) → falls through to Degraded check → fails the second condition too → Dropped. But a feature with min_tier=Cortex, compute=Light, memory=100MB would be Degraded. This means "Dropped vs Degraded" depends on the compute/memory fit, not just the tier gap.
4. **No runtime monitoring.** Push-down evaluates statically (feature spec vs tier capacity). In a real boat, the Pi might have 30% CPU available one minute and 90% the next. The evaluator can't adapt.

### 3f. Agent / CoCaptain Trait

The `CoCaptain` trait defines the lifecycle: `sense → decide → act → fallback`. Clean, simple.

`Decision::Escalate(String)` is a nice escape hatch for asking a higher tier for help.

`EmergencyAction` only has three variants: `Shutdown, Surface, Mayday`. For marine: missing `DropAnchor`, `HeadToWind`, `EngageBackupCompass`. But this is the core crate — domain-specific emergencies belong in `cocapn-marine`.

---

## 4. Real Integration Test Results

I wrote 5 scenario tests in `tests/boat_scenario.rs`. All pass.

### Scenario: ESP32 holds heading at 90°

Deadband correctly tracks Normal → Approaching → Exceeded transitions. The proportional controller produces appropriate rudder commands. **No issues found.**

### Scenario: Crossfade handoff ESP32 to Pi

Handoff state transitions through all 4 states correctly. Timing-based progress works. **One gap:** there's no actual control signal blending — the test manually computes blended weight but the crate provides no `blend()` function for the consumer.

### Scenario: Pi fails, handoff back to ESP32

Failure detected via `failed` flag on the agent. Handoff completes in 1 second (emergency reversion). ESP32 takes over with correct rudder command. **This works but the failure detection is manual — real autopilots need watchdog timer integration.**

### Scenario: Stripe-based failover

Cortex → Backbone → Reflex cascade works correctly through `fail_layer()`. Events are emitted at each level. **Stripe never propagates failure info to the Handoff module** — these are two separate systems that a consumer would need to wire together themselves.

### Scenario: Push-down for marine features

Heading hold and NMEA parser correctly flagged as Available at Reflex. Route following correctly Dropped. Wave prediction correctly Dropped at Backbone (Backbone's compute capacity is Light, wave prediction is Heavy). **This matches expectations for real marine hardware.**

---

## 5. What's Missing for Real Boat Deployment

### Critical (must-have before I'd put this on a fishing boat):

1. **No watchdog / heartbeat integration.** Every autopilot has a watchdog timer. The ESP32 needs to assert a hardware WDT every N milliseconds or the rudder goes failsafe. The `Device` struct has `last_seen` but nothing uses it for timeout detection.

2. **No real-time blending function.** The `Handoff` tracks the crossfade phase but doesn't provide `blend(a: f64, b: f64, t: f64) -> f64`. Anyone building an autopilot has to roll their own.

3. **No NMEA layer.** This belongs in `cocapn-marine` — but that crate doesn't exist yet. Without NMEA 0183/2000 parsing, the system can't interface with real boat electronics (GPS, compass, wind sensors, AIS).

4. **No PID controller.** The crate provides deadband thresholds but no control loop. Deadband says "something is wrong" but doesn't compute the correction. You'd wire this yourself, but a reference PID or at least a `Controller` trait would save months of rework.

5. **No no_std support.** ESP32-Rust supports std, so this isn't a blocker — but many marine microcontrollers (STM32, nRF52) use `#![no_std]`.

### Important (should have before production):

6. **Stripe::rebalance() is a no-op.** It returns `None` instead of doing anything. This should at minimum trigger a push-down reevaluation across tiers.

7. **No runtime feature scaling.** Push-down is purely static. Real systems need "the Pi is at 80% CPU → degrade the route planner to 5Hz instead of 10Hz."

8. **Center=0 deadband ambiguity.** Absolute vs relative tolerance should be an explicit enum, not inferred from center value.

9. **Handoff has no "Failed" state.** If the target device dies during handoff, there's no recovery path.

### Nice to have:

10. **Configurable approaching threshold.** The hardcoded 80% of tolerance is fine but domain-specific. A marine autopilot might want 50% (warn earlier for safety) or 95% (tight margins for efficiency).

11. **Device metadata.** Things like: firmware version, boot count, uptime, sensor calibration offsets.

12. **Multiplexed deadbands.** Often you want to check heading *and* rate of turn *and* cross-track error — the "approach" state should be a composite, not a single value check.

---

## 6. Score: ★★★☆☆ (3 out of 5)

### Breakdown

| Category | Stars | Notes |
|----------|-------|-------|
| **Concept & Architecture** | ★★★★★ | The tier model, push-down principle, and AI-optional philosophy are genuinely well-designed. This is a solid foundation. |
| **Code Quality** | ★★★★☆ | Clean Rust, no unsafe, good tests (36 unit tests pass), clippy-clean. Lean dependencies. |
| **Correctness** | ★★★★☆ | Deadband math checks out. Handoff state machine is complete except for missing failure states. |
| **Real-world Readiness** | ★★☆☆☆ | Missing: control blending, watchdog, PID, NMEA, no_std. The concepts are right but the *implementation* hasn't hit seawater yet. |
| **Documentation** | ★★★☆☆ | README is good but some API docs are thin (look at `pub` fields with no doc comments). |

**Overall: 3/5**

It's a promising *specification-in-code* more than a *usable library* right now. If you told me "we're building the marine crate next and these are the core types," I'd say "great foundation, start shipping." If you told me "put this on a boat today," I'd laugh. The architecture decisions are sound, the code is clean, but it's v0.1-level — the plumbing between modules (stripe → handoff → deadband → control) isn't wired up.

For an embedded systems engineer evaluating a crate for production: **watch it, contribute if you can, but don't depend on it yet.** Write your own PID loop, your own NMEA parser, and use `Deadband`, `Handoff`, and `DeviceTier` as your vocabulary. Those are the bits worth keeping.

---

## Appendix: Test Output (36 built-in + 5 scenario)

```
cargo test --all-targets
    Finished `test` profile [unoptimized + debuginfo]
    Running tests/integration_tests.rs — 36 passed, 0 failed

cargo test --test boat_scenario
    Running tests/boat_scenario.rs — 5 passed, 0 failed

cargo clippy --all-targets -D warnings — clean
```

Test code for the boat scenario is in `tests/boat_scenario.rs`.
