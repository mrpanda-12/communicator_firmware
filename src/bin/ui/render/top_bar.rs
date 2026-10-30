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

/// Draw GPS fix, coordinates, and the clock in the persistent top bar.
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

use crate::calendar::{civil_from_days, days_from_civil};

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
        assert_eq!(
            (base + -60).div_euclid(1440),
            days_from_civil(2026, 1, 1) - 1
        );
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
