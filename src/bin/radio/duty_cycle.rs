use embassy_time::{Duration, Instant, Timer};

use crate::radio::types::FrequencyBand;

/// Transmit budget as a percentage of wall-clock time.
const DUTY_PERCENT: i64 = 10;
/// Length of the accounting window.
const WINDOW_US: i64 = 3_600_000_000;
/// Airtime one window allows at the configured duty cycle.
const BURST_US: i64 = WINDOW_US * DUTY_PERCENT / 100;
/// Airtime a fresh boot may spend before it has accrued any budget.
const INITIAL_US: i64 = BURST_US / 60;

/// Enforces a 10 % transmit duty cycle for each RF band.
///
/// A token bucket accrues airtime at 10 % of real time and starts full, so a
/// fresh boot can transmit one window's worth of traffic before it is paced.
pub struct DutyCycle {
    budgets: [Budget; 3],
}

#[derive(Clone, Copy)]
struct Budget {
    tokens_us: i64,
    updated_us: u64,
}

impl DutyCycle {
    /// Create a limiter with a full budget for every band.
    pub const fn new() -> Self {
        Self {
            budgets: [Budget {
                tokens_us: INITIAL_US,
                updated_us: 0,
            }; 3],
        }
    }

    /// Wait until the band has airtime available, then charge it.
    pub async fn reserve(&mut self, band: FrequencyBand, airtime_us: u64) {
        let airtime = airtime_us.max(1) as i64;

        loop {
            let now = Instant::now().as_micros();
            let budget = &mut self.budgets[band as usize];
            let elapsed = now.saturating_sub(budget.updated_us) as i64;
            let earned = elapsed * DUTY_PERCENT / 100;
            budget.tokens_us = (budget.tokens_us + earned).min(BURST_US);
            budget.updated_us = now;

            if budget.tokens_us >= airtime {
                budget.tokens_us -= airtime;
                return;
            }

            // Wait long enough for the missing airtime to accrue at 10 %.
            let deficit = airtime - budget.tokens_us;
            let wait_us = (deficit as u64) * 100 / DUTY_PERCENT as u64;
            Timer::after(Duration::from_micros(wait_us)).await;
        }
    }
}

impl Default for DutyCycle {
    fn default() -> Self {
        Self::new()
    }
}
