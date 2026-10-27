use core::fmt::Write as _;

use embedded_graphics::{
    Drawable,
    pixelcolor::Rgb565,
    prelude::{DrawTarget, Point, Primitive, Size},
    primitives::{PrimitiveStyle, Rectangle},
};
use heapless::String;

use crate::ui::{
    GpsSnapshot,
    navigation::{compass_direction, distance_and_bearing, relative_turn_degrees},
};

use super::style::{ACK_GREEN, MUTED, WHITE, text};

/// Draw distance and movement-relative guidance to the selected remote coordinate.
pub fn draw<D>(
    display: &mut D,
    current: GpsSnapshot,
    movement_course_degrees: Option<u16>,
    target_latitude_microdegrees: i32,
    target_longitude_microdegrees: i32,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    Rectangle::new(Point::new(5, 28), Size::new(310, 188))
        .into_styled(PrimitiveStyle::with_stroke(MUTED, 1))
        .draw(display)?;
    text(display, "NAVIGATE TO GPS LOCATION", 12, 36, WHITE, false)?;
    let mut coordinate = String::<48>::new();
    let _ = write!(coordinate, "TARGET ");
    write_coordinate(&mut coordinate, target_latitude_microdegrees);
    let _ = coordinate.push(',');
    write_coordinate(&mut coordinate, target_longitude_microdegrees);
    text(display, coordinate.as_str(), 12, 62, WHITE, false)?;

    if !current.fix {
        text(display, "WAITING FOR LOCAL GPS FIX", 12, 94, MUTED, false)?;
        return text(display, "BACK: RETURN TO CHAT", 12, 188, WHITE, false);
    }

    let current_coordinates = (
        current.latitude_microdegrees,
        current.longitude_microdegrees,
    );
    let target_coordinates = (target_latitude_microdegrees, target_longitude_microdegrees);
    if let Some((distance_meters, target_bearing)) =
        distance_and_bearing(current_coordinates, target_coordinates)
    {
        let mut distance = String::<32>::new();
        let _ = write!(
            distance,
            "DISTANCE: {} M",
            libm::round(distance_meters) as u32
        );
        text(display, distance.as_str(), 12, 90, WHITE, false)?;
        let mut bearing = String::<32>::new();
        let _ = write!(
            bearing,
            "GO {} / {} DEG",
            compass_direction(target_bearing),
            target_bearing
        );
        text(display, bearing.as_str(), 12, 110, ACK_GREEN, false)?;

        if distance_meters <= 5.0 {
            text(
                display,
                "YOU ARE AT THE LOCATION",
                12,
                140,
                ACK_GREEN,
                false,
            )?;
        } else if let Some(course) = movement_course_degrees {
            let turn = relative_turn_degrees(target_bearing, course);
            let mut guidance = String::<40>::new();
            let direction = if turn.abs() < 15 {
                "STRAIGHT"
            } else if turn > 0 {
                "RIGHT"
            } else {
                "LEFT"
            };
            let _ = write!(guidance, "TURN {direction} {} DEG", turn.unsigned_abs());
            text(display, guidance.as_str(), 12, 140, WHITE, false)?;
            let mut movement = String::<32>::new();
            let _ = write!(movement, "MOVING {}", compass_direction(course));
            text(display, movement.as_str(), 12, 160, MUTED, false)?;
        } else {
            text(display, "WALK 5M TO SET DIRECTION", 12, 140, MUTED, false)?;
        }
    } else {
        text(display, "COULD NOT CALCULATE ROUTE", 12, 110, MUTED, false)?;
    }

    text(display, "BACK: RETURN TO CHAT", 12, 188, WHITE, false)
}

/// Format signed microdegrees without allocating or depending on float formatting.
fn write_coordinate(output: &mut String<48>, coordinate: i32) {
    let sign = if coordinate < 0 { "-" } else { "" };
    let absolute = coordinate.unsigned_abs();
    let _ = write!(
        output,
        "{sign}{}.{:06}",
        absolute / 1_000_000,
        absolute % 1_000_000
    );
}
