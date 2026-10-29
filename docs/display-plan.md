# Display and controls

The UI is a state machine in `src/bin/ui/state.rs` with drawing code in `src/bin/ui/render/`. It drives a 320×240 ILI9341 display. Hardware input comes from the MCP23017 5×5 keypad in `src/bin/hardware/keypad.rs`; there is no simulated navigator in the runtime. The keypad worker publishes debounced key events and the UI loop applies them to `UiState`.

## Physical key layout

Rows and columns map to this logical layout, with row 0 at the top:

```text
0 1 2 3 SEND
4 5 6 7 BACK
8 9 A B ENTER
C D E F HELP
UP DOWN LEFT RIGHT (unused)
```

Each matrix press produces one logical event after three stable 10 ms scans. The worker waits for the MCP23017 interrupt between scans and retries an unresponsive expander every two seconds. The scanner reports a newly pressed key, not a repeating held-key event. The hardware documentation explains row/column pins, address straps, interrupt wiring, and diode direction.

- `UP` and `DOWN` move list selection; in a chat with an empty composer they select history rows and page through older/newer records.
- `LEFT`/`RIGHT` move the text cursor where supported and adjust UTC offset in its setting.
- `ENTER` and `SEND` activate the same primary action.
- `BACK` dismisses a pending hex nibble first, then deletes a character if applicable, then navigates back.
- `HELP` opens or closes the legend overlay. The overlay consumes input except Help, Back, and Enter.
- Every key except HELP dismisses an active popup first, without passing the press on to the screen underneath. HELP toggles the legend even while a popup is visible.

For text entry, digits A–F are hexadecimal nibbles. Two key presses form one ASCII byte: `4` followed by `5` adds `E` (hex `0x45`). A single first nibble is shown pending, such as `4_`, until the second arrives. Non-printable ASCII is ignored. This is how both the chat composer and pairing secret entry receive characters.

## Main screens

### Main menu

The menu opens Chats, Contacts, or Settings. Up/Down selects; Enter opens; Back returns from a child screen.

### Chats list

The list includes the reserved broadcast conversation and visible known or heard peers, plus the clear-history row. Enter opens the selected conversation; direct chats without a saved pairing key show an explicit unencrypted warning. Right opens contact-management actions for a direct peer. The clear-history row requests deletion of all conversation message keys; it does not delete the device ID or settings.

A broadcast message row displays the sender label before the message. For paired/saved peers that label is the contact display name (currently custom name or full hex ID); for an unknown sender it is a short `UNHEARD` ID. That label is derived from the stored message's sender ID; it is not part of the broadcast payload.

### Contacts and discovery

Contacts lists saved contacts, nearby IDs, and a scan action. Nearby IDs are learned from accepted packet origins; scan queues an unauthenticated broadcast `DeviceAnnounce`. No automatic reply is sent, so a quiet peer may not be visible until it transmits. Selecting an unknown peer offers an unencrypted chat warning or pairing.

Pairing uses the same strong shared secret entered on both devices. The UI requires at least 16 characters. Since two hex keys form an ASCII character, physical entry uses pairs. The secret is not transmitted; a derived key is saved only after the handshake succeeds. The application does not provide a password manager or an automatic peer challenge UI beyond this workflow.

### Chat view and history scrolling

A chat displays four records at a time, loaded from flash in chronological order within the selected page. Up selects the preceding row; Down selects the next row. At the first row, Up loads the preceding four-record page when one exists. At the last row, Down loads the following page; after the newest record, one more Down clears selection. Up/Down do not scroll history while there is draft text or the send menu is open. The text composer is bounded by the 512-byte protocol limit.

A new message/history revision reloads the newest page. The UI retains only the visible rows in RAM; older records are fetched by index from flash when requested. Enter on a selected coordinate row opens the navigation view; otherwise Enter in the composer opens the send menu, which sends the typed text or the latest valid GPS position. On send, the UI queues all fragment packets, then appends one local history record. A queue or flash failure is logged; a queue may already contain earlier fragments if a later fragment fails.

Message color and prefix behavior:

- Other people's broadcast rows begin with the sender's label.
- Direct received rows show `FROM`; outgoing rows show `ME>` (coordinate rows use `GPS<-` / `GPS->`).
- Encrypted rows are cyan except an outgoing encrypted direct row, which remains muted with `SENDING..` until its ACK is stored, then becomes green.
- Unencrypted rows are muted. Broadcast and plaintext introduction rows have no delivery acknowledgment.

The ACK reports that the receiver accepted and persisted the complete direct message; it does not prove reading or display. Until it arrives, the outbox retransmits the message after 20 s, up to three rounds.

### Coordinates and navigation

A received coordinate is shown as a selectable `GPS` row. Enter opens a simple navigation page with target coordinate, approximate great-circle distance, absolute bearing, and—after enough local movement—an estimated left/right/straight correction. The course comes from successive GPS fixes after at least five metres of movement; it is not a compass heading. With no local fix, the screen waits. See [chat history and navigation](chat-fragmentation-and-navigation.md) for limitations.

## Settings

| Section | Item | Actual behavior |
| --- | --- | --- |
| Device | Device info | Shows the current 16-byte ID and firmware version. |
| Device | Reset identity | Generates and saves a different ID using the TRNG; updates `DEVICE_ID` so the UI and receive handler use it without rebooting. Contacts and history remain on this device. Peers must pair again with the new ID. Failure is shown on the result screen. |
| Frequency | Current frequency | Displays the current worker-published frequency and transmit power. |
| Frequency | Change frequency | Offers configured presets 869.4 MHz sub-GHz and 2.4 GHz. Enter queues a local retune through `RadioControl`; the radio worker applies it at a safe point and publishes the resulting config. Peers are not notified. A full control channel or rejected request reports busy. Hardware-level radio failure is handled by the worker. |
| GPS | Fix status, coordinates, UTC time | Read-only values from the latest GPS snapshot. |
| GPS | UTC offset | Up/Down or Left/Right changes a fixed offset in 30-minute steps between UTC−12 and UTC+14. The new value is written to flash as the key is changed. It adjusts display only, not protocol timestamps. |

The display brightness setting is loaded from flash at boot and applied to GPIO7 PWM. There is not currently a connected brightness-adjustment screen.

## Status bar, popups, and drawing

The top bar shows current GPS status/coordinates and UTC time/date. A nonzero configured offset adds a second local clock/date; offset zero avoids duplicate output. GPS snapshots arrive on a watch and cause the bar to redraw when changed.

A received-message popup can cover any page. The receive side enqueues it only after the accepted message is saved to flash. A key dismisses the popup before the action underneath. The popup channel has bounded capacity; when full it drops its oldest entry so a newer toast can be queued.

`render::draw_if_changed` retains the existing display pixels and updates the changed page/widget when possible. State changes to screens/overlays redraw the page; chat history changes redraw the message panel; GPS/offset changes redraw the bar. Rendering is separated from flash and radio operations: render functions receive display-ready state and do not load messages themselves.

## State-to-subsystem boundary

`UiState` owns screen selection, draft text, selected history row, visible history page, temporary pairing/reset/frequency statuses, and pending actions. The outer UI loop consumes pending actions and invokes the appropriate subsystem:

| UI action | Work outside the state machine |
| --- | --- |
| Send text/coordinates | Build/fragment/encrypt, queue frames, append one sent record. |
| Pair | Derive key on local inputs, queue handshake if this ID initiates, await pairing event, adopt saved contact. |
| Announcement | Allocate packet ID and queue one broadcast `DeviceAnnounce`. |
| Reset identity | Regenerate and save device ID, publish new value through watch. |
| Frequency change | Request worker retune via `RadioControl`. |
| Contact delete/block | Write/remove contact and conversation data in flash. |
| History paging | Read one four-record page by indexed storage ordinal. |
| UTC offset | Persist the value; display applies it to GPS UTC. |

For end-to-end message details and failure behavior see [messaging](messaging.md); for the actual GPIO/I2C matrix wiring see [pinout](pinout.md).