//! The persistent top bar: GPS fix state, coordinates, and the clock.
//!
//! The clock always shows UTC time and date from the receiver. When the user
//! sets a UTC offset in the settings, the local time and date appear next to
//! it. Both share one clock reading, so they can never drift apart.

use core::fmt::Write;

use embedded_graphics::{
    Drawable,
    pixelcolor::Rgb565,
    prelude::{DrawTarget, Point, Primitive, Size},
    primitives::{PrimitiveStyle, Rectangle},
};
use heapless::String;

use crate::ui::GpsSnapshot;

use super::style::{BLUE, CYAN, WHITE, text};

/// 1/ Draw GPS fix, coordinates, and the clock in the persistent top bar.
pub fn draw<D>(
    display: &mut D,
    gps: GpsSnapshot,
    timezone_offset_minutes: i32,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    Rectangle::new(Point::zero(), Size::new(320, 24))
        .into_styled(PrimitiveStyle::with_fill(BLUE))
        .draw(display)?;

    let mut status: String<48> = String::new();
    if !gps.fix {
        let _ = status.push_str("NO GPS FIX");
    } else {
        let lat = gps.latitude_microdegrees;
        let lon = gps.longitude_microdegrees;
        let lat_abs = lat.unsigned_abs();
        let lon_abs = lon.unsigned_abs();
        let _ = write!(
            status,
            "GPS {}{}.{:04},{}{}.{:04}",
            if lat < 0 { "-" } else { "" },
            lat_abs / 1_000_000,
            lat_abs % 1_000_000 / 100,
            if lon < 0 { "-" } else { "" },
            lon_abs / 1_000_000,
            lon_abs % 1_000_000 / 100,
        );
    }

    // UTC clock, plus the shifted clock when the offset leaves UTC.
    if gps.has_time {
        let minutes_of_day = (gps.hour as i32) * 60 + (gps.minute as i32);
        let mut clock = String::<24>::new();
        let _ = write!(clock, " {:02}:{:02}", gps.hour, gps.minute);
        if let Some(date) = gps.date {
            let _ = write!(clock, " {:02}.{:02}", date.day, date.month);
            // The shifted clock needs a known UTC date to show a shifted date.
            if timezone_offset_minutes != 0 {
                let local_minutes = minutes_of_day + timezone_offset_minutes;
                let hour = local_minutes.rem_euclid(24 * 60) / 60;
                let minute = local_minutes.rem_euclid(24 * 60) % 60;
                let day_offset = local_minutes.div_euclid(24 * 60);
                let (_year, month, day) =
                    civil_from_days(days_from_civil(date.year, date.month, date.day) + day_offset);
                let _ = write!(clock, " {:02}:{:02} {:02}.{:02}", hour, minute, day, month);
            }
        }
        let _ = status.push_str(clock.as_str());
    }

    text(display, status.as_str(), 5, 7, CYAN, false)?;
    text(
        display,
        if gps.fix { "FIX" } else { "--" },
        294,
        7,
        WHITE,
        false,
    )
}

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
fn days_from_civil(year: i32, month: u32, day: u32) -> i32 {
    let month = month.min(12) as i32;
    let y = year - if month <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = month - 3;
    let doy = (153 * m + 2) / 5 + day as i32 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Inverse of `days_from_civil`: the civil date for days since the epoch.
fn civil_from_days(days: i32) -> (i32, i32, i32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_date_round_trips() {
        let days = days_from_civil(2026, 10, 28);
        assert_eq!(civil_from_days(days), (2026, 10, 28));
        let days = days_from_civil(2000, 2, 29);
        assert_eq!(civil_from_days(days), (2000, 2, 29));
    }

    #[test]
    fn offset_crosses_midnight() {
        // 23:30 UTC minus 60 minutes lands on the previous day.
        let base = days_from_civil(2026, 1, 1) * 1440 + 23 * 60 + 30;
        assert_eq!((base + -60).div_euclid(1440), days_from_civil(2026, 1, 1) - 1);
        // 00:30 UTC plus 60 minutes stays on the same day.
        let base = days_from_civil(2026, 1, 1) * 1440 + 30;
        assert_eq!((base + 60).div_euclid(1440), days_from_civil(2026, 1, 1));
        // 23:30 UTC plus 90 minutes lands on the next day.
        assert_eq!(
            (days_from_civil(2026, 1, 1) * 1440 + 23 * 60 + 30 + 90).div_euclid(1440),
            days_from_civil(2026, 1, 2)
        );
    }
}
