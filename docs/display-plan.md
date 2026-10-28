# User interface

Screens, menus, and input on the ILI9341 TFT (320x240). Sources:
[`src/bin/ui/`](../src/bin/ui/). Hardware wiring lives in [Pinout](pinout.md).

## Screens

```
MAIN MENU
├── CHATS      → one row per known peer + DELETE ALL CHATS
├── CONTACTS   → saved contacts + NEARBY PEERS + SCAN FOR DEVICES
└── SETTINGS   → DEVICE · FREQUENCY · GPS
```

Every conversation the user could have appears in the lists — broadcast first,
then all known peers, whether or not anything was ever sent. A direct message
from a peer without a saved key opens the unencrypted-DM warning screen first;
confirming it opens the chat in unencrypted mode.

### Chats list

- Row: name or device ID, newest-message preview below, and for sent previews
  a badge — "SENDING.." in amber until the ACK arrives, "OK" in green after.
- Known peers render white; peers that were only heard on the air render
  **amber** (`UNKNOWN xx..`) and disappear from the list if the user blocks
  them. They gain a real contact record only through pairing.
- `RIGHT` opens management for the row's peer: DELETE CHAT + CONTACT, or
  BLOCK/UNBLOCK. The last row, `DELETE ALL CHATS`, wipes every conversation
  record after confirm.

### Chat view

Four message rows at a time, browsed with Up/Down when the composer is empty.
Color rules: a sent row of an encrypted chat is green after its ACK, grey
while sending; received rows are cyan when encrypted, muted when public.

Coordinates appear as `GPS->` / `GPS<-` rows; Enter on a selected coordinate
row opens the navigation screen (distance, target bearing, estimated turn).

The composer types through the keypad's hex pairs, with a context menu
(Enter) that sends the typed text or the last GPS fix as a coordinate packet.

### Settings

| Section | Item | Behavior |
| --- | --- | --- |
| Device | Device info | Full device ID and firmware version. |
| Device | Reset identity | Preview only — not implemented. |
| Frequency | Current frequency | Configured frequency and TX power. |
| Frequency | Change frequency | Preview only — retune has no UI path yet. |
| GPS | Fix status / coordinates / UTC time | Info from the GPS snapshot. |
| GPS | UTC OFFSET | Up/Down (or Left/Right) adjusts ±30 min across
UTC−12:00…+14:00; each step saves to flash immediately. |

## Top bar

Persistent 24 px bar: GPS fix/coordinates on the left, `FIX` indicator on the
right, and the clock between them. The clock shows UTC time and date from the
receiver. With a non-zero UTC offset set, the shifted local time and date are
shown next to UTC; both derive from the same reading, so they cannot drift.
When the offset crosses midnight, the local date shifts accordingly
(implementation: civil-date arithmetic in `ui/render/top_bar.rs`).

## Popups

A received message shows a toast over whatever screen is open — lists,
settings, or an open chat. One key press dismisses it before acting. Blocked
peers do not toast.

## Input

The MCP23017 5x5 keypad (and, in `fake-navigator` builds, a scripted driver)
feeds logical keys into `UiState::handle_keypad`:

```
Row 0: 0 1 2 3 SEND    Row 3: C D E F HELP
Row 1: 4 5 6 7 BACK    Row 4: UP DOWN LEFT RIGHT -
Row 2: 8 9 A B ENTER
```

- Two hex digits make one ASCII character (`4`,`5` → `E`); a lone digit shows
  pending in the composer as `4_`. Hex digits are only used by the composer
  and the pairing secret field; lists ignore them.
- BACK clears a pending digit, then deletes, then leaves the screen. ENTER
  (or SEND) confirms menus; arrows select. HELP toggles a legend overlay that
  owns input until closed.
- Every key first dismisses an open popup.

## Rendering

`ui/render/mod.rs` repaints only what changed: rows track their previous
state, and whole pages redraw only when the screen changes, an overlay opens
or closes, or the underlying data differs (for example the contact book).
The top bar redraws when the GPS snapshot or the UTC offset changes.

## State

`ui/state.rs`'s `UiState` owns the screen stack alternatives, the contact
book, chat rows, popups, and pending commands:

| Pending | Consumed by the run loop as |
| --- | --- |
| `pending_message` | fragment + queue + persist a sent message |
| `pending_manage` | block or delete a contact and its history |
| `pending_delete_all` | wipe every conversation record |
| `pending_timezone` | persist the new UTC offset |
| `pending_pairing` | run the handshake and save the contact |
| `pending_announcement` | queue one broadcast `DeviceAnnounce` |

Screen state lives in `UiState`; renderers are pure functions of it. There is
no global mutable render cache beyond the previous-state snapshot used for
diffing.
