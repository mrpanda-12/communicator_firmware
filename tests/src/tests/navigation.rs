use super::*;

#[test]
fn navigation_same_point() {
    let (distance, bearing) = distance_and_bearing((0, 0), (0, 0)).unwrap();
    assert!(distance.abs() < 1e-6);
    assert_eq!(bearing, 0);
}

#[test]
fn navigation_cardinal_bearings() {
    let (north_distance, north) = distance_and_bearing((0, 0), (1_000_000, 0)).unwrap();
    assert_eq!(north, 0);
    assert!((north_distance - 111_195.0).abs() < 50.0);

    let (east_distance, east) = distance_and_bearing((0, 0), (0, 1_000_000)).unwrap();
    assert_eq!(east, 90);
    assert!((east_distance - 111_195.0).abs() < 50.0);

    let (_, south) = distance_and_bearing((0, 0), (-1_000_000, 0)).unwrap();
    assert_eq!(south, 180);

    let (_, west) = distance_and_bearing((0, 0), (0, -1_000_000)).unwrap();
    assert_eq!(west, 270);
}

#[test]
fn compass_direction_boundaries() {
    assert_eq!(compass_direction(0), "N");
    assert_eq!(compass_direction(22), "N");
    assert_eq!(compass_direction(23), "NE");
    assert_eq!(compass_direction(90), "E");
    assert_eq!(compass_direction(180), "S");
    assert_eq!(compass_direction(270), "W");
    assert_eq!(compass_direction(337), "NW");
    assert_eq!(compass_direction(359), "N");
}

#[test]
fn relative_turn_is_shortest() {
    assert_eq!(relative_turn_degrees(90, 0), 90);
    assert_eq!(relative_turn_degrees(0, 90), -90);
    assert_eq!(relative_turn_degrees(350, 10), -20);
    assert_eq!(relative_turn_degrees(10, 350), 20);
    assert_eq!(relative_turn_degrees(0, 180).abs(), 180);
}
