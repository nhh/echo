//! Zeitpunkte in den Server-Tabellen (`members`, `bans`, `approvals`,
//! `events`) stehen wie beim C#-Vorgaenger als .NET-Ticks UTC: 100 ns seit
//! dem 1. Januar 0001. Genau, sortierbar, und `DateTime.MaxValue` steht fuer
//! einen Bann ohne Ende.

use std::time::{SystemTime, UNIX_EPOCH};

/// Ticks am 1. Januar 1970.
const UNIX_EPOCH_TICKS: i64 = 621_355_968_000_000_000;
const TICKS_PER_SECOND: i64 = 10_000_000;
const NANOS_PER_TICK: u128 = 100;

/// `DateTime.MaxValue`: ein Bann ohne Ende.
pub(crate) const FOREVER: i64 = 3_155_378_975_999_999_999;

#[must_use]
pub(crate) fn now() -> i64 {
    from_system_time(SystemTime::now())
}

#[must_use]
pub(crate) fn from_system_time(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(after) => UNIX_EPOCH_TICKS
            .saturating_add(i64::try_from(after.as_nanos() / NANOS_PER_TICK).unwrap_or(i64::MAX)),
        Err(before) => UNIX_EPOCH_TICKS.saturating_sub(
            i64::try_from(before.duration().as_nanos() / NANOS_PER_TICK).unwrap_or(i64::MAX),
        ),
    }
}

/// Ticks in einer Anzahl Sekunden ab jetzt.
#[must_use]
pub(crate) fn after_seconds(seconds: u32) -> i64 {
    now().saturating_add(i64::from(seconds) * TICKS_PER_SECOND)
}

/// Fuer Meldungen an Nutzer: "2026-10-18 12:00:00Z", wie `{0:u}` in .NET.
#[must_use]
pub(crate) fn display(ticks: i64) -> String {
    let unix_seconds = (ticks - UNIX_EPOCH_TICKS) / TICKS_PER_SECOND;
    let format =
        time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]:[second]Z");
    time::OffsetDateTime::from_unix_timestamp(unix_seconds)
        .ok()
        .and_then(|t| t.format(&format).ok())
        .unwrap_or_else(|| "9999-12-31 23:59:59Z".to_owned())
}

/// Unix-Millisekunden, wie sie im Protokoll stehen.
#[must_use]
pub(crate) fn to_unix_millis(ticks: i64) -> i64 {
    (ticks - UNIX_EPOCH_TICKS) / (TICKS_PER_SECOND / 1000)
}

#[must_use]
pub(crate) fn unix_millis_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn matches_the_ticks_of_the_csharp_fixtures() {
        // 2026-10-01 12:00 UTC, so in den Testdaten des C#-Servers.
        let time = UNIX_EPOCH + Duration::from_hours(497_460);
        assert_eq!(from_system_time(time), 639_264_528_000_000_000);
        assert_eq!(display(639_264_528_000_000_000), "2026-10-01 12:00:00Z");
        assert_eq!(to_unix_millis(639_264_528_000_000_000), 1_790_856_000_000);
    }

    #[test]
    fn forever_lies_far_ahead_and_still_displays() {
        assert!(FOREVER > after_seconds(u32::MAX));
        assert_eq!(display(FOREVER), "9999-12-31 23:59:59Z");
        assert!(from_system_time(UNIX_EPOCH - Duration::from_secs(1)) < UNIX_EPOCH_TICKS);
        assert!(unix_millis_now() > 1_700_000_000_000);
    }
}
