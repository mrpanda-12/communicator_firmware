const EARTH_RADIUS_METERS: f64 = 6_371_000.0;
const DEGREES_TO_RADIANS: f64 = core::f64::consts::PI / 180.0;

/// Calculate approximate great-circle distance and compass bearing between microdegree coordinates.
pub fn distance_and_bearing(from: (i32, i32), to: (i32, i32)) -> Option<(f64, u16)> {
    let (from_latitude, from_longitude) = coordinates_in_radians(from);
    let (to_latitude, to_longitude) = coordinates_in_radians(to);
    let latitude_delta = to_latitude - from_latitude;
    let longitude_delta = to_longitude - from_longitude;

    // Use the haversine formula so distances remain useful over short and long routes.
    let latitude_haversine = libm::sin(latitude_delta / 2.0);
    let longitude_haversine = libm::sin(longitude_delta / 2.0);
    let haversine = (latitude_haversine * latitude_haversine
        + libm::cos(from_latitude)
            * libm::cos(to_latitude)
            * longitude_haversine
            * longitude_haversine)
        .clamp(0.0, 1.0);
    let central_angle = 2.0 * libm::atan2(libm::sqrt(haversine), libm::sqrt(1.0 - haversine));
    let distance = EARTH_RADIUS_METERS * central_angle;

    // Measure bearings clockwise from north and normalize to one compass revolution.
    let east = libm::sin(longitude_delta) * libm::cos(to_latitude);
    let north = libm::cos(from_latitude) * libm::sin(to_latitude)
        - libm::sin(from_latitude) * libm::cos(to_latitude) * libm::cos(longitude_delta);
    let mut bearing = libm::atan2(east, north) / DEGREES_TO_RADIANS;
    if bearing < 0.0 {
        bearing += 360.0;
    }

    if distance.is_finite() && bearing.is_finite() {
        Some((distance, libm::round(bearing) as u16 % 360))
    } else {
        None
    }
}

/// Return a compass abbreviation for an absolute bearing.
pub fn compass_direction(degrees: u16) -> &'static str {
    const DIRECTIONS: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    DIRECTIONS[((degrees as usize + 22) / 45) % DIRECTIONS.len()]
}

/// Return the signed shortest turn from the current course to a target bearing.
pub fn relative_turn_degrees(target: u16, course: u16) -> i16 {
    let difference = (target as i32 - course as i32 + 540) % 360 - 180;
    difference as i16
}

fn coordinates_in_radians(coordinates: (i32, i32)) -> (f64, f64) {
    (
        f64::from(coordinates.0) / 1_000_000.0 * DEGREES_TO_RADIANS,
        f64::from(coordinates.1) / 1_000_000.0 * DEGREES_TO_RADIANS,
    )
}
