//! Colony-level food economy: delivery-rate statistics and the nest store.

use bevy::prelude::*;

use crate::constants::ant::{
    ANT_DELIVERY_BOOST, ANT_DELIVERY_EMA_TAU, ANT_DELIVERY_REFERENCE_RATE,
};
use crate::constants::colony::{NEST_STORE_CAP, NEST_STORE_INITIAL};

/// Rolling counters for colony activity.
#[derive(Resource, Default)]
pub struct ColonyStats {
    /// Total food carried into the nest since startup.
    pub total_food_delivered: f32,
    /// Exponential moving average of food delivered per second. Advanced by
    /// [`tick_delivery_ema`] once per fixed step, so its equilibrium is the
    /// current delivery *rate* and it decays when foraging stops.
    pub delivery_ema: f32,
    /// Food delivered since the last [`tick_delivery_ema`] step.
    pub delivered_this_tick: f32,
}

impl ColonyStats {
    /// Record `amount` of food arriving at the nest. The amount only feeds the
    /// counters; the rate estimate is advanced once per tick by
    /// [`ColonyStats::tick_delivery_rate`].
    pub fn record_delivery(&mut self, amount: f32) {
        self.total_food_delivered += amount;
        self.delivered_this_tick += amount;
    }

    /// Advance the delivery-rate EMA by `dt` seconds.
    ///
    /// Leaky integrator: `ema = ema * exp(-dt / tau) + delivered / tau`. For a
    /// sustained rate `r` the equilibrium is exactly `r`; with no deliveries it
    /// decays with time constant [`ANT_DELIVERY_EMA_TAU`]. There is no division
    /// by `dt`, so a single delivery does not masquerade as a 64 Hz burst.
    pub fn tick_delivery_rate(&mut self, dt: f32) {
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }

        let decay = (-dt / ANT_DELIVERY_EMA_TAU).exp();
        self.delivery_ema =
            self.delivery_ema * decay + self.delivered_this_tick / ANT_DELIVERY_EMA_TAU;
        self.delivered_this_tick = 0.0;
    }

    /// Spawn-batch multiplier in `[1, 1 + ANT_DELIVERY_BOOST]`.
    pub fn delivery_boost(&self) -> f32 {
        let normalized = (self.delivery_ema / ANT_DELIVERY_REFERENCE_RATE).clamp(0.0, 1.0);
        1.0 + ANT_DELIVERY_BOOST * normalized
    }
}

/// Advance the delivery-rate estimate once per fixed step, after collisions
/// have recorded this tick's arrivals and before the next spawn batch reads
/// [`ColonyStats::delivery_boost`].
pub fn tick_delivery_ema(mut colony: ResMut<ColonyStats>, time: Res<Time<Fixed>>) {
    colony.tick_delivery_rate(time.delta_secs());
}

/// Food stored in the nest. Dropoffs are the only income; refills and nursing
/// are the expenses. The store cannot go negative or above
/// [`NEST_STORE_CAP`].
#[derive(Resource)]
pub struct NestStore {
    /// Food currently held by the nest.
    pub food: f32,
}

impl Default for NestStore {
    fn default() -> Self {
        Self {
            food: NEST_STORE_INITIAL,
        }
    }
}

impl NestStore {
    /// Add a delivery, clamped at [`NEST_STORE_CAP`].
    pub fn add(&mut self, amount: f32) {
        self.food = (self.food + amount.max(0.0)).clamp(0.0, NEST_STORE_CAP);
    }

    /// Spend `amount` food, clamped at zero; returns what was actually spent.
    pub fn spend(&mut self, amount: f32) -> f32 {
        let spent = amount.max(0.0).min(self.food);
        self.food -= spent;
        spent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-6;

    #[test]
    fn delivery_boost_rises_with_recent_deliveries() {
        let mut stats = ColonyStats::default();
        assert_eq!(stats.delivery_boost(), 1.0);

        for _ in 0..64 {
            stats.record_delivery(1.0);
        }
        stats.tick_delivery_rate(1.0 / 64.0);

        assert!((stats.total_food_delivered - 64.0).abs() < EPS);
        assert!(stats.delivery_boost() > 1.0);
        assert!(stats.delivery_boost() <= 1.0 + ANT_DELIVERY_BOOST);
    }

    #[test]
    fn single_delivery_is_not_reported_as_a_64hz_rate() {
        let mut stats = ColonyStats::default();
        stats.record_delivery(1.0);
        stats.tick_delivery_rate(1.0 / 64.0);

        // A burst of one contributes 1 / tau, not 1 / dt = 64.
        assert!((stats.delivery_ema - 1.0 / ANT_DELIVERY_EMA_TAU).abs() < EPS);
        assert!(stats.delivery_ema < 1.0);
    }

    #[test]
    fn sustained_rate_reaches_its_equilibrium() {
        let mut stats = ColonyStats::default();
        let dt = 1.0 / 64.0;
        let rate = 10.0;

        for _ in 0..(64 * 30) {
            stats.record_delivery(rate * dt);
            stats.tick_delivery_rate(dt);
        }

        assert!(
            (stats.delivery_ema - rate).abs() < 0.1,
            "EMA should track the sustained rate {rate}, got {}",
            stats.delivery_ema
        );
    }

    #[test]
    fn ema_decays_to_baseline_after_five_taus() {
        let mut stats = ColonyStats::default();
        stats.record_delivery(5.0);
        stats.tick_delivery_rate(1.0 / 64.0);

        let peak = stats.delivery_ema;
        assert!(peak > 0.5);

        let dt = 1.0 / 64.0;
        for _ in 0..((64.0 * 5.0 * ANT_DELIVERY_EMA_TAU) as u32) {
            stats.tick_delivery_rate(dt);
        }

        assert!(
            stats.delivery_ema < peak * (-5.0f32).exp() * 1.01 + EPS,
            "EMA must decay by exp(-5) after five taus, got {}",
            stats.delivery_ema
        );
        assert!((stats.delivery_boost() - 1.0).abs() < 0.01);
    }

    #[test]
    fn zero_dt_is_safe_and_keeps_pending_deliveries() {
        let mut stats = ColonyStats::default();
        stats.record_delivery(1.0);
        stats.tick_delivery_rate(0.0);

        assert_eq!(stats.delivery_ema, 0.0);
        assert_eq!(stats.delivered_this_tick, 1.0);

        stats.tick_delivery_rate(1.0 / 64.0);
        assert!(stats.delivery_ema > 0.0);
        assert_eq!(stats.delivered_this_tick, 0.0);
    }

    #[test]
    fn nest_store_defaults_to_the_bootstrap_amount() {
        assert!((NestStore::default().food - NEST_STORE_INITIAL).abs() < EPS);
    }

    #[test]
    fn nest_store_add_clamps_at_cap_and_spend_at_zero() {
        let mut store = NestStore {
            food: NEST_STORE_CAP - 1.0,
        };
        store.add(50.0);
        assert!((store.food - NEST_STORE_CAP).abs() < EPS);

        store.food = 0.5;
        assert!((store.spend(2.0) - 0.5).abs() < EPS);
        assert_eq!(store.food, 0.0);
        assert_eq!(store.spend(1.0), 0.0);
        assert_eq!(store.food, 0.0);

        // Negative or non-finite additions must not corrupt the store.
        store.food = 1.0;
        store.add(-5.0);
        assert!((store.food - 1.0).abs() < EPS);
    }
}
