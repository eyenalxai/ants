//! Colony-level food-delivery statistics used to pace recruitment.

use bevy::prelude::*;

use crate::constants::ant::{
    ANT_DELIVERY_BOOST, ANT_DELIVERY_EMA_TAU, ANT_DELIVERY_REFERENCE_RATE,
};

/// Rolling counters for colony activity.
#[derive(Resource, Default)]
pub struct ColonyStats {
    /// Total food carried into the nest since startup.
    pub total_food_delivered: f32,
    /// Exponential moving average of food delivered per second.
    pub delivery_ema: f32,
}

impl ColonyStats {
    /// Record `amount` of food arriving at the nest over `dt` seconds.
    pub fn record_delivery(&mut self, amount: f32, dt: f32) {
        self.total_food_delivered += amount;

        if dt <= 0.0 {
            return;
        }

        let instant_rate = amount / dt;
        let alpha = (dt / (dt + ANT_DELIVERY_EMA_TAU)).clamp(0.0, 1.0);
        self.delivery_ema += (instant_rate - self.delivery_ema) * alpha;
    }

    /// Spawn-batch multiplier in `[1, 1 + ANT_DELIVERY_BOOST]`.
    pub fn delivery_boost(&self) -> f32 {
        let normalized = (self.delivery_ema / ANT_DELIVERY_REFERENCE_RATE).clamp(0.0, 1.0);
        1.0 + ANT_DELIVERY_BOOST * normalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivery_boost_rises_with_recent_deliveries() {
        let mut stats = ColonyStats::default();
        assert_eq!(stats.delivery_boost(), 1.0);

        let dt = 1.0 / 64.0;
        for _ in 0..64 {
            stats.record_delivery(1.0, dt);
        }

        assert!(stats.total_food_delivered > 63.0);
        assert!(stats.delivery_boost() > 1.0);
        assert!(stats.delivery_boost() <= 1.0 + ANT_DELIVERY_BOOST);
    }

    #[test]
    fn zero_dt_is_safe() {
        let mut stats = ColonyStats::default();
        stats.record_delivery(1.0, 0.0);
        assert_eq!(stats.delivery_ema, 0.0);
        assert_eq!(stats.total_food_delivered, 1.0);
    }
}
