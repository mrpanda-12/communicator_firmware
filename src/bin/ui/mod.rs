//! Application UI state and display rendering.

pub mod contact;
pub mod render;
pub mod state;

use state::UiState;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpsSnapshot {
    pub fix: bool,
    pub latitude_microdegrees: i32,
    pub longitude_microdegrees: i32,
    pub hour: u8,
    pub minute: u8,
    pub has_time: bool,
}

/// 1/ Convert one GPS coordinate to signed microdegrees for compact display text.
pub fn gps_snapshot(
    coordinate: Option<crate::hardware::gps::Coordinate>,
    time: Option<crate::hardware::gps::GpsTime>,
) -> GpsSnapshot {
    let (latitude_microdegrees, longitude_microdegrees, fix) = match coordinate {
        Some(coordinate) => (
            (coordinate.latitude_degrees * 1_000_000.0) as i32,
            (coordinate.longitude_degrees * 1_000_000.0) as i32,
            true,
        ),
        None => (0, 0, false),
    };

    let (hour, minute, has_time) = match time {
        Some(time) => (time.hour as u8, time.minute as u8, true),
        None => (0, 0, false),
    };

    GpsSnapshot {
        fix,
        latitude_microdegrees,
        longitude_microdegrees,
        hour,
        minute,
        has_time,
    }
}

/// 2/ Render the current UI model with a consistent, always-visible GPS bar.
pub fn draw<D>(display: &mut D, state: &UiState, gps: GpsSnapshot) -> Result<(), D::Error>
where
    D: embedded_graphics::prelude::DrawTarget<Color = embedded_graphics::pixelcolor::Rgb565>,
{
    render::draw(display, state, gps)
}

/// 3/ Render the current page whenever the shared GPS status changes.
pub async fn run<D>(
    mut display: D,
    mut gps: embassy_sync::watch::Receiver<
        'static,
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        GpsSnapshot,
        2,
    >,
) -> !
where
    D: embedded_graphics::prelude::DrawTarget<Color = embedded_graphics::pixelcolor::Rgb565>,
{
    let state = UiState::new();
    loop {
        let snapshot = gps.changed().await;
        if draw(&mut display, &state, snapshot).is_err() {
            defmt::error!("Display draw failed");
        }
    }
}
