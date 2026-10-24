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

/// 1/ Draw GPS fix, coordinates, and UTC time in the persistent top bar.
pub fn draw<D>(display: &mut D, gps: GpsSnapshot) -> Result<(), D::Error>
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
    if gps.has_time {
        let _ = write!(status, "  {:02}:{:02}Z", gps.hour, gps.minute);
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
