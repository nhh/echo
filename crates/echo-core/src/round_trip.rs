//! Die Antwortzeit zum Server, geglaettet ueber die letzten Pings (echo-92).
//! Ein einzelner langsamer Ping soll die Anzeige nicht springen lassen; die
//! erste Messung gilt sofort, jede weitere zieht den Wert ein Stueck mit.
//! Geschrieben vom Ping-Takt der Session, gelesen von der Oberflaeche.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Gewicht der neuen Messung; 0,3 folgt einer echten Aenderung nach wenigen
/// Pings, ohne jedem Ausreisser nachzulaufen.
pub const SMOOTHING: f64 = 0.3;

const NONE: u64 = u64::MAX;

#[derive(Debug)]
pub struct RoundTripMeter {
    micros: AtomicU64,
}

impl Default for RoundTripMeter {
    fn default() -> Self {
        Self {
            micros: AtomicU64::new(NONE),
        }
    }
}

impl RoundTripMeter {
    /// Die geglaettete Antwortzeit; `None`, solange nichts gemessen ist.
    #[must_use]
    pub fn current(&self) -> Option<Duration> {
        match self.micros.load(Ordering::Relaxed) {
            NONE => None,
            micros => Some(Duration::from_micros(micros)),
        }
    }

    pub fn add(&self, sample: Duration) {
        let sample = u64::try_from(sample.as_micros()).unwrap_or(NONE - 1);
        let next = match self.micros.load(Ordering::Relaxed) {
            NONE => sample,
            before => blend(before, sample),
        };
        self.micros.store(next, Ordering::Relaxed);
    }

    pub fn reset(&self) {
        self.micros.store(NONE, Ordering::Relaxed);
    }
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn blend(before: u64, sample: u64) -> u64 {
    // Mikrosekunden einer Antwortzeit liegen weit im genauen Bereich von f64.
    let next = before as f64 + SMOOTHING * (sample as f64 - before as f64);
    next.round().max(0.0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_sample_counts_then_values_follow_slowly() {
        let meter = RoundTripMeter::default();
        assert_eq!(meter.current(), None);
        meter.add(Duration::from_millis(100));
        assert_eq!(meter.current(), Some(Duration::from_millis(100)));
        meter.add(Duration::from_millis(200));
        assert_eq!(meter.current(), Some(Duration::from_millis(130)));
        meter.reset();
        assert_eq!(meter.current(), None);
    }
}
