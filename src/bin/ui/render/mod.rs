mod chat;
mod menu;
mod style;
mod top_bar;

use embedded_graphics::{pixelcolor::Rgb565, prelude::DrawTarget};

use super::{
    GpsSnapshot,
    state::{Screen, UiState},
};

/// 1/ Draw the fixed status bar, then dispatch to the active page renderer.
pub fn draw<D>(display: &mut D, state: &UiState, gps: GpsSnapshot) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    display.clear(style::BLACK)?;
    top_bar::draw(display, gps)?;

    match &state.screen {
        Screen::MainMenu { selected } => menu::draw_main(display, *selected),
        Screen::ChatsList { selected } => menu::draw_contacts(display, &state.contacts, *selected),
        Screen::Settings { selected } => menu::draw_settings(display, *selected),
        Screen::ChatView {
            conversation,
            input,
            menu_open,
            ..
        } => chat::draw(display, *conversation, input.as_str(), *menu_open),
    }
}
