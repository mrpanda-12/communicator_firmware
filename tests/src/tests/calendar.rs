use super::*;

#[test]
fn civil_epoch_is_day_zero() {
    assert_eq!(days_from_civil(1970, 1, 1), 0);
    assert_eq!(civil_from_days(0), (1970, 1, 1));
}

#[test]
fn civil_round_trips_known_dates() {
    for (year, month, day) in [
        (2000i32, 1u32, 1u32),
        (2000, 12, 31),
        (2026, 10, 28),
        (2100, 3, 1),
        (1999, 12, 31),
    ] {
        let days = days_from_civil(year, month, day);
        assert_eq!(
            civil_from_days(days),
            (year, month as i32, day as i32),
            "round trip failed for {year}-{month}-{day}"
        );
    }
}

#[test]
fn civil_handles_leap_day() {
    let leap = days_from_civil(2000, 2, 29);
    assert_eq!(civil_from_days(leap), (2000, 2, 29));
    assert_eq!(days_from_civil(2000, 3, 1), leap + 1);
    // 1900 was not a leap year: the algorithm must skip Feb 29.
    assert_eq!(days_from_civil(1900, 3, 1), days_from_civil(1900, 2, 28) + 1);
}

#[test]
fn civil_negative_days_go_before_epoch() {
    assert_eq!(civil_from_days(-1), (1969, 12, 31));
    assert_eq!(days_from_civil(1969, 12, 31), -1);
}

#[test]
fn civil_clamps_month_overflow() {
    // The clock only ever passes 1..=12; the clamp is documented behaviour.
    assert_eq!(days_from_civil(2026, 13, 1), days_from_civil(2026, 12, 1));
}

#[test]
fn epoch_2000_offset_matches_storage_format() {
    // Stored timestamps count seconds since 2000-01-01.
    assert_eq!(days_from_civil(2000, 1, 1), 10_957);
}
