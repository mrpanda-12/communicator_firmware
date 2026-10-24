# Display UI Plan (ILI9341 2.4" SPI)

## Hardware
- Display: ILI9341 2.4" SPI TFT (planned)
- Pins (from docs/pinout.md): GPIO12 SCK, GPIO11 MOSI, GPIO10 CS, GPIO9 D/C, GPIO8 RESET, GPIO7 BL
- SPI: Use SPI3 (SPI2 is taken by LR1121)
- Driver: `mipidsi` + `embedded-graphics` + `display-interface-spi` (recommended)

## Layout
```
+----------------------------------+
| GPS: XX.XXXX,YY.YYYY | HH:MM | FX|  <- TOP BAR (always visible)
+----------------------------------+
|                                  |
|           CURRENT SCREEN         |
|                                  |
+----------------------------------+
| [1][2][3][4][5]                  |
| [6][7][8][9][0]                  |
| [Q][W][E][R][T]                  |
| [A][S][D][F][G]                  |
| [<-][^][v][->][ENT]              |  <- ARROW ROW (bottom)
+----------------------------------+
```

- Top bar is persistent - shows GPS coords/time/fix status, updated from GPS snapshot via `embassy_sync::watch` from `gps_task`. If GPS disabled or no fix, show appropriate status.
- Bottom row is arrow keys (left/up/down/right/enter). Top 4 rows form the character grid for typing.
- Entire UI driven by single state machine.

## GPS
- Add `gps_enabled: bool` in settings (default true)
- In `gps_task`, if disabled skip polling; just sleep. No hardware changes needed.
- Publish `GpsSnapshot { fix: bool, lat: f64/f32, lon: ..., time: ..., enabled: bool, sats: ... }` via `embassy_sync::watch` for UI
- Top bar shows status regardless (NO FIX / OFF / coords)

## Boot/Menu Flow
```
MAIN MENU
├── CHATS  -> merged chats list
└── SETTINGS
```

## Chats (Merged Message + Send)
**No separate SEND screen** - everything is in one chat view.

### CHATS List
```
CHATS
> BROADCAST        [PUBLIC]
  Alice            "see ya later"   14:32
  Bob              "got it"         13:58
  UNKNOWN AB12...   "?"              12:41
```

- **Broadcast** is treated as a pseudo-conversation at the top (like a contact). Unencrypted, `to = [0x00;16]`. Clearly labeled `[PUBLIC - UNENCRYPTED]`.
- Known contacts show `custom_name` if set, else shortened device ID
- Unknown senders can appear if they messaged - can add to contacts/block from chat menu
- Sorted by last activity (most recent first)

### Chat View (per conversation)
```
+----------------------------------+
| < | Alice / BROADCAST | ⋮        |  <- back, name/id, 3-dot menu
+----------------------------------+
|                                  |
| THEM: hey                        |
| YOU:  yep                        |
| THEM: [COORDS] 59.9139, 10.7522  |
|                                  |
+----------------------------------+
| [TYPE MESSAGE...............]    |  <- text input area
+----------------------------------+
```

- UP/DOWN to scroll history
- Type using 5x5 grid, arrows to navigate cursor
- ENTER to send/focus input
- 3-dot menu (⋮) while in input opens context menu

### 3-dot Menu (in text input)
```
┌─────────────────────────┐
│ SEND COORDS             │
│ CANCEL                  │
└─────────────────────────┘
```

- **SEND COORDS**: Pull from GPS snapshot. If no fix → toast "NO GPS FIX". Build coords payload (lat/lon as f32 LE, 8 bytes). Send with same encryption rules as message (encrypted if DM to known contact, unencrypted if broadcast). Display as `[COORDS] lat, lon` in chat.
- **CANCEL**: Close menu

## Encryption Rules
- **Broadcast** (`ConversationKind::Broadcast`, `to=[0x00;16]`): `is_encrypted = false`, plaintext payload
- **DM to known contact**: `is_encrypted = true`, encrypt with contact's key via `crypto::encrypt`. Each recipient gets separate packet if sending to multiple (own nonce/tag)
- **DM to unknown**: Don't allow encrypted sending unless added to contacts first. Can read but send disabled or prompt to add
- Payload format when encrypted: `nonce (12) || ciphertext || tag (16)`

## Message Storage
- Save up to **N=50 messages per conversation** (ring buffer, prune oldest on overflow)
- Keyed by device ID: DMs use peer device ID, broadcast uses special key (`[0xFF;16]` or similar)
- Store decrypted plaintext in flash for fast UI
- Lazy load only the active conversation
- Save on send and on receive (after reassembly/decryption)
- Track unread count per conversation

## Settings Structure
```
SETTINGS
├── Device
│   ├── View Device ID (hex)
│   ├── Firmware Version
│   └── Reset Identity (confirm)
├── Contacts
│   ├── Broadcast Device ID  (send DeviceAnnounce)
│   ├── Search for Devices   (discover announces, prompt for shared password)
│   ├── View/Edit/Delete     (select contact -> edit name/delete)
│   └── Blacklist
├── Frequency
│   └── Change frequency (show current, cycle; may require restart)
└── GPS
    ├── Enable/Disable
    └── Show Status
```

### Contact Discovery & Pairing
**Broadcaster**: `Settings > Contacts > Broadcast Device ID` → queue `DeviceAnnounce` packet (broadcast, empty payload, `PacketType::DeviceAnnounce = 0x05`)

**Searcher**: `Settings > Contacts > Search for Devices` → discover mode for ~10-15s, collect announces (ignore self). Select device → prompt for shared password. Derive key with `crypto::derive_pair_key(password, &my_id, &peer_id)` (sort IDs, domain `communicator-pair-v1`). Save contact with derived key.

### Clicking device ID vs custom name
- In chat list: if contact has custom name → show it; else show shortened hex
- On unknown sender: show "UNKNOWN <short hex>", can press ENTER for menu: ADD TO CONTACTS | BLOCK | VIEW ID | BACK

## State Machine (Draft)
```rust
#[derive(Clone, Debug)]
enum Screen {
    MainMenu { index: usize },
    ChatsList { index: usize },
    ChatView {
        kind: ConversationKind,
        scroll: usize,
        input: heapless::String<200>,
        cursor: usize,
        menu_open: bool,
    },
    Settings { index: usize },
    SettingsDevice,
    SettingsContacts { index: usize, mode: ContactsMode },
    SettingsFrequency,
    SettingsGps,
    Discovering { expires_at: u32, selected: usize },
    PasswordPrompt { target_device: [u8;16], input: heapless::String<64> },
}
```

## Implementation Order
1. **Data layer** - `ui/state.rs`, conversation/message types integrated with storage
2. **GPS snapshot sharing** - `embassy_sync::watch` in `gps_task`, top bar display
3. **Navigation/Input** - Key mapping for 5x5 grid + arrows, screen stack
4. **Top bar** - Always render first
5. **ChatsList + ChatView** - Merged UI, broadcast as pseudo-convo
6. **3-dot menu + SEND COORDS** - Context menu in input area
7. **Contact discovery** - DeviceAnnounce send/receive, search UI, pairwise key derivation
8. **Flash message persistence** - Save/load with N=50 cap, lazy load
9. **Settings pages** - Device/Contacts/Frequency/GPS
10. **Polish** - Unread counts, toasts, blacklisting

## Notes
- Don't implement groups yet (v2). Focus on solid DMs + broadcast first.
- Discovery uses existing announce mechanism, no key exchange over radio.
- GPS toggle stops polling in `gps_task` without hardware changes.
- Multiple recipients: send separate packets per contact (each encrypted with their key).
- Messages stored decrypted in flash for UI responsiveness.
