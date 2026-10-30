//! Active announce screen: sends every two seconds and lists the replies.

use embedded_graphics::{pixelcolor::Rgb565, prelude::DrawTarget};

use crate::network::types::DiscoveryList;

use super::draw_device_list;

/// Draw the announce list; ENTER opens the add flow, BACK stops the scan.
pub fn draw<D>(display: &mut D, devices: &DiscoveryList, selected: usize) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    draw_device_list(
        display,
        "ADD DEVICE",
        "ANNOUNCING EVERY 2 SECONDS",
        devices,
        selected,
        "ENTER ADD   BACK STOPS SCAN",
    )
}
