# Board wiring and pin map

This is the wiring the firmware expects on the Waveshare ESP32-S3-LR1121-HF-Kit. GPIO numbers match `src/bin/hardware/pinout.rs` and the initialization in `src/bin/main.rs`. Verify the exposed pins against the board schematic before soldering; this document is not a substitute for the vendor schematic. Keep all logic at 3.3 V and do not drive ESP32 GPIOs with 5 V.

## Pin assignments

| GPIO | Function | Notes |
| ---: | --- | --- |
| 4 | GPS UART1 RX | Connect to the GPS module's TX. |
| 5 | GPS UART1 TX | Connect to the GPS module's RX. |
| 7 | TFT backlight PWM | Active-high input; driven by LEDC at 20 kHz. |
| 8 | TFT reset | |
| 9 | TFT D/C | Data/command select. |
| 10 | TFT chip select | Dedicated; do not share with the radio. |
| 11 | TFT MOSI | SPI3. |
| 12 | TFT SCLK | SPI3. |
| 16 | MCP23017 INTA | Open-drain active-low interrupt; firmware enables the internal pull-up. |
| 17 | I2C0 SDA | MCP23017 SDA, 4.7 kΩ pull-up to 3V3. |
| 18 | I2C0 SCL | MCP23017 SCL, 4.7 kΩ pull-up to 3V3. |
| 38 | LR1121 IRQ | Radio interrupt input. |
| 39 | LR1121 reset | |
| 40 | LR1121 SCLK | SPI2. |
| 41 | LR1121 BUSY | |
| 42 | LR1121 NSS | SPI2 chip select. |
| 45 | LR1121 MOSI | SPI2. |
| 46 | LR1121 MISO | SPI2. |
| 43/44 | UART0 TX/RX | Keep for programming and console output. |

The LR1121 uses SPI2 at 1 MHz and the display uses SPI3 at 10 MHz; they are separate buses. The LR1121's RF switch is controlled by the radio's own DIO pins through `SetDioAsRfSwitch`, so it needs no external GPIO. The board front end must still physically support the selected frequency.

## GPS (u-blox NEO-M6N class module)

The firmware opens UART1 at 9600 baud, 8-N-1, and parses NMEA GGA and RMC sentences. It polls the UART without blocking, assembles sentences in a fixed buffer, and ignores checksum/format errors (exposed through `last_sentence_error`). A GGA with an invalid fix or an RMC with invalid fix status clears the cached coordinate. RMC provides the date; GGA time without a date keeps the last known date.

| ESP32-S3 | GPS module |
| --- | --- |
| GPIO4 (UART1 RX) | module TX |
| GPIO5 (UART1 TX) | module RX |
| 3V3 (confirm module rating) | VCC |
| GND | GND |

Confirm the specific breakout's supply range and current draw. If the module has been reconfigured before, do not assume it is still at its factory baud rate; check with a serial monitor before relying on parsing. The supported module has no PPS output, so the firmware does not use PPS timestamps.

## Display (ILI9341 SPI TFT)

SPI3 is dedicated to the display; the microSD socket on many such modules is unused. The firmware initializes `ILI9341Rgb565` with a 90° rotation and a horizontal flip, so the 320×240 panel is driven in the orientation the UI expects.

| ESP32-S3 | TFT |
| --- | --- |
| GPIO12 | SCLK |
| GPIO11 | MOSI/SDI |
| GPIO10 | CS (active low) |
| GPIO9 | D/C |
| GPIO8 | RESET |
| GPIO7 | LED/backlight PWM input |
| 3V3 | VCC/logic |
| GND | GND |
| — | MISO/SDO not connected (write-only) |

Backlight brightness is the `display_brightness` setting, loaded at boot (default 80) and applied to GPIO7 through LEDC PWM. There is no on-device brightness screen, so changing it currently means writing a new value into flash settings and rebooting.

If a board revision exposes the bare backlight LEDs instead of a logic-level enable input, use a transistor driver with current limiting rather than driving LEDs from GPIO7.

## Keypad (MCP23017 5×5 matrix)

The expander is at I²C address `0x20` (A0–A2 tied low) on a 400 kHz bus. PA0–PA3 read rows 0–3 with pull-ups, PA4–PA7 drive columns 0–3, PB0 drives column 4, and PB1 reads row 4. INTB is mirrored onto INTA, which is configured open-drain active-low, so GPIO16's pull-up sets the idle high level.

### Solder list (PDIP-28 / SOIC-28 / SSOP-28 numbering)

| MCP pin | Label | Connect to |
| ---: | --- | --- |
| 9 | VDD | 3V3 |
| 10 | VSS | GND |
| 15, 16, 17 | A0, A1, A2 | GND (address `0x20`) |
| 18 | RESET | 3V3 through 10 kΩ, or a controlled GPIO |
| 12 | SCL | GPIO18, 4.7 kΩ to 3V3 |
| 13 | SDA | GPIO17, 4.7 kΩ to 3V3 |
| 20 | INTA | GPIO16 (external 10 kΩ optional) |
| 19 | INTB | leave open (mirrored to INTA) |
| 11, 14 | NC | leave open |
| 21–24 | PA0–PA3 | rows 0–3 |
| 25–28 | PA4–PA7 | columns 0–3 |
| 1 | PB0 | column 4 |
| 2 | PB1 | row 4 |
| 3–8 | PB2–PB7 | leave open |

Breakouts usually silkscreen PA0–PA7/PB0–PB7, and the two headers are often in opposite order; follow the printed labels rather than pin position. Tying any of A0–A2 high changes the address and the keypad stops responding; if the breakout already ties them low, do not strap them elsewhere.

Parts for one keypad: one MCP23017, 24 keys (one cell is unused), 24 × 1N4148-class diodes, two 4.7 kΩ I²C pull-ups, one 10 kΩ reset resistor.

### Key grid

| row \ col | C0 PA4 | C1 PA5 | C2 PA6 | C3 PA7 | C4 PB0 |
| --- | --- | --- | --- | --- | --- |
| R0 PA0 | `0` | `1` | `2` | `3` | `SEND` |
| R1 PA1 | `4` | `5` | `6` | `7` | `BACK` |
| R2 PA2 | `8` | `9` | `A` | `B` | `ENTER` |
| R3 PA3 | `C` | `D` | `E` | `F` | `HELP` |
| R4 PB1 | `UP` | `DOWN` | `LEFT` | `RIGHT` | unused |

### Per-key diode

Every cell is the switch in series with a diode pointing at the column:

```text
row line ────[ switch ]────|>|──── column line
                             ^
                             cathode (band) faces the column
```

Either series order works along the cell; only the diode direction matters. One diode per key is required: without them, three keys forming a rectangle connect a driven-high column through two switches to the low column, so two column outputs fight each other far beyond the expander's per-pin current rating, and a floating-column design also reports a phantom fourth key. With diodes, current can only flow row → switch → column. The 100 kΩ row pull-up limits current to roughly 33 µA, so a silicon diode drops only about 0.3 V, well below the expander's input-low threshold, and reverse leakage is negligible.

Do not fit a key at row 4 column 4. A practical build is five horizontal row wires and five vertical column wires with one key-plus-diode across each crossing; keep the diode close to the key and check the band direction before soldering all of them.

### How the firmware scans

At configuration the driver writes `IOCON=0x44` (mirror INTB to INTA, open-drain active-low), `IODIRA=0x0F`, `IODIRB=0xFE`, `IPOL=0`, `GPPUA=0x0F`, `GPPUB=0xFE`, `INTCON=0` (interrupt on any change), `GPINTENA=0x0F`, `GPINTENB=0x02`, and idle `OLATA=0`, `OLATB=0` in the reset-default BANK=0/byte mode.

Columns rest low, so pressing a key pulls its pulled-up row low and latches the interrupt. The worker waits for the falling edge on GPIO16, then runs three 10 ms scans: each scan drives one column low while the other four go high, reads both ports, then restores idle columns and reads the ports again to clear the latch. A key must read the same for three scans before it is reported, and the burst keeps scanning while a change settles. Idle I²C traffic is zero.

If the expander does not answer, the worker logs a warning, retries every two seconds, and reconfigures when it responds. A missing keypad therefore removes input only; it does not stop the radio, GPS, or display.

For the logical key meanings and the two-hex-digits-per-character behavior, see [display and controls](display-plan.md).

## Consolidated GPIO usage

| GPIO | Assignment |
| ---: | --- |
| 0–3 | Free; review boot strapping before using GPIO0. |
| 4, 5 | GPS UART1. |
| 6 | Free. |
| 7–12 | TFT (backlight, reset, D/C, CS, MOSI, SCLK). |
| 13–15 | Free. |
| 16–18 | Keypad interrupt and I²C. |
| 19–37 | Not assigned by this firmware; check module flash/PSRAM and board access first. |
| 38–42, 45, 46 | LR1121; do not reassign. |
| 43, 44 | UART0 console/programming. |

## Bring-up checklist

1. Compare the connector labels on your board revision with the table above, especially GPIO4–18, and confirm they are not reserved by the kit adapters.
2. Verify each module's supply range, logic levels, pull-ups, and current draw. Do not power the TFT backlight from a GPIO.
3. Confirm GPS NMEA output on GPIO4/5 at the expected baud rate.
4. Confirm the ILI9341 initializes on SPI3 and that CS/D-C/RESET/backlight behave as expected.
5. Bring up the MCP23017 at address `0x20` on GPIO17/18, confirm INTA pulses low on a press and returns high after a scan, then test all 25 cells. Press three keys that form a rectangle to confirm the diodes prevent a phantom key.
6. Leave the LR1121 and GPS pins unchanged when adding the expander.

Wiring is one part of board verification; RF range, antenna suitability, display behavior, keypad soldering, and regulatory limits all require physical testing that the repository does not record.