mod chat;
mod menu;
mod navigation;
mod settings;
mod style;
mod top_bar;

use embedded_graphics::{
    Drawable,
    pixelcolor::Rgb565,
    prelude::{DrawTarget, Point, Primitive, Size},
    primitives::{PrimitiveStyle, Rectangle},
};

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
    draw_page(display, state)
}

/// 2/ Redraw changed UI objects and retain pixels for every unchanged object.
pub fn draw_if_changed<D>(
    display: &mut D,
    state: &UiState,
    gps: GpsSnapshot,
    previous_state: &mut Option<UiState>,
    previous_gps: &mut Option<GpsSnapshot>,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let Some(old_state) = previous_state.as_ref() else {
        draw(display, state, gps)?;
        *previous_state = Some(state.clone());
        *previous_gps = Some(gps);
        return Ok(());
    };

    if old_state != state {
        draw_state_changes(display, old_state, state)?;
        *previous_state = Some(state.clone());
    }
    if *previous_gps != Some(gps) {
        top_bar::draw(display, gps)?;
        *previous_gps = Some(gps);
    }

    Ok(())
}

fn draw_state_changes<D>(
    display: &mut D,
    previous: &UiState,
    state: &UiState,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    match (&previous.screen, &state.screen) {
        (Screen::MainMenu { selected: old }, Screen::MainMenu { selected }) => {
            menu::update_main_selection(display, *old, *selected)
        }
        (Screen::Settings { selected: old }, Screen::Settings { selected }) => {
            menu::update_settings_selection(display, *old, *selected)
        }
        (Screen::ContactsList { selected: old }, Screen::ContactsList { selected })
            if previous.contacts == state.contacts =>
        {
            menu::update_contact_manager_selection(display, &state.contacts, *old, *selected)
        }
        (
            Screen::SettingsSection {
                section: old_section,
                selected: old,
            },
            Screen::SettingsSection { section, selected },
        ) if old_section == section => {
            settings::update_selection(display, *section, *old, *selected)
        }
        (Screen::ChatsList { selected: old }, Screen::ChatsList { selected })
            if previous.contacts == state.contacts =>
        {
            menu::update_contact_selection(display, &state.contacts, *old, *selected)
        }
        (
            Screen::ChatView {
                conversation: old_conversation,
                unencrypted_dm: old_unencrypted,
                input: old_input,
                menu_open: old_menu_open,
                selected_message: old_selected_message,
                ..
            },
            Screen::ChatView {
                conversation,
                unencrypted_dm,
                input,
                menu_open,
                selected_message,
                ..
            },
        ) => {
            if previous.messages != state.messages || old_selected_message != selected_message {
                chat::draw_message_panel(
                    display,
                    *conversation,
                    &state.messages,
                    *selected_message,
                )?;
            }
            if old_conversation != conversation || old_unencrypted != unencrypted_dm {
                chat::draw_header(display, *conversation, *unencrypted_dm)?;
            }
            if old_input != input {
                chat::draw_input(display, input.as_str())?;
            }
            if old_menu_open != menu_open {
                if *menu_open {
                    chat::draw_context_menu(display, *unencrypted_dm)?;
                } else {
                    chat::clear_context_menu(display)?;
                }
            }
            Ok(())
        }
        _ => {
            clear_page_area(display)?;
            draw_page(display, state)
        }
    }
}

fn clear_page_area<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    Rectangle::new(Point::new(0, 24), Size::new(320, 216))
        .into_styled(PrimitiveStyle::with_fill(style::BLACK))
        .draw(display)
        .map(|_| ())
}

fn draw_page<D>(display: &mut D, state: &UiState) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    match &state.screen {
        Screen::MainMenu { selected } => menu::draw_main(display, *selected),
        Screen::ChatsList { selected } => menu::draw_contacts(display, &state.contacts, *selected),
        Screen::ContactsList { selected } => {
            menu::draw_contact_manager(display, &state.contacts, *selected)
        }
        Screen::DeviceSearch { selected } => {
            settings::draw_device_search(display, &state.discovered_devices, *selected)
        }
        Screen::DiscoveredDevice {
            device_id,
            selected,
        } => settings::draw_discovered_device(display, device_id, *selected),
        Screen::UnencryptedDmWarning { device_id, .. } => {
            settings::draw_unencrypted_dm_warning(display, device_id)
        }
        Screen::PairingEntry {
            device_id, secret, ..
        } => settings::draw_pairing_entry(display, device_id, secret.as_str()),
        Screen::PairingStatus { device_id, status } => {
            settings::draw_pairing_status(display, device_id, *status)
        }
        Screen::AnnouncementStatus { status } => {
            settings::draw_announcement_status(display, *status)
        }
        Screen::Navigation {
            target_latitude_microdegrees,
            target_longitude_microdegrees,
            ..
        } => navigation::draw(
            display,
            state.gps_snapshot,
            state.movement_course_degrees,
            *target_latitude_microdegrees,
            *target_longitude_microdegrees,
        ),
        Screen::Settings { selected } => menu::draw_settings(display, *selected),
        Screen::SettingsSection { section, selected } => {
            settings::draw(display, *section, *selected)
        }
        Screen::SettingsItem { section, item } => settings::draw_item(
            display,
            *section,
            *item,
            state.device_id,
            state.firmware_version,
            state.radio_config,
        ),
        Screen::ChatView {
            conversation,
            unencrypted_dm,
            input,
            menu_open,
            selected_message,
            ..
        } => chat::draw(
            display,
            *conversation,
            &state.messages,
            input.as_str(),
            *menu_open,
            *unencrypted_dm,
            *unencrypted_dm
                || match conversation {
                    super::state::ConversationKind::Direct(device_id) => {
                        state.contacts.direct_key(device_id).is_some()
                    }
                    super::state::ConversationKind::Broadcast => false,
                },
            *selected_message,
        ),
    }
}
